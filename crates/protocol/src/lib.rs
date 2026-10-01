//! Wire types for the Kernel protocol.
//!
//! The source of truth is the zod schema in `packages/protocol`. These types are
//! kept in sync by the contract test in `tests/fixtures.rs`, which round-trips
//! every fixture in `packages/protocol/fixtures`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Offline,
    Disabled,
    NotPermitted,
    NeedsApproval,
    InvalidParams,
    ModuleFailed,
    Timeout,
    Busy,
    Unauthorized,
    BadRequest,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Disabled => "disabled",
            Self::NotPermitted => "not_permitted",
            Self::NeedsApproval => "needs_approval",
            Self::InvalidParams => "invalid_params",
            Self::ModuleFailed => "module_failed",
            Self::Timeout => "timeout",
            Self::Busy => "busy",
            Self::Unauthorized => "unauthorized",
            Self::BadRequest => "bad_request",
            Self::Internal => "internal",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(Value::String(s.to_owned())).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: ErrorCode,
    pub message: String,
}

impl ErrorInfo {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTier {
    Safe,
    Confirm,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    Int,
    Float,
    String,
    Bool,
    Enum,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    #[serde(rename = "type")]
    pub kind: ParamType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionSpec {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub ai: AiTier,
    pub params: Map<String, Value>,
    /// Read-only and frequent: not written to the activity log. Only allowed on safe actions.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub quiet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleState {
    Starting,
    Running,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub version: String,
    pub state: ModuleState,
    pub actions: Vec<ActionSpec>,
    pub status: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    Assistant,
    Automation,
    Phone,
}

impl ActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Automation => "automation",
            Self::Phone => "phone",
        }
    }

    /// A person pressed a button (desktop or phone), as opposed to software acting.
    pub fn is_human(self) -> bool {
        matches!(self, Self::User | Self::Phone)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "ref")]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityResult {
    Ok,
    Error,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub id: String,
    pub ts: String,
    pub actor: Actor,
    pub node_id: String,
    pub module: String,
    pub action: String,
    pub params: Map<String, Value>,
    pub result: ActivityResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub client: ClientInfo,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    pub node: NodeInfo,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Empty {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub modules: Vec<ModuleInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionInvoke {
    pub module: String,
    pub action: String,
    pub params: Map<String, Value>,
    pub actor: Actor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
}

impl ActionResult {
    pub fn success(value: Value) -> Self {
        Self {
            ok: true,
            result: Some(value),
            error: None,
        }
    }

    pub fn failure(error: ErrorInfo) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ActivityQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub entries: Vec<ActivityEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventLevel {
    Info,
    Warn,
    Error,
}

impl EventLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "info" => Some(Self::Info),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// Something that happened on a node without anyone asking: a server crashed, a player
/// joined, an update is out. Modules report them; the node keeps them and sends alerts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeEvent {
    pub id: String,
    pub ts: String,
    pub node_id: String,
    pub module: String,
    /// Short dotted name, like `server.crashed` or `player.joined`.
    pub kind: String,
    pub level: EventLevel,
    /// One line for people.
    pub message: String,
    #[serde(default)]
    pub data: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EventsQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Events {
    pub events: Vec<NodeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "body")]
pub enum Payload {
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "welcome")]
    Welcome(Welcome),
    #[serde(rename = "catalog.get")]
    CatalogGet(Empty),
    #[serde(rename = "catalog")]
    Catalog(Catalog),
    #[serde(rename = "action.invoke")]
    ActionInvoke(ActionInvoke),
    #[serde(rename = "action.result")]
    ActionResult(ActionResult),
    #[serde(rename = "activity.query")]
    ActivityQuery(ActivityQuery),
    #[serde(rename = "activity")]
    Activity(Activity),
    #[serde(rename = "events.query")]
    EventsQuery(EventsQuery),
    #[serde(rename = "events")]
    Events(Events),
    /// Pushed to every connected client as it happens (no `re`).
    #[serde(rename = "event")]
    Event(NodeEvent),
    #[serde(rename = "error")]
    Error(ErrorInfo),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub re: Option<String>,
    pub ts: String,
    #[serde(flatten)]
    pub payload: Payload,
}

#[derive(Debug)]
pub enum DecodeError {
    Json(serde_json::Error),
    Version(u8),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid message: {e}"),
            Self::Version(v) => write!(f, "unsupported protocol version {v}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Current UTC time as RFC 3339 with second precision (`2026-09-24T20:11:02Z`).
pub fn now_ts() -> String {
    let now = time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

pub fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

impl Envelope {
    pub fn new(payload: Payload) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id: new_id(),
            re: None,
            ts: now_ts(),
            payload,
        }
    }

    pub fn reply(to: &str, payload: Payload) -> Self {
        Self {
            re: Some(to.to_owned()),
            ..Self::new(payload)
        }
    }

    pub fn decode(text: &str) -> Result<Self, DecodeError> {
        let env: Self = serde_json::from_str(text).map_err(DecodeError::Json)?;
        if env.v != PROTOCOL_VERSION {
            return Err(DecodeError::Version(env.v));
        }
        Ok(env)
    }

    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("envelope serialization cannot fail")
    }
}
