//! Easy install: the downloaded exe carries the invite as a trailer, copies itself into
//! %LOCALAPPDATA%\Programs\PetLink on first run, and can remove itself again.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"PLINVITE";
const MAX_INVITE: u64 = 64 * 1024;

/// The invite appended to the exe: bytes + u32 LE length + `PLINVITE`.
pub fn read_invite(exe: &Path) -> Option<String> {
    let mut f = std::fs::File::open(exe).ok()?;
    let size = f.metadata().ok()?.len();
    if size < 12 {
        return None;
    }
    f.seek(SeekFrom::Start(size - 12)).ok()?;
    let mut tail = [0u8; 12];
    f.read_exact(&mut tail).ok()?;
    let len = u64::from(u32::from_le_bytes(tail[..4].try_into().ok()?));
    if &tail[4..] != MAGIC || len == 0 || len > MAX_INVITE || len + 12 > size {
        return None;
    }
    f.seek(SeekFrom::Start(size - 12 - len)).ok()?;
    let mut buf = vec![0u8; len as usize];
    f.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

/// The exe without its trailer (unchanged when there is none or it is corrupt).
pub fn strip_trailer(bytes: &[u8]) -> &[u8] {
    let n = bytes.len();
    if n < 12 || &bytes[n - 8..] != MAGIC {
        return bytes;
    }
    let len = u32::from_le_bytes(bytes[n - 12..n - 8].try_into().expect("4 bytes")) as usize;
    if len == 0 || len as u64 > MAX_INVITE || len + 12 > n {
        return bytes;
    }
    &bytes[..n - 12 - len]
}

#[cfg(test)]
fn with_trailer(exe: &[u8], invite: &str) -> Vec<u8> {
    let mut v = exe.to_vec();
    v.extend_from_slice(invite.as_bytes());
    v.extend_from_slice(&(invite.len() as u32).to_le_bytes());
    v.extend_from_slice(MAGIC);
    v
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

/// Install only when started from somewhere else, and never for dev setups.
pub fn should_install(
    current: &Path,
    installed: &Path,
    petlink_dir_set: bool,
    no_install: bool,
) -> bool {
    !(petlink_dir_set || no_install || same_path(current, installed))
}

pub fn install_exe() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("Programs")
            .join("PetLink")
            .join("petlink.exe"),
    )
}

#[cfg(windows)]
fn shortcut_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("PetLink.lnk"),
    )
}

/// The config carried by the running exe's trailer, if any.
pub fn own_invite() -> Option<crate::config::Config> {
    let invite = read_invite(&std::env::current_exe().ok()?)?;
    crate::config::Config::from_invite(&invite).ok()
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::config::Config;
    use anyhow::Context;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED: u32 = 0x0000_0008;

    fn powershell(script: &str, envs: &[(&str, &Path)]) -> anyhow::Result<()> {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-Command", script])
            .creation_flags(NO_WINDOW);
        // Paths travel as environment variables, so no quoting can break them.
        for (k, v) in envs {
            c.env(k, v);
        }
        let out = c.output().context("running powershell")?;
        anyhow::ensure!(out.status.success(), "powershell failed: {}", out.status);
        Ok(())
    }

    fn make_shortcut(lnk: &Path, exe: &Path) -> anyhow::Result<()> {
        if let Some(p) = lnk.parent() {
            std::fs::create_dir_all(p)?;
        }
        let dir = exe.parent().unwrap_or(exe);
        powershell(
            "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:PL_LNK);\
             $s.TargetPath=$env:PL_EXE;$s.WorkingDirectory=$env:PL_DIR;$s.Save()",
            &[("PL_LNK", lnk), ("PL_EXE", exe), ("PL_DIR", dir)],
        )
    }

    /// Copy this exe into place and start it; the caller then exits.
    pub fn run(dir: &Path, name: &str) -> anyhow::Result<()> {
        let me = std::env::current_exe()?;
        let dest = install_exe().context("no LOCALAPPDATA")?;
        let new_cfg = own_invite();
        let old_cfg = Config::load(dir).ok().flatten();
        let other_token = matches!((&new_cfg, &old_cfg), (Some(n), Some(o)) if n.token != o.token);
        // Held until this process exits, so the installed copy waits for us, not for itself.
        let running = !crate::win::single_instance(name);
        if running && !other_token {
            return Ok(()); // already installed and running: nothing to do
        }
        if running {
            // A different invite: stop the running copy so it can be replaced.
            let _ = powershell(
                "Get-Process petlink -ErrorAction SilentlyContinue | \
                 Where-Object { $_.Path -eq $env:PL_EXE } | Stop-Process -Force",
                &[("PL_EXE", &dest)],
            );
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
        let bytes = std::fs::read(&me).context("reading myself")?;
        let old = dest.with_file_name("petlink.old.exe");
        if dest.exists() {
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&dest, &old);
        }
        crate::files::atomic_write(&dest, strip_trailer(&bytes)).context("copying into place")?;
        if let Some(c) = &new_cfg
            && (old_cfg.is_none() || other_token)
        {
            c.save(dir)?;
        }
        if let Some(lnk) = shortcut_path()
            && let Err(e) = make_shortcut(&lnk, &dest)
        {
            tracing::warn!("shortcut: {e:#}");
        }
        // ponytail: PETLINK_NO_AUTOSTART exists only so the manual install test leaves HKCU alone.
        if std::env::var_os("PETLINK_NO_AUTOSTART").is_none()
            && let Err(e) = crate::win::set_autostart_for(&dest)
        {
            tracing::warn!("autostart: {e:#}");
        }
        Command::new(&dest)
            .arg("--installed")
            .spawn()
            .context("starting the installed copy")?;
        Ok(())
    }

    /// Remove the autostart entry, shortcut, settings and (after we exit) the install folder.
    pub fn uninstall(dir: &Path) -> anyhow::Result<()> {
        let _ = crate::win::set_autostart(false);
        if let Some(lnk) = shortcut_path() {
            let _ = std::fs::remove_file(lnk);
        }
        let me = std::env::current_exe()?;
        let mut cmd = String::from("/c ping -n 3 127.0.0.1 >nul");
        if install_exe().is_some_and(|i| same_path(&me, &i))
            && let Some(d) = me.parent()
        {
            cmd += &format!(" & rmdir /s /q \"{}\"", d.display());
        }
        cmd += &format!(" & rmdir /s /q \"{}\"", dir.display());
        Command::new("cmd")
            .raw_arg(cmd)
            .creation_flags(DETACHED | NO_WINDOW)
            .spawn()
            .context("scheduling the cleanup")?;
        Ok(())
    }
}

#[cfg(windows)]
pub use imp::{run, uninstall};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailer_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.exe");
        let body = b"MZ fake exe body".to_vec();
        let inv = "petlink:abc_-\u{e9}";
        let full = with_trailer(&body, inv);
        std::fs::write(&p, &full).unwrap();
        assert_eq!(read_invite(&p).as_deref(), Some(inv));
        assert_eq!(strip_trailer(&full), &body[..]);
    }

    #[test]
    fn no_trailer_or_corrupt_length_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.exe");
        let plain = b"MZ just an exe, long enough to have a tail".to_vec();
        std::fs::write(&p, &plain).unwrap();
        assert_eq!(read_invite(&p), None);
        assert_eq!(strip_trailer(&plain), &plain[..]);
        assert_eq!(read_invite(&dir.path().join("missing")), None);
        // length larger than the file, and zero length
        for len in [1000u32, 0] {
            let mut bad = b"MZ short".to_vec();
            bad.extend_from_slice(&len.to_le_bytes());
            bad.extend_from_slice(MAGIC);
            std::fs::write(&p, &bad).unwrap();
            assert_eq!(read_invite(&p), None);
            assert_eq!(strip_trailer(&bad), &bad[..]);
        }
        std::fs::write(&p, b"PLINVITE").unwrap();
        assert_eq!(read_invite(&p), None);
        assert_eq!(strip_trailer(b"PLINVITE"), b"PLINVITE");
    }

    #[test]
    fn install_decision() {
        let dest = Path::new("C:/x/Programs/PetLink/petlink.exe");
        let dl = Path::new("C:/Users/f/Downloads/PetLink.exe");
        assert!(should_install(dl, dest, false, false));
        assert!(!should_install(dest, dest, false, false));
        let shouty = Path::new("c:/X/programs/petlink/PETLINK.EXE");
        assert!(!should_install(shouty, dest, false, false));
        assert!(!should_install(dl, dest, true, false));
        assert!(!should_install(dl, dest, false, true));
    }
}
