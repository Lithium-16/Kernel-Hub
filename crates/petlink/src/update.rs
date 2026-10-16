//! Self-update from this repo's node-build-N releases (the same feed as kerneld).

use anyhow::{Context, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;

pub const REPO: &str = "Lithium-16/Kernel-Hub";
pub const ASSET: &str = "petlink-windows-x64.exe";
const EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Deserialize)]
pub struct GhRelease {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhAsset {
    pub name: String,
    pub browser_download_url: String,
}

pub struct Release {
    pub build: u32,
    pub exe: GhAsset,
    pub sha: GhAsset,
}

pub fn pick_latest(releases: Vec<GhRelease>) -> Option<Release> {
    releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let build = r.tag_name.strip_prefix("node-build-")?.parse().ok()?;
            let find = |n: &str| r.assets.iter().find(|a| a.name == n).cloned();
            Some(Release {
                build,
                exe: find(ASSET)?,
                sha: find(&format!("{ASSET}.sha256"))?,
            })
        })
        .max_by_key(|r| r.build)
}

/// The CI run number baked in at build time; `None` for dev builds.
pub fn current_build() -> Option<u32> {
    option_env!("PETLINK_BUILD")?.parse().ok()
}

fn old_path() -> Option<std::path::PathBuf> {
    Some(
        std::env::current_exe()
            .ok()?
            .with_file_name("petlink.old.exe"),
    )
}

pub fn cleanup_old() {
    if let Some(p) = old_path() {
        let _ = std::fs::remove_file(p);
    }
}

pub fn spawn_checker() {
    if current_build().is_none() {
        tracing::info!("dev build: no self-update");
        return;
    }
    std::thread::Builder::new()
        .name("petlink-update".into())
        .spawn(|| {
            loop {
                match check_now() {
                    Ok(true) => std::process::exit(0),
                    Ok(false) => {}
                    Err(e) => tracing::warn!("update check failed: {e:#}"),
                }
                std::thread::sleep(EVERY);
            }
        })
        .expect("update thread");
}

/// True if it installed an update and started it (the caller should then exit).
pub fn check_now() -> anyhow::Result<bool> {
    let Some(current) = current_build() else {
        return Ok(false);
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .user_agent("petlink")
            .timeout(Duration::from_secs(120))
            .build()?;
        let releases: Vec<GhRelease> = client
            .get(format!(
                "https://api.github.com/repos/{REPO}/releases?per_page=30"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let Some(latest) = pick_latest(releases) else {
            return Ok(false);
        };
        if latest.build <= current {
            return Ok(false);
        }
        let want = client
            .get(&latest.sha.browser_download_url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let want = want
            .split_whitespace()
            .next()
            .context("empty .sha256")?
            .to_ascii_lowercase();
        let exe = client
            .get(&latest.exe.browser_download_url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        if hex::encode(Sha256::digest(&exe)) != want {
            bail!(
                "downloaded build {} doesn't match its SHA-256",
                latest.build
            );
        }
        let me = std::env::current_exe()?;
        let old = old_path().context("no exe path")?;
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&me, &old).context("moving the running exe aside")?;
        if let Err(e) = std::fs::write(&me, &exe) {
            let _ = std::fs::rename(&old, &me);
            return Err(e).context("writing the new exe");
        }
        std::process::Command::new(&me)
            .spawn()
            .context("starting the new version")?;
        tracing::info!("updated to build {}", latest.build);
        Ok(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, names: &[&str], draft: bool) -> GhRelease {
        GhRelease {
            tag_name: tag.into(),
            draft,
            prerelease: false,
            assets: names
                .iter()
                .map(|n| GhAsset {
                    name: (*n).into(),
                    browser_download_url: format!("https://x/{tag}/{n}"),
                })
                .collect(),
        }
    }

    #[test]
    fn picks_the_newest_complete_build() {
        let both = [ASSET, "petlink-windows-x64.exe.sha256"];
        let releases = vec![
            rel("node-build-12", &both, true),     // draft
            rel("node-build-11", &[ASSET], false), // no hash
            rel("node-build-10", &both, false),
            rel("node-build-9", &both, false),
            rel("v1.0.0", &both, false), // not a build tag
        ];
        let r = pick_latest(releases).unwrap();
        assert_eq!(r.build, 10);
        assert!(
            r.exe
                .browser_download_url
                .ends_with("node-build-10/petlink-windows-x64.exe")
        );
        assert!(pick_latest(vec![]).is_none());
    }
}
