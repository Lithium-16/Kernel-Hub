//! Self-update from GitHub Releases.
//!
//! Every merge to `main` publishes a `node-build-<N>` release with `kernel-node-windows-x64.zip`
//! and its `.sha256`. An installed node lives in `<root>/app/kerneld.exe`. To update, it
//! downloads the newest build, checks the hash, unpacks it into `<root>/staging/`, and hands off
//! to `kerneld apply-update` (a copy of the current, known-good binary in the data folder).
//! The helper waits for this process to exit, swaps `<root>/app` for the staged folder,
//! reinstalls the Python SDK that ships with the build, and starts the new kerneld. If the
//! install fails, it puts the previous folder back and starts that instead.
//!
//! Trust: releases come only from the configured repo over HTTPS, and the archive must match
//! the published SHA-256. Whoever can publish releases on that repo can run code on the node.

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kernel_protocol::now_ts;
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::config::{Config, UpdateConfig};

pub const TAG_PREFIX: &str = "node-build-";
/// Windows: start a console program without giving it a window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub const ASSET: &str = "kernel-node-windows-x64.zip";
const RESULT_FILE: &str = "update-result.json";

/// The CI build number this binary was built from (0 for local builds).
pub fn build() -> u64 {
    option_env!("KERNEL_BUILD")
        .and_then(|b| b.parse().ok())
        .unwrap_or(0)
}

pub fn exe_name() -> String {
    format!("kerneld{}", std::env::consts::EXE_SUFFIX)
}

#[derive(Debug, Clone, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: String,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GhAsset {
    name: String,
    /// API URL; fetched with `Accept: application/octet-stream` it serves the file.
    url: String,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub build: u64,
    pub tag: String,
    pub page: String,
    pub published_at: Option<String>,
    zip: GhAsset,
    sha: GhAsset,
}

/// The newest complete, published `node-build-N` release.
fn pick_latest(releases: Vec<GhRelease>) -> Option<Release> {
    releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let build = r.tag_name.strip_prefix(TAG_PREFIX)?.parse().ok()?;
            let find = |name: &str| r.assets.iter().find(|a| a.name == name).cloned();
            let zip = find(ASSET)?;
            let sha = find(&format!("{ASSET}.sha256"))?;
            Some(Release {
                build,
                tag: r.tag_name,
                page: r.html_url,
                published_at: r.published_at,
                zip,
                sha,
            })
        })
        .max_by_key(|r| r.build)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    #[default]
    Idle,
    Checking,
    Downloading,
    Installing,
}

#[derive(Default)]
struct State {
    phase: Phase,
    latest: Option<Release>,
    last_check: Option<String>,
    error: Option<String>,
}

/// What the helper left behind about the last update, shown in the node's status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub ok: bool,
    pub at: String,
    #[serde(default)]
    pub error: Option<String>,
}

pub struct Updater {
    cfg: UpdateConfig,
    config_path: Option<PathBuf>,
    data_dir: PathBuf,
    /// `<root>` when running as `<root>/app/kerneld[.exe]`; `None` for dev builds.
    root: Option<PathBuf>,
    http: reqwest::Client,
    state: Mutex<State>,
    last_outcome: Option<Outcome>,
}

type Res<T> = Result<T, String>;

impl Updater {
    pub fn new(cfg: &Config) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("kerneld/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600))
            .build()
            .expect("HTTP client");
        let last_outcome = std::fs::read(cfg.data_dir.join(RESULT_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        Self {
            cfg: cfg.update.clone(),
            config_path: cfg.path.clone(),
            data_dir: cfg.data_dir.clone(),
            root: std::env::current_exe()
                .ok()
                .and_then(|e| installed_root(&e)),
            http,
            state: Mutex::new(State::default()),
            last_outcome,
        }
    }

    /// Test hook: pretend to be installed under `root`.
    pub fn with_root(mut self, root: PathBuf) -> Self {
        self.root = Some(root);
        self
    }

    pub fn enabled(&self) -> bool {
        !self.cfg.repo.is_empty()
    }

    pub fn config(&self) -> &UpdateConfig {
        &self.cfg
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("update state")
    }

    /// Move to `phase` unless another update step is already running.
    fn begin(&self, phase: Phase) -> Res<()> {
        let mut s = self.state();
        if s.phase != Phase::Idle {
            return Err(format!("an update step is already running ({:?})", s.phase).to_lowercase());
        }
        s.phase = phase;
        Ok(())
    }

    fn end(&self, error: Option<String>) {
        let mut s = self.state();
        s.phase = Phase::Idle;
        s.error = error;
    }

    fn get(&self, url: &str, accept: &str) -> reqwest::RequestBuilder {
        let mut r = self
            .http
            .get(url)
            .header(ACCEPT, accept)
            .header("X-GitHub-Api-Version", "2022-11-28");
        if !self.cfg.token.is_empty() {
            r = r.bearer_auth(&self.cfg.token);
        }
        r
    }

    async fn fetch(&self, url: &str, accept: &str) -> Res<Vec<u8>> {
        let resp = self
            .get(url, accept)
            .send()
            .await
            .map_err(|e| format!("can't reach GitHub: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let hint = if matches!(status.as_u16(), 401 | 403 | 404) && self.cfg.token.is_empty() {
                " (the repo is private: set [update] token in node.toml)"
            } else if matches!(status.as_u16(), 401 | 403 | 404) {
                " (check that [update] token can read the repo's contents)"
            } else {
                ""
            };
            return Err(format!("GitHub answered {status}{hint}"));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("download failed: {e}"))?;
        Ok(bytes.to_vec())
    }

    /// Ask GitHub for the newest build. Returns it when it is newer than this one.
    pub async fn check(&self) -> Res<Option<Release>> {
        if !self.enabled() {
            return Err("updates are off: set [update] repo in node.toml".into());
        }
        self.begin(Phase::Checking)?;
        let url = format!(
            "{}/repos/{}/releases?per_page=30",
            self.cfg.api.trim_end_matches('/'),
            self.cfg.repo
        );
        let result: Res<Option<Release>> = async {
            let body = self.fetch(&url, "application/vnd.github+json").await?;
            let releases: Vec<GhRelease> = serde_json::from_slice(&body)
                .map_err(|e| format!("unexpected reply from GitHub: {e}"))?;
            Ok(pick_latest(releases))
        }
        .await;
        match result {
            Ok(latest) => {
                let newer = latest.clone().filter(|r| r.build > build());
                {
                    let mut s = self.state();
                    s.latest = latest;
                    s.last_check = Some(now_ts());
                }
                self.end(None);
                Ok(newer)
            }
            Err(e) => {
                self.end(Some(e.clone()));
                Err(e)
            }
        }
    }

    /// Download, verify and unpack `rel` into `<root>/staging/<tag>`.
    pub async fn stage(&self, rel: &Release) -> Res<PathBuf> {
        let root = self.root.clone().ok_or_else(|| {
            format!(
                "this kerneld isn't an installed copy (<root>/app/{}), so it can't update itself",
                exe_name()
            )
        })?;
        self.begin(Phase::Downloading)?;
        let result: Res<PathBuf> = async {
            let zip = self.fetch(&rel.zip.url, "application/octet-stream").await?;
            let sha = self.fetch(&rel.sha.url, "application/octet-stream").await?;
            verify_sha256(&zip, &String::from_utf8_lossy(&sha))?;
            let dir = root.join("staging").join(&rel.tag);
            let out = dir.clone();
            tokio::task::spawn_blocking(move || {
                if out.exists() {
                    std::fs::remove_dir_all(&out)
                        .map_err(|e| format!("clearing {}: {e}", out.display()))?;
                }
                extract_zip(&zip, &out)
            })
            .await
            .map_err(|e| e.to_string())??;
            if !dir.join(exe_name()).is_file() {
                return Err(format!("the update has no {}", exe_name()));
            }
            Ok(dir)
        }
        .await;
        self.end(result.as_ref().err().cloned());
        result
    }

    /// Stage the newest build if there is one, then hand off to the helper. `None` means
    /// already up to date. After `Some`, the caller must exit the process soon.
    pub async fn install(&self) -> Res<Option<u64>> {
        let Some(rel) = self.check().await? else {
            return Ok(None);
        };
        let dir = self.stage(&rel).await?;
        self.handoff(Some(&dir))?;
        Ok(Some(rel.build))
    }

    /// Restart kerneld through the helper (it waits for this process to exit first).
    pub fn restart(&self) -> Res<()> {
        self.handoff(None)
    }

    fn handoff(&self, staging: Option<&Path>) -> Res<()> {
        let config = self
            .config_path
            .clone()
            .ok_or("kerneld was started without a config file")?;
        let current = std::env::current_exe().map_err(|e| e.to_string())?;
        // Run the helper from a copy: the app folder is about to be moved.
        let helper_dir = self.data_dir.join("updater");
        std::fs::create_dir_all(&helper_dir).map_err(|e| e.to_string())?;
        let helper = helper_dir.join(format!("kerneld-updater{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(&current, &helper).map_err(|e| format!("copying the update helper: {e}"))?;
        let mut cmd = Command::new(&helper);
        cmd.arg("apply-update")
            .arg("--config")
            .arg(&config)
            .arg("--start")
            .arg(&current);
        if let Some(dir) = staging {
            cmd.arg("--staging").arg(dir);
        }
        spawn_detached(&mut cmd).map_err(|e| format!("starting the update helper: {e}"))?;
        self.state().phase = Phase::Installing;
        Ok(())
    }

    /// Status fields for the built-in Node module.
    pub fn status(&self) -> Map<String, Value> {
        let s = self.state();
        let word = if !self.enabled() {
            "off"
        } else if s.phase != Phase::Idle {
            match s.phase {
                Phase::Checking => "checking",
                Phase::Downloading => "downloading",
                _ => "installing",
            }
        } else if s.error.is_some() {
            "failed"
        } else {
            match &s.latest {
                Some(r) if r.build > build() => "available",
                Some(_) => "current",
                None if s.last_check.is_some() => "none",
                None => "unknown",
            }
        };
        let mut m = Map::new();
        m.insert("version".into(), json!(env!("CARGO_PKG_VERSION")));
        m.insert("build".into(), json!(build()));
        m.insert("update".into(), json!(word));
        m.insert(
            "latest_build".into(),
            json!(s.latest.as_ref().map(|r| r.build)),
        );
        m.insert("last_check".into(), json!(s.last_check));
        if let Some(r) = &s.latest {
            m.insert("release".into(), json!(r.page));
            m.insert("released_at".into(), json!(r.published_at));
        }
        if let Some(o) = &self.last_outcome {
            m.insert("last_update".into(), json!(o.at));
            if let Some(e) = &o.error {
                m.insert("last_update_error".into(), json!(e));
            }
        }
        if let Some(e) = &s.error {
            m.insert("error".into(), json!(e));
        }
        m
    }
}

/// `<root>` when `exe` is `<root>/app/kerneld[.exe]`.
fn installed_root(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    if dir.file_name()? == "app" {
        dir.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

pub fn verify_sha256(bytes: &[u8], published: &str) -> Res<()> {
    let expected = published
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the published checksum is malformed".into());
    }
    let actual = hex::encode(Sha256::digest(bytes));
    if actual != expected {
        return Err(format!(
            "checksum mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

/// Unpack a zip, refusing entries that would land outside `dest`.
pub fn extract_zip(bytes: &[u8], dest: &Path) -> Res<()> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("bad archive: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("bad archive: {e}"))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(format!("unsafe path in the archive: {}", entry.name()));
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut data = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut data)
            .map_err(|e| format!("reading {}: {e}", entry.name()))?;
        std::fs::write(&out, data).map_err(|e| format!("writing {}: {e}", out.display()))?;
        // Keep the executable bit on Unix (Windows has no such thing).
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Start a process that outlives this one.
pub fn spawn_detached(cmd: &mut Command) -> std::io::Result<()> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        // Leave the job kerneld runs in (a scheduled task's, say) so the helper survives it.
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        if cmd.spawn().is_ok() {
            return Ok(());
        }
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map(|_| ())
}

// ---------------------------------------------------------------- the helper side

/// Hold the node lock for as long as the process runs. `None` means another kerneld has it.
pub fn acquire_lock(path: &Path) -> std::io::Result<Option<File>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => {
            // Record who holds it, for people (and tests) looking for the running node. A
            // separate file, because Windows won't let other processes read a locked one.
            std::fs::write(path.with_extension("pid"), std::process::id().to_string())?;
            Ok(Some(file))
        }
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

fn wait_for_lock(path: &Path, timeout: Duration) -> Res<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match acquire_lock(path) {
            Ok(Some(_lock)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(250)),
            Ok(None) => return Err("the old kerneld did not exit".into()),
            Err(e) => return Err(format!("lock file: {e}")),
        }
    }
}

fn retry<T>(what: &str, mut f: impl FnMut() -> std::io::Result<T>) -> Res<T> {
    // Windows can hold files briefly after a process exits (antivirus, indexers).
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(500)),
            Err(e) => return Err(format!("{what}: {e}")),
        }
    }
}

/// Replace `<root>/app` with `staging`, then run `install` on it. Rolls back on any failure.
pub fn swap_and_install(
    root: &Path,
    staging: &Path,
    install: impl FnOnce(&Path) -> Res<()>,
) -> Res<()> {
    let app = root.join("app");
    let previous = root.join("app.previous");
    if previous.exists() {
        retry("removing the old backup", || {
            std::fs::remove_dir_all(&previous)
        })?;
    }
    retry("moving the current version aside", || {
        std::fs::rename(&app, &previous)
    })?;
    if let Err(e) = std::fs::rename(staging, &app) {
        let _ = std::fs::rename(&previous, &app);
        return Err(format!("moving the new version in: {e}"));
    }
    if let Err(e) = install(&app) {
        let failed = root.join("app.failed");
        let _ = std::fs::remove_dir_all(&failed);
        let _ = std::fs::rename(&app, &failed);
        let _ = std::fs::rename(&previous, &app);
        return Err(format!(
            "the new version didn't install, so the old one is back: {e}"
        ));
    }
    Ok(())
}

/// A command that never opens a console window on Windows.
fn windowless(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Reinstall the Python SDK wheel that ships in `app/sdk/`, if there is one.
fn install_sdk(cfg: &Config, app: &Path) -> Res<()> {
    let wheel = std::fs::read_dir(app.join("sdk"))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "whl"));
    let Some(wheel) = wheel else { return Ok(()) };
    let out = windowless(&cfg.update.uv)
        .args([
            "pip",
            "install",
            "--reinstall-package",
            "kernel-sdk",
            "--python",
        ])
        .arg(&cfg.python)
        .arg(&wheel)
        .output()
        .map_err(|e| format!("running {}: {e}", cfg.update.uv))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!(
            "uv pip install failed: {}",
            err.lines().last().unwrap_or("")
        ))
    }
}

fn start(cfg: &Config, exe: &Path, config: &Path) -> Res<()> {
    if cfg!(windows) && !cfg.update.scheduled_task.is_empty() {
        let ran = windowless("schtasks")
            .args(["/Run", "/TN", &cfg.update.scheduled_task])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if ran.is_ok_and(|s| s.success()) {
            return Ok(());
        }
    }
    spawn_detached(Command::new(exe).arg("--config").arg(config))
        .map_err(|e| format!("starting {}: {e}", exe.display()))
}

pub struct ApplyArgs {
    pub config: PathBuf,
    pub start: PathBuf,
    pub staging: Option<PathBuf>,
}

/// `kerneld apply-update`: wait for the old process, swap in the staged build, start kerneld.
pub fn apply(args: &ApplyArgs) -> anyhow::Result<()> {
    let cfg = Config::load(&args.config)?;
    wait_for_lock(&cfg.lock_path(), Duration::from_secs(90)).map_err(anyhow::Error::msg)?;
    let outcome = match &args.staging {
        Some(staging) => {
            let root = args
                .start
                .parent()
                .and_then(Path::parent)
                .filter(|_| installed_root(&args.start).is_some())
                .ok_or_else(|| {
                    anyhow::anyhow!("{} is not in an app folder", args.start.display())
                })?;
            swap_and_install(root, staging, |app| install_sdk(&cfg, app))
        }
        None => Ok(()),
    };
    let record = Outcome {
        ok: outcome.is_ok(),
        at: now_ts(),
        error: outcome.as_ref().err().cloned(),
    };
    let _ = std::fs::write(
        cfg.data_dir.join(RESULT_FILE),
        serde_json::to_vec_pretty(&record).unwrap_or_default(),
    );
    start(&cfg, &args.start, &args.config).map_err(anyhow::Error::msg)?;
    outcome.map_err(anyhow::Error::msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn rel(tag: &str, assets: &[&str]) -> GhRelease {
        GhRelease {
            tag_name: tag.into(),
            draft: false,
            prerelease: false,
            html_url: format!("https://example/{tag}"),
            published_at: None,
            assets: assets
                .iter()
                .map(|n| GhAsset {
                    name: (*n).into(),
                    url: format!("https://api/{tag}/{n}"),
                })
                .collect(),
        }
    }

    #[test]
    fn picks_the_newest_complete_release() {
        let sha = format!("{ASSET}.sha256");
        let full = [ASSET, sha.as_str()];
        let mut draft = rel("node-build-9", &full);
        draft.draft = true;
        let releases = vec![
            rel("node-build-3", &full),
            rel("node-build-7", &full),
            rel("node-build-8", &[ASSET]), // no checksum: skipped
            draft,                         // draft: skipped
            rel("v1.0.0", &full),          // not a node build: skipped
            rel("node-build-x", &full),    // not a number: skipped
        ];
        let latest = pick_latest(releases).unwrap();
        assert_eq!(latest.build, 7);
        assert_eq!(
            latest.zip.url,
            "https://api/node-build-7/kernel-node-windows-x64.zip"
        );
        assert!(pick_latest(vec![]).is_none());
    }

    #[test]
    fn verifies_checksums() {
        let data = b"kernel";
        let good = hex::encode(Sha256::digest(data));
        assert!(verify_sha256(data, &format!("{good}  {ASSET}\n")).is_ok());
        assert!(verify_sha256(data, &good.to_uppercase()).is_ok());
        assert!(
            verify_sha256(b"other", &good)
                .unwrap_err()
                .contains("mismatch")
        );
        assert!(
            verify_sha256(data, "nope")
                .unwrap_err()
                .contains("malformed")
        );
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            for (name, data) in files {
                w.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn extracts_safely() {
        let dir = tempfile::tempdir().unwrap();
        let ok = zip_of(&[
            ("kerneld.exe", b"bin"),
            ("modules/hello/main.py", b"print()"),
        ]);
        extract_zip(&ok, dir.path()).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("modules/hello/main.py")).unwrap(),
            b"print()"
        );

        let evil = zip_of(&[("../escape.txt", b"x")]);
        let out = dir.path().join("evil");
        assert!(
            extract_zip(&evil, &out)
                .unwrap_err()
                .contains("unsafe path")
        );
        assert!(!dir.path().join("escape.txt").exists());
    }

    #[test]
    fn finds_the_install_root() {
        let exe = Path::new("/opt/kernel/node/app").join(exe_name());
        assert_eq!(
            installed_root(&exe),
            Some(PathBuf::from("/opt/kernel/node"))
        );
        assert_eq!(
            installed_root(Path::new("/home/me/target/debug/kerneld")),
            None
        );
    }

    fn app_with(root: &Path, name: &str, marker: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("VERSION"), marker).unwrap();
        dir
    }

    fn version(root: &Path, name: &str) -> String {
        std::fs::read_to_string(root.join(name).join("VERSION")).unwrap()
    }

    #[test]
    fn swaps_in_the_new_version() {
        let root = tempfile::tempdir().unwrap();
        app_with(root.path(), "app", "old");
        app_with(root.path(), "app.previous", "older");
        let staged = app_with(root.path(), "staging/node-build-2", "new");
        swap_and_install(root.path(), &staged, |app| {
            assert_eq!(version(app.parent().unwrap(), "app"), "new");
            Ok(())
        })
        .unwrap();
        assert_eq!(version(root.path(), "app"), "new");
        assert_eq!(version(root.path(), "app.previous"), "old");
        assert!(!staged.exists());
    }

    #[test]
    fn rolls_back_when_the_install_fails() {
        let root = tempfile::tempdir().unwrap();
        app_with(root.path(), "app", "old");
        let staged = app_with(root.path(), "staging/node-build-2", "new");
        let err =
            swap_and_install(root.path(), &staged, |_| Err("uv exploded".into())).unwrap_err();
        assert!(err.contains("the old one is back") && err.contains("uv exploded"));
        assert_eq!(version(root.path(), "app"), "old");
        assert_eq!(version(root.path(), "app.failed"), "new");
    }

    #[test]
    fn the_lock_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kerneld.lock");
        let held = acquire_lock(&path).unwrap().expect("first lock");
        assert!(acquire_lock(&path).unwrap().is_none());
        assert!(wait_for_lock(&path, Duration::from_millis(300)).is_err());
        drop(held);
        assert!(wait_for_lock(&path, Duration::from_secs(1)).is_ok());
    }
}
