//! Updates for the desktop app itself, from the same `node-build-N` GitHub releases the node
//! updates from: CI attaches `kernel-desktop-setup.exe` and its SHA-256 to each one.
//!
//! The installer is checked against that hash, then run silently after the app exits, and the
//! app is started again. A failed install leaves the old version, which also starts again.

use serde::Serialize;
use sha2::{Digest, Sha256};

pub const REPO: &str = "B0RE16/laughing-waffle";
pub const ASSET: &str = "kernel-desktop-setup.exe";

/// The CI run that built this app; 0 for builds made on a PC (those never update).
pub fn build() -> u64 {
    option_env!("KERNEL_BUILD")
        .and_then(|b| b.parse().ok())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Available {
    pub build: u64,
    pub installer: String,
    pub sha256: String,
}

/// The newest release that carries the desktop installer, from GitHub's release list.
pub fn newest(releases: &serde_json::Value) -> Option<Available> {
    releases
        .as_array()?
        .iter()
        .filter(|r| {
            !r["draft"].as_bool().unwrap_or(false) && !r["prerelease"].as_bool().unwrap_or(false)
        })
        .filter_map(|r| {
            let build: u64 = r["tag_name"]
                .as_str()?
                .strip_prefix("node-build-")?
                .parse()
                .ok()?;
            let asset = |name: &str| {
                r["assets"].as_array()?.iter().find(|a| a["name"] == name)?["browser_download_url"]
                    .as_str()
                    .map(str::to_owned)
            };
            Some(Available {
                build,
                installer: asset(ASSET)?,
                sha256: asset(&format!("{ASSET}.sha256"))?,
            })
        })
        .max_by_key(|a| a.build)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("kernel-desktop/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}

async fn get(url: &str) -> Result<Vec<u8>, String> {
    let resp = client()?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("couldn't reach GitHub ({e})"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub answered {}", resp.status()));
    }
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

/// A newer build than this one, if there is one.
pub async fn check() -> Result<Option<Available>, String> {
    let body = get(&format!(
        "https://api.github.com/repos/{REPO}/releases?per_page=30"
    ))
    .await?;
    let releases: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    Ok(newest(&releases).filter(|a| a.build > build()))
}

/// The hex hash at the start of a `sha256sum` line.
pub fn expected_hash(text: &str) -> Option<String> {
    let h = text.split_whitespace().next()?.to_ascii_lowercase();
    (h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())).then_some(h)
}

/// Download and verify the installer; returns where it was saved.
pub async fn download(a: &Available) -> Result<std::path::PathBuf, String> {
    let sums = String::from_utf8_lossy(&get(&a.sha256).await?).into_owned();
    let expected = expected_hash(&sums).ok_or("the checksum file is malformed")?;
    let bytes = get(&a.installer).await?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != expected {
        return Err(format!(
            "checksum mismatch (expected {expected}, got {actual})"
        ));
    }
    let path = std::env::temp_dir().join(format!("kernel-desktop-setup-{}.exe", a.build));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Run the installer silently once this app has exited, then start the app again.
#[cfg(windows)]
pub fn run_installer(installer: &std::path::Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let app = std::env::current_exe().map_err(|e| e.to_string())?;
    // ping is the console-free way to wait a moment for this process to exit.
    let script = format!(
        r#"/C ping -n 3 127.0.0.1 >nul & "{}" /S & start "" "{}""#,
        installer.display(),
        app.display()
    );
    std::process::Command::new("cmd")
        .raw_arg(script)
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("couldn't start the installer: {e}"))
}

#[cfg(not(windows))]
pub fn run_installer(_installer: &std::path::Path) -> Result<(), String> {
    Err("updates are only installed on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release(tag: &str, with_desktop: bool) -> serde_json::Value {
        let mut assets =
            vec![json!({"name": "kernel-node-windows-x64.zip", "browser_download_url": "z"})];
        if with_desktop {
            assets.push(
                json!({"name": ASSET, "browser_download_url": format!("https://x/{tag}/exe")}),
            );
            assets.push(json!({"name": format!("{ASSET}.sha256"), "browser_download_url": format!("https://x/{tag}/sha")}));
        }
        json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": assets})
    }

    #[test]
    fn picks_the_newest_release_with_an_installer() {
        let list = json!([
            release("node-build-150", false),
            release("node-build-149", true),
            release("node-build-120", true),
            release("something-else", true),
        ]);
        let a = newest(&list).unwrap();
        assert_eq!(a.build, 149);
        assert_eq!(a.installer, "https://x/node-build-149/exe");
        assert!(newest(&json!([release("node-build-123", false)])).is_none());
    }

    #[test]
    fn reads_sha256sum_lines() {
        let h = "c544012d9c704b6af345e15d7c7bc1f83c17c7fed51c706e3c96dd7c0743d452";
        assert_eq!(
            expected_hash(&format!("{h} *kernel-desktop-setup.exe\n")).as_deref(),
            Some(h)
        );
        assert_eq!(expected_hash(&h.to_uppercase()).as_deref(), Some(h));
        assert!(expected_hash("nope").is_none());
    }
}
