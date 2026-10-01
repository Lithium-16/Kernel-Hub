//! Module settings from the app: the defaults in each module.toml, overridden per machine by
//! `data/settings/<module>.toml` (what the SDK reads). Saving restarts the module.
//!
//! Secrets (tokens, webhooks, passwords) are never sent to the app, and settings that name a
//! program to run (`*command`) can only be changed on the PC itself.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::manifest::Manifest;

pub const HIDDEN: &str = "(hidden)";

pub fn is_secret(key: &str) -> bool {
    ["token", "secret", "password", "webhook", "api_key"]
        .iter()
        .any(|w| key.contains(w))
}

pub fn is_locked(key: &str) -> bool {
    key == "command" || key.ends_with("_command")
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_i64() || n.is_u64() => "int",
        Value::Number(_) => "float",
        Value::String(_) => "string",
        Value::Array(_) => "list",
        _ => "other",
    }
}

fn same_kind(default: &Value, value: &Value) -> bool {
    match (kind(default), kind(value)) {
        ("float", "int") => true,
        (a, b) => a == b,
    }
}

/// The `# comment` lines right above each key in module.toml's `[settings]`.
pub fn notes(module_toml: &str) -> Map<String, Value> {
    let mut out = Map::new();
    let mut inside = false;
    let mut comment: Vec<String> = Vec::new();
    for line in module_toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            inside = t == "[settings]";
            comment.clear();
            continue;
        }
        if !inside {
            continue;
        }
        if let Some(c) = t.strip_prefix('#') {
            comment.push(c.trim().to_string());
        } else if let Some((key, _)) = t.split_once('=') {
            let key = key.trim();
            if !comment.is_empty() && !key.is_empty() && !key.contains(' ') {
                out.insert(key.into(), Value::String(comment.join(" ")));
            }
            comment.clear();
        } else if t.is_empty() {
            comment.clear();
        }
    }
    out
}

fn read_table(path: &Path) -> Result<Map<String, Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e.to_string()),
    };
    let table: toml::Table =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    match serde_json::to_value(table) {
        Ok(Value::Object(m)) => Ok(m),
        _ => Err("settings file is not a table".into()),
    }
}

/// Every setting with its default, current value and note, for the app.
pub fn describe(m: &Manifest, file: &Path) -> Result<Value, String> {
    let local = read_table(file)?;
    let text = std::fs::read_to_string(m.dir.join("module.toml")).unwrap_or_default();
    let notes = notes(&text);
    let fields: Vec<Value> = m
        .settings
        .iter()
        .map(|(key, default)| {
            let value = local.get(key).unwrap_or(default);
            let secret = is_secret(key);
            let shown = |v: &Value| {
                if secret && v.as_str().is_some_and(|s| !s.is_empty()) {
                    json!(HIDDEN)
                } else {
                    v.clone()
                }
            };
            json!({
                "key": key,
                "type": kind(default),
                "value": shown(value),
                "default": shown(default),
                "changed": local.contains_key(key),
                "note": notes.get(key),
                "secret": secret,
                "locked": is_locked(key),
            })
        })
        .collect();
    Ok(json!({ "module": m.id, "file": file.to_string_lossy(), "fields": fields }))
}

/// Apply `values` over this machine's settings file. Values equal to the default are dropped
/// from the file; `(hidden)` for a secret keeps what's there. Returns the keys changed.
pub fn save(m: &Manifest, file: &Path, values: &Map<String, Value>) -> Result<Vec<String>, String> {
    let mut local = read_table(file)?;
    let mut changed = Vec::new();
    for (key, value) in values {
        let Some(default) = m.settings.get(key) else {
            return Err(format!("{} has no setting '{key}'", m.id));
        };
        if is_locked(key) {
            return Err(format!(
                "'{key}' names a program to run, so it can only be changed on the PC itself"
            ));
        }
        if is_secret(key) && value.as_str() == Some(HIDDEN) {
            continue;
        }
        if !same_kind(default, value) {
            return Err(format!("'{key}' must be a {}", kind(default)));
        }
        // 2 for a float setting is 2.0, so it compares equal to a default of 2.0.
        let value = &match (default.is_f64(), value.as_f64()) {
            (true, Some(f)) => json!(f),
            _ => value.clone(),
        };
        let before = local.get(key).unwrap_or(default).clone();
        if value == default {
            local.remove(key);
        } else {
            local.insert(key.clone(), value.clone());
        }
        if &before != value {
            changed.push(key.clone());
        }
    }
    let table: toml::Table =
        serde_json::from_value(Value::Object(local)).map_err(|e| e.to_string())?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = format!(
        "# Settings for {} on this PC (changed from the Kernel app). Defaults: module.toml.\n{}",
        m.id,
        toml::to_string(&table).map_err(|e| e.to_string())?
    );
    std::fs::write(file, text).map_err(|e| e.to_string())?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comfy() -> Manifest {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../modules/comfyui/module.toml");
        Manifest::load(&path).unwrap()
    }

    #[test]
    fn describes_and_saves_settings() {
        let m = comfy();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings/comfyui.toml");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "civitai_token = \"abc\"\npoll_s = 5.0\n").unwrap();

        let d = describe(&m, &file).unwrap();
        let field = |k: &str| {
            d["fields"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["key"] == k)
                .unwrap()
                .clone()
        };
        assert_eq!(field("civitai_token")["value"], HIDDEN);
        assert_eq!(field("poll_s")["value"], 5.0);
        assert!(field("poll_s")["changed"].as_bool().unwrap());
        assert!(field("start_command")["locked"].as_bool().unwrap());
        assert!(field("url")["note"].as_str().is_none_or(|n| !n.is_empty()));
        assert!(
            field("comfy_dir")["note"]
                .as_str()
                .unwrap()
                .contains("ComfyUI folder")
        );

        let values = json!({"civitai_token": HIDDEN, "poll_s": 2, "comfy_dir": "D:/ComfyUI"});
        let changed = save(&m, &file, values.as_object().unwrap()).unwrap();
        assert_eq!(changed, ["comfy_dir", "poll_s"]);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("civitai_token = \"abc\""),
            "secret kept: {text}"
        );
        assert!(text.contains("comfy_dir = \"D:/ComfyUI\""));
        assert!(
            !text.contains("poll_s"),
            "back to the default, so dropped: {text}"
        );

        for bad in [
            json!({"nope": 1}),
            json!({"poll_s": "fast"}),
            json!({"start_command": ["calc.exe"]}),
        ] {
            assert!(save(&m, &file, bad.as_object().unwrap()).is_err(), "{bad}");
        }
    }
}
