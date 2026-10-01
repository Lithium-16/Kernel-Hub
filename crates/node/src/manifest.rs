//! `module.toml` parsing and validation. Mirrors `kernel_sdk.manifest` in the Python SDK.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use kernel_protocol::{ActionSpec, AiTier, ParamSpec, ParamType};
use regex::Regex;
use serde::Deserialize;

static MODULE_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9-]{1,31}$").unwrap());
static ACTION_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$").unwrap());
static PARAM_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9_]*$").unwrap());

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0}: {1}")]
    Parse(PathBuf, toml::de::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Python,
    Node,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    id: String,
    name: String,
    icon: String,
    version: String,
    runtime: Runtime,
    entry: String,
    #[serde(default)]
    actions: Vec<RawAction>,
    #[serde(default)]
    settings: toml::Table,
}

#[derive(Debug, Deserialize)]
struct RawAction {
    id: String,
    label: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "default_ai")]
    ai: AiTier,
    #[serde(default)]
    params: BTreeMap<String, ParamSpec>,
    #[serde(default)]
    timeout_s: Option<f64>,
    #[serde(default)]
    quiet: bool,
}

fn default_ai() -> AiTier {
    AiTier::Confirm
}

#[derive(Debug, Clone)]
pub struct Action {
    pub spec: ActionSpec,
    pub timeout: Duration,
}

impl Action {
    /// MCP tool name for this action: `server.start` -> `server__start`.
    pub fn tool_name(&self) -> String {
        self.spec.id.replace('.', "__")
    }
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub version: String,
    pub runtime: Runtime,
    pub entry: String,
    pub actions: Vec<Action>,
    /// `[settings]` defaults (overridden per machine in the data folder).
    pub settings: serde_json::Map<String, serde_json::Value>,
    pub dir: PathBuf,
}

impl Manifest {
    pub fn action(&self, id: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.spec.id == id)
    }

    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        let text = std::fs::read_to_string(path).map_err(|e| ManifestError::Io(path.into(), e))?;
        let raw: RawManifest =
            toml::from_str(&text).map_err(|e| ManifestError::Parse(path.into(), e))?;
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        validate(raw, dir)
    }
}

fn invalid(msg: String) -> ManifestError {
    ManifestError::Invalid(msg)
}

fn validate(raw: RawManifest, dir: PathBuf) -> Result<Manifest, ManifestError> {
    if !MODULE_ID.is_match(&raw.id) {
        return Err(invalid(format!("module: invalid id '{}'", raw.id)));
    }
    let mut seen = HashSet::new();
    let mut actions = Vec::with_capacity(raw.actions.len());
    for (i, a) in raw.actions.into_iter().enumerate() {
        let at = format!("{}: actions[{i}]", raw.id);
        if !ACTION_ID.is_match(&a.id) {
            return Err(invalid(format!(
                "{at}: invalid action id '{}' (expected noun.verb)",
                a.id
            )));
        }
        if !seen.insert(a.id.clone()) {
            return Err(invalid(format!("{at}: duplicate action id '{}'", a.id)));
        }
        let mut params = serde_json::Map::new();
        for (name, p) in a.params {
            if !PARAM_NAME.is_match(&name) {
                return Err(invalid(format!("{at}: invalid parameter name '{name}'")));
            }
            if p.kind == ParamType::Enum && p.options.as_ref().is_none_or(Vec::is_empty) {
                return Err(invalid(format!(
                    "{at}.{name}: enum parameters need 'options'"
                )));
            }
            params.insert(
                name,
                serde_json::to_value(p).expect("param spec serializes"),
            );
        }
        if a.quiet && a.ai != AiTier::Safe {
            return Err(invalid(format!(
                "{at}: only safe actions can be quiet (everything else is always logged)"
            )));
        }
        let timeout_s = a.timeout_s.unwrap_or(60.0);
        if !(timeout_s > 0.0 && timeout_s <= 3600.0) {
            return Err(invalid(format!(
                "{at}: timeout_s must be between 0 and 3600"
            )));
        }
        actions.push(Action {
            spec: ActionSpec {
                id: a.id,
                label: a.label,
                icon: a.icon,
                description: a.description,
                ai: a.ai,
                params,
                quiet: a.quiet,
            },
            timeout: Duration::from_secs_f64(timeout_s),
        });
    }
    Ok(Manifest {
        id: raw.id,
        name: raw.name,
        icon: raw.icon,
        version: raw.version,
        runtime: raw.runtime,
        entry: raw.entry,
        actions,
        settings: match serde_json::to_value(raw.settings) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        },
        dir,
    })
}

/// Every `*/module.toml` directly under `dir`, parsed. Invalid manifests are returned as errors.
pub fn discover(dir: &Path) -> Vec<Result<Manifest, ManifestError>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path().join("module.toml"))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    paths.iter().map(|p| Manifest::load(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_modules() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules")
    }

    fn parse(text: &str) -> Result<Manifest, ManifestError> {
        let raw: RawManifest =
            toml::from_str(text).map_err(|e| ManifestError::Parse("t".into(), e))?;
        validate(raw, PathBuf::from("."))
    }

    const HEAD: &str =
        "id='demo'\nname='Demo'\nicon='box'\nversion='1.0.0'\nruntime='python'\nentry='main.py'\n";

    #[test]
    fn loads_hello_module() {
        let m = Manifest::load(&repo_modules().join("hello/module.toml")).unwrap();
        assert_eq!(m.id, "hello");
        assert_eq!(m.runtime, Runtime::Python);
        let ids: Vec<_> = m.actions.iter().map(|a| a.spec.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "greet.say",
                "counter.reset",
                "slow.wait",
                "greet.count",
                "debug.crash",
                "event.emit"
            ]
        );
        assert_eq!(m.action("greet.say").unwrap().tool_name(), "greet__say");
        assert_eq!(
            m.action("slow.wait").unwrap().timeout,
            Duration::from_secs(1)
        );
        assert_eq!(m.action("debug.crash").unwrap().spec.ai, AiTier::Never);
        assert!(m.action("greet.count").unwrap().spec.quiet);
        assert!(!m.action("greet.say").unwrap().spec.quiet);
        assert_eq!(
            m.action("greet.say").unwrap().spec.params["name"]["default"],
            "world"
        );
    }

    #[test]
    fn every_repo_module_loads() {
        let found = discover(&repo_modules());
        let mut ids = Vec::new();
        for m in found {
            match m {
                Ok(m) => ids.push(m.id),
                Err(e) => panic!("{e}"),
            }
        }
        ids.sort();
        assert_eq!(
            ids,
            [
                "comfyui",
                "hello",
                "minecraft",
                "pc-monitor",
                "roblox",
                "vram"
            ]
        );
    }

    #[test]
    fn defaults_ai_to_confirm() {
        let m = parse(&format!("{HEAD}[[actions]]\nid='a.b'\nlabel='x'\n")).unwrap();
        assert_eq!(m.actions[0].spec.ai, AiTier::Confirm);
    }

    #[test]
    fn rejects_invalid_manifests() {
        let cases = [
            (
                "id='Bad'\nname='D'\nicon='b'\nversion='1'\nruntime='python'\nentry='m.py'\n",
                "invalid id",
            ),
            (
                &format!("{HEAD}[[actions]]\nid='nodots'\nlabel='x'\n"),
                "invalid action id",
            ),
            (
                &format!(
                    "{HEAD}[[actions]]\nid='a.b'\nlabel='x'\n[[actions]]\nid='a.b'\nlabel='y'\n"
                ),
                "duplicate",
            ),
            (
                &format!("{HEAD}[[actions]]\nid='a.b'\nlabel='x'\nparams.p={{type='enum'}}\n"),
                "need 'options'",
            ),
            (
                &format!("{HEAD}[[actions]]\nid='a.b'\nlabel='x'\ntimeout_s=0\n"),
                "timeout_s",
            ),
            (
                &format!("{HEAD}[[actions]]\nid='a.b'\nlabel='x'\nai='confirm'\nquiet=true\n"),
                "only safe actions can be quiet",
            ),
        ];
        for (text, needle) in cases {
            let err = parse(text).unwrap_err().to_string();
            assert!(err.contains(needle), "expected '{needle}' in '{err}'");
        }
        assert!(parse(&HEAD.replace("python", "ruby")).is_err());
    }
}
