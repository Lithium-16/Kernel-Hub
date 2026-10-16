//! The client's settings: %APPDATA%\PetLink\config.toml, first filled from an invite.

use anyhow::{Context, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn default_true() -> bool {
    true
}

fn default_scale() -> u8 {
    2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub relay_url: String,
    pub token: String,
    #[serde(default = "default_scale")]
    pub scale: u8,
    /// Overrides how my own pet's bow looks here (friends see the hue the relay has).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bow_hue: Option<u16>,
    /// "Let <friend> peek": the source of truth for the toggle (the relay only mirrors it).
    #[serde(default)]
    pub peek: bool,
    /// "Let pets wander": pets visit each other by themselves.
    #[serde(default = "default_true")]
    pub wander: bool,
    /// Little sounds for letters, doors, pats and games.
    #[serde(default = "default_true")]
    pub sounds: bool,
}

#[derive(Deserialize)]
struct Invite {
    relay_url: String,
    token: String,
}

impl Config {
    pub fn dir() -> PathBuf {
        if let Some(d) = std::env::var_os("PETLINK_DIR") {
            return PathBuf::from(d);
        }
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("PetLink")
    }

    pub fn load(dir: &Path) -> anyhow::Result<Option<Config>> {
        let path = dir.join("config.toml");
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut c: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        c.scale = c.scale.clamp(1, 4);
        Ok(Some(c))
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        crate::files::atomic_write(&dir.join("config.toml"), toml::to_string(self)?.as_bytes())?;
        Ok(())
    }

    pub fn from_invite(invite: &str) -> anyhow::Result<Config> {
        let Some(blob) = invite.trim().strip_prefix("petlink:") else {
            bail!("that isn't a PetLink invite (it starts with petlink:)");
        };
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(blob.trim_end_matches('='))
            .context("the invite is damaged")?;
        let i: Invite = serde_json::from_slice(&bytes).context("the invite is damaged")?;
        Ok(Config {
            relay_url: i.relay_url,
            token: i.token,
            scale: default_scale(),
            bow_hue: None,
            peek: false,
            wander: true,
            sounds: true,
        })
    }

    pub fn files_url(&self) -> String {
        let http = self
            .relay_url
            .replacen("wss://", "https://", 1)
            .replacen("ws://", "http://", 1);
        format!("{}/files", http.trim_end_matches("/ws"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_round_trip_and_files_url() {
        use base64::Engine;
        let blob = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"relay_url":"wss://pluto.x.ts.net:10000/petlink/ws","token":"t0k"}"#);
        let c = Config::from_invite(&format!("  petlink:{blob}\n")).unwrap();
        assert_eq!((c.token.as_str(), c.scale), ("t0k", 2));
        assert_eq!(c.files_url(), "https://pluto.x.ts.net:10000/petlink/files");
        assert!(Config::from_invite("hello").is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(Config::load(dir.path()).unwrap().is_none());
        c.save(dir.path()).unwrap();
        assert_eq!(Config::load(dir.path()).unwrap().unwrap().token, "t0k");
        let local = Config {
            relay_url: "ws://127.0.0.1:8098/ws".into(),
            token: "t".into(),
            scale: 2,
            bow_hue: None,
            peek: false,
            wander: true,
            sounds: true,
        };
        assert_eq!(local.files_url(), "http://127.0.0.1:8098/files");
    }

    #[test]
    fn sounds_are_on_by_default_and_the_choice_is_kept() {
        let old = "relay_url = \"ws://x/ws\"
token = \"t\"
";
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), old).unwrap();
        let mut c = Config::load(dir.path()).unwrap().unwrap();
        assert!(c.sounds);
        c.sounds = false;
        c.save(dir.path()).unwrap();
        assert!(!Config::load(dir.path()).unwrap().unwrap().sounds);
    }

    #[test]
    fn wander_is_on_by_default_and_the_choice_is_kept() {
        let old = "relay_url = \"ws://x/ws\"
token = \"t\"
";
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), old).unwrap();
        let mut c = Config::load(dir.path()).unwrap().unwrap();
        assert!(c.wander);
        c.wander = false;
        c.save(dir.path()).unwrap();
        assert!(!Config::load(dir.path()).unwrap().unwrap().wander);
    }

    #[test]
    fn peek_is_off_by_default_and_the_local_choice_is_kept() {
        let old = "relay_url = \"ws://x/ws\"
token = \"t\"
";
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), old).unwrap();
        let mut c = Config::load(dir.path()).unwrap().unwrap();
        assert!(!c.peek);
        c.peek = true;
        c.save(dir.path()).unwrap();
        assert!(Config::load(dir.path()).unwrap().unwrap().peek);
    }
}
