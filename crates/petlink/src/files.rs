//! Received files: safe names, no overwrites, and Mark-of-the-Web so Windows treats them as downloads.

use std::path::{Path, PathBuf};

const DANGEROUS: &[&str] = &[
    "exe", "bat", "cmd", "ps1", "vbs", "js", "msi", "scr", "lnk", "com", "pif", "hta", "wsf",
    "wsh", "vbe", "jse", "cpl", "msc", "jar", "reg", "url", "psm1",
];
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Judged on the name it will be saved under, so "a.exe." and "a.exe " don't slip through.
pub fn is_dangerous(name: &str) -> bool {
    Path::new(&safe_name(name))
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| DANGEROUS.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn safe_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !"<>:\"|?*".contains(*c))
        .collect();
    let cleaned = cleaned
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_owned();
    if cleaned.is_empty() {
        return "file".into();
    }
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        format!("_{cleaned}")
    } else {
        cleaned
    }
}

/// Write to a temp file in the same folder, then rename, so a crash never leaves a short file.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = PathBuf::from(tmp);
    let r = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    (1..)
        .map(|i| dir.join(format!("{stem} ({i}){ext}")))
        .find(|c| !c.exists())
        .expect("some free name")
}

pub fn save(dir: &Path, name: &str, bytes: &[u8], source_url: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = unique_path(dir, &safe_name(name));
    atomic_write(&path, bytes)?;
    #[cfg(windows)]
    {
        let mut ads = path.clone().into_os_string();
        ads.push(":Zone.Identifier");
        std::fs::write(
            ads,
            format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={source_url}\r\n"),
        )?;
    }
    #[cfg(not(windows))]
    let _ = source_url;
    Ok(path)
}

pub fn downloads_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("PETLINK_DIR") {
        return PathBuf::from(d).join("Downloads"); // test setups keep to their own folder
    }
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Downloads")
        .join("PetLink")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dangerous_extensions() {
        for n in [
            "a.exe",
            "A.BAT",
            "x.cmd",
            "y.ps1",
            "z.vbs",
            "q.js",
            "i.msi",
            "s.scr",
            "l.lnk",
            "cat.png.exe",
            "a.exe.",
            "a.exe ",
            "a.exe. .",
            "x.hta",
        ] {
            assert!(is_dangerous(n), "{n}");
        }
        for n in ["cat.png", "notes.txt", "exe", "song.mp3"] {
            assert!(!is_dangerous(n), "{n}");
        }
    }

    #[test]
    fn names_cannot_escape_the_folder() {
        assert_eq!(safe_name("..\\..\\Windows\\evil.dll"), "evil.dll");
        assert_eq!(safe_name("a/b/c.txt"), "c.txt");
        assert_eq!(safe_name("CON.txt"), "_CON.txt");
        assert_eq!(safe_name(""), "file");
        assert_eq!(safe_name("bad\u{0}name.txt"), "badname.txt");
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join("f.bin");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
        // a failed write (target is a directory) cleans up its temp file
        let d = dir.path().join("d");
        std::fs::create_dir(&d).unwrap();
        assert!(atomic_write(&d, b"x").is_err());
        assert!(!dir.path().join("d.part").exists());
    }

    #[test]
    fn saves_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let a = save(dir.path(), "cat.png", b"1", "https://relay").unwrap();
        let b = save(dir.path(), "cat.png", b"2", "https://relay").unwrap();
        assert_eq!(a.file_name().unwrap(), "cat.png");
        assert_eq!(b.file_name().unwrap(), "cat (1).png");
        assert_eq!(std::fs::read(&a).unwrap(), b"1");
    }
}
