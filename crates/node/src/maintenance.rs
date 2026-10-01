//! Upkeep: reading logs from the app, a diagnostics zip, and nightly database backups.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use crate::config::Config;

/// The last `lines` lines of a text file, reading at most the last 1 MB.
pub fn tail(path: &Path, lines: usize) -> std::io::Result<Vec<String>> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(1024 * 1024);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let mut all: Vec<&str> = text.lines().collect();
    if start > 0 && !all.is_empty() {
        all.remove(0); // probably cut in half
    }
    Ok(all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|l| l.to_string())
        .collect())
}

/// kerneld's own log rolls daily (`kerneld.log.2026-09-30`); the newest one.
pub fn newest_log(dir: &Path, prefix: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

/// Where a module's log is (or kerneld's, for the node itself).
pub fn log_path(cfg: &Config, module: &str) -> Option<PathBuf> {
    if module == crate::builtin::ID {
        return newest_log(&cfg.logs_dir(), "kerneld.log");
    }
    let valid = !module.is_empty()
        && module
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    valid.then(|| cfg.logs_dir().join("modules").join(format!("{module}.log")))
}

/// node.toml without its secrets.
pub fn redacted_config(text: &str) -> String {
    text.lines()
        .map(|line| {
            let key = line.split('=').next().unwrap_or("").trim();
            if ["token", "discord_webhook"].contains(&key) {
                format!("{key} = \"(hidden)\"")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A zip with recent logs, the config (secrets hidden) and `extra` JSON files, for when
/// something breaks. Returns its path.
pub fn diagnostics(cfg: &Config, extra: &[(&str, serde_json::Value)]) -> std::io::Result<PathBuf> {
    let dir = cfg.data_dir.join("diagnostics");
    std::fs::create_dir_all(&dir)?;
    let stamp = time_stamp();
    let path = dir.join(format!("kernel-diagnostics-{stamp}.zip"));
    let file = std::fs::File::create(&path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    let mut add = |name: &str, bytes: &[u8]| -> std::io::Result<()> {
        zip.start_file(name, opts).map_err(std::io::Error::other)?;
        zip.write_all(bytes)
    };
    if let Some(p) = &cfg.path
        && let Ok(text) = std::fs::read_to_string(p)
    {
        add("node.toml", redacted_config(&text).as_bytes())?;
    }
    let mut logs: Vec<PathBuf> = newest_log(&cfg.logs_dir(), "kerneld.log")
        .into_iter()
        .collect();
    if let Ok(entries) = std::fs::read_dir(cfg.logs_dir().join("modules")) {
        logs.extend(entries.filter_map(Result::ok).map(|e| e.path()));
    }
    for log in logs {
        let name = format!(
            "logs/{}",
            log.file_name().unwrap_or_default().to_string_lossy()
        );
        let lines = tail(&log, 5_000).unwrap_or_default();
        add(&name, lines.join("\n").as_bytes())?;
    }
    for (name, value) in extra {
        add(
            name,
            serde_json::to_vec_pretty(value)
                .unwrap_or_default()
                .as_slice(),
        )?;
    }
    zip.finish().map_err(std::io::Error::other)?;
    // Keep the last five.
    prune(&dir, "kernel-diagnostics-", 5);
    Ok(path)
}

fn time_stamp() -> String {
    kernel_protocol::now_ts().replace([':', '-'], "")
}

/// Delete all but the newest `keep` files starting with `prefix` (names sort by date).
pub fn prune(dir: &Path, prefix: &str, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(prefix))
        })
        .collect();
    files.sort();
    let extra = files.len().saturating_sub(keep);
    for old in &files[..extra] {
        let _ = std::fs::remove_file(old);
    }
}

/// Copy node.db (activity and events) to `data/backups`, keeping a week.
pub fn backup(cfg: &Config, store: &crate::activity::ActivityStore) -> Result<PathBuf, String> {
    let dir = cfg.data_dir.join("backups");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let day = &kernel_protocol::now_ts()[..10];
    let path = dir.join(format!("node-{day}.db"));
    let _ = std::fs::remove_file(&path); // VACUUM INTO won't overwrite
    store.backup_to(&path).map_err(|e| e.to_string())?;
    prune(&dir, "node-", 7);
    Ok(path)
}

pub const BACKUP_EVERY: Duration = Duration::from_secs(24 * 3600);

pub fn diag_summary(path: &Path) -> serde_json::Value {
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    json!({ "saved": path.to_string_lossy(), "bytes": bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails_and_redacts() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.log");
        std::fs::write(
            &p,
            (1..=50).map(|i| format!("line {i}\n")).collect::<String>(),
        )
        .unwrap();
        assert_eq!(tail(&p, 2).unwrap(), ["line 49", "line 50"]);
        assert_eq!(tail(&p, 500).unwrap().len(), 50);
        let cfg = "node_id = \"pluto\"\ntoken = \"secret\"\n[notify]\ndiscord_webhook = \"https://x\"\n[update]\ntoken = \"ghp\"";
        let r = redacted_config(cfg);
        assert!(!r.contains("secret") && !r.contains("https://x") && !r.contains("ghp"));
        assert!(r.contains("node_id = \"pluto\""));
    }

    #[test]
    fn prunes_old_files() {
        let dir = tempfile::tempdir().unwrap();
        for d in [
            "node-2026-09-01.db",
            "node-2026-09-02.db",
            "node-2026-09-03.db",
            "other.txt",
        ] {
            std::fs::write(dir.path().join(d), "x").unwrap();
        }
        prune(dir.path(), "node-", 2);
        let mut left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            ["node-2026-09-02.db", "node-2026-09-03.db", "other.txt"]
        );
    }
}
