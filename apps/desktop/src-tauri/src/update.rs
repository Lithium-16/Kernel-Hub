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
        let limited = resp
            .headers()
            .get("x-ratelimit-remaining")
            .is_some_and(|v| v == "0");
        return Err(if limited {
            "GitHub's hourly limit for this network is used up".into()
        } else {
            format!("GitHub answered {}", resp.status())
        });
    }
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

/// The release a `releases/latest` redirect points at, with its files' download links.
pub fn from_latest_location(location: &str) -> Option<Available> {
    let tag = location.rsplit('/').next()?;
    let build = tag.strip_prefix("node-build-")?.parse().ok()?;
    let file = |name: String| format!("https://github.com/{REPO}/releases/download/{tag}/{name}");
    Some(Available {
        build,
        installer: file(ASSET.to_string()),
        sha256: file(format!("{ASSET}.sha256")),
    })
}

/// github.com's own `releases/latest`, for when the API refuses (its hourly limit is shared
/// by everything on the home network).
async fn latest_from_web() -> Result<Option<Available>, String> {
    let resp = reqwest::Client::builder()
        .user_agent(concat!("kernel-desktop/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?
        .get(format!("https://github.com/{REPO}/releases/latest"))
        .send()
        .await
        .map_err(|e| format!("couldn't reach github.com ({e})"))?;
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| format!("github.com answered {}", resp.status()))?;
    Ok(from_latest_location(location))
}

/// A newer build than this one, if there is one.
pub async fn check() -> Result<Option<Available>, String> {
    let latest = match get(&format!(
        "https://api.github.com/repos/{REPO}/releases?per_page=30"
    ))
    .await
    {
        Ok(body) => {
            let releases: serde_json::Value =
                serde_json::from_slice(&body).map_err(|e| e.to_string())?;
            newest(&releases)
        }
        Err(api) => latest_from_web()
            .await
            .map_err(|web| format!("{api}; asking github.com directly failed too: {web}"))?,
    };
    Ok(latest.filter(|a| a.build > build()))
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
    fn reads_the_latest_redirect() {
        let a = from_latest_location(
            "https://github.com/B0RE16/laughing-waffle/releases/tag/node-build-164",
        )
        .unwrap();
        assert_eq!(a.build, 164);
        assert_eq!(
            a.installer,
            "https://github.com/B0RE16/laughing-waffle/releases/download/node-build-164/kernel-desktop-setup.exe"
        );
        assert!(
            a.sha256
                .ends_with("node-build-164/kernel-desktop-setup.exe.sha256")
        );
        assert_eq!(
            from_latest_location("https://github.com/x/y/releases/tag/v1"),
            None
        );
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
