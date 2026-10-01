//! The assistant: a local model on Ollama answers first, Claude (Messages API) is the fallback.
//!
//! The model gets the modules' actions as tools and runs them through `Node::invoke` as the
//! assistant, so the normal permission tiers apply: `safe` actions run, `confirm` actions come
//! back to the person as an Approve button, `never` actions aren't offered at all. Every run is
//! in the activity log like any other.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kernel_protocol::{
    ActionInvoke, Actor, ActorKind, AiTier, ChatApproval, ChatReply, ChatSend, ChatStep, ErrorCode,
    ModuleState, new_id,
};
use serde_json::{Map, Value, json};

use crate::config::AssistantConfig;
use crate::node::Node;

const STATUS_TOOL: &str = "kernel_status";
const KEEP_TURNS: usize = 40;
const KEEP_CONVERSATIONS: usize = 50;
const IDLE_FORGET: Duration = Duration::from_secs(2 * 3600);

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub args: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallResult {
    pub id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

/// One entry of a conversation, independent of which model wrote it.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    User(String),
    Assistant { text: String, calls: Vec<Call> },
    Results(Vec<CallResult>),
}

/// A module action offered to the model.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub module: String,
    pub action: String,
    pub label: String,
    pub description: String,
    pub schema: Value,
}

#[derive(Debug, Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

#[derive(Debug)]
pub struct ModelReply {
    pub text: String,
    pub calls: Vec<Call>,
    pub usage: Usage,
}

#[derive(Debug)]
pub enum ModelError {
    /// Nothing answered at that address (try the next one, or fall back).
    Unreachable(String),
    Failed(String),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(e) | Self::Failed(e) => f.write_str(e),
        }
    }
}

struct Conversation {
    turns: Vec<Turn>,
    used: Instant,
}

pub struct Assistant {
    pub cfg: AssistantConfig,
    http: reqwest::Client,
    conversations: Mutex<HashMap<String, Conversation>>,
}

// -- tools -------------------------------------------------------------------------------

/// `module__action`, in the characters both APIs accept.
pub fn tool_name(module: &str, action: &str) -> String {
    let raw = format!("{module}__{action}");
    let clean: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    clean.chars().take(64).collect()
}

/// A module action's parameters as JSON Schema.
pub fn param_schema(params: &Map<String, Value>) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (name, spec) in params {
        let mut p = Map::new();
        let kind = spec["type"].as_str().unwrap_or("string");
        p.insert(
            "type".into(),
            json!(match kind {
                "int" => "integer",
                "float" => "number",
                "bool" => "boolean",
                _ => "string",
            }),
        );
        if kind == "enum"
            && let Some(options) = spec.get("options")
        {
            p.insert("enum".into(), options.clone());
        }
        for key in ["description", "minimum", "maximum", "default"] {
            let src = match key {
                "minimum" => "min",
                "maximum" => "max",
                other => other,
            };
            if let Some(v) = spec.get(src).filter(|v| !v.is_null()) {
                p.insert(key.into(), v.clone());
            }
        }
        if spec.get("default").is_none() {
            required.push(json!(name));
        }
        properties.insert(name.clone(), Value::Object(p));
    }
    json!({"type": "object", "properties": properties, "required": required})
}

pub fn tools(node: &Node) -> Vec<Tool> {
    let mut out = vec![Tool {
        name: STATUS_TOOL.into(),
        module: String::new(),
        action: String::new(),
        label: "Read status".into(),
        description: "The current status of a module (what its page shows): states, numbers, \
                      lists. Use this to answer questions about what's going on right now."
            .into(),
        schema: json!({
            "type": "object",
            "properties": {"module": {"type": "string", "description": "Module id, e.g. minecraft"}},
            "required": ["module"],
        }),
    }];
    for m in node.catalog() {
        if m.state != ModuleState::Running {
            continue;
        }
        for a in &m.actions {
            if a.ai == AiTier::Never || a.quiet && a.id.starts_with("settings.") {
                continue;
            }
            let approval = if a.ai == AiTier::Confirm {
                " Needs the user's approval: calling it shows them an Approve button."
            } else {
                ""
            };
            out.push(Tool {
                name: tool_name(&m.id, &a.id),
                module: m.id.clone(),
                action: a.id.clone(),
                label: a.label.clone(),
                description: format!(
                    "[{}] {}. {}{approval}",
                    m.name,
                    a.label,
                    a.description.clone().unwrap_or_default()
                ),
                schema: param_schema(&a.params),
            });
        }
    }
    out
}

// -- prompts -----------------------------------------------------------------------------

const PERSONA: &str = "Your personality: a tsundere catgirl. A little prickly and teasing \
(\"hmph\", \"b-baka\", the occasional \"nya\"), but genuinely helpful and always accurate. \
Never let the persona get in the way of errors, safety or anything serious: say those plainly.";

pub fn system_prompt(node: &Node, persona: bool) -> String {
    let mut modules = Vec::new();
    for m in node.catalog() {
        let state = m
            .status
            .as_ref()
            .and_then(|s| s.get("state"))
            .and_then(Value::as_str)
            .map(|s| format!(", {s}"))
            .unwrap_or_default();
        modules.push(format!(
            "- {} ({}): module {:?}{state}",
            m.name, m.id, m.state
        ));
    }
    format!(
        "You are Kernel, the assistant inside the user's home-PC manager. You act on their PCs \
only through the tools; each tool is a button in one of their modules.\n\
Rules:\n\
- When they ask you to do something, or about what's going on right now, use the tools. Read \
status with {STATUS_TOOL}. Never invent results or numbers.\n\
- Some tools need their approval; calling one shows them an Approve button instead of running \
it. Tell them what you asked for.\n\
- If no tool can do it, say so.\n\
- Keep replies short (a sentence or two) unless they ask for more. Plain text, no markdown \
tables.\n\
{persona_line}\n\nModules on {node} right now:\n{modules}\nTime on the node: {now}",
        persona_line = if persona {
            PERSONA
        } else {
            "Be friendly and direct."
        },
        node = node.cfg.node_name,
        modules = modules.join("\n"),
        now = kernel_protocol::now_ts(),
    )
}

// -- the models --------------------------------------------------------------------------

/// Removes a `<think>…</think>` block that some local models put before the answer.
pub fn strip_thinking(text: &str) -> String {
    let mut out = text.to_string();
    while let (Some(a), Some(b)) = (out.find("<think>"), out.find("</think>")) {
        if b < a {
            break;
        }
        out.replace_range(a..b + "</think>".len(), "");
    }
    out.trim().to_string()
}

pub fn ollama_messages(system: &str, turns: &[Turn]) -> Vec<Value> {
    let mut out = vec![json!({"role": "system", "content": system})];
    for t in turns {
        match t {
            Turn::User(text) => out.push(json!({"role": "user", "content": text})),
            Turn::Assistant { text, calls } => {
                let mut m = json!({"role": "assistant", "content": text});
                if !calls.is_empty() {
                    m["tool_calls"] = calls
                        .iter()
                        .map(|c| json!({"function": {"name": c.name, "arguments": c.args}}))
                        .collect();
                }
                out.push(m);
            }
            Turn::Results(results) => {
                for r in results {
                    out.push(json!({"role": "tool", "content": r.content, "tool_name": r.name}));
                }
            }
        }
    }
    out
}

pub fn parse_ollama(body: &Value) -> Result<ModelReply, ModelError> {
    if let Some(e) = body.get("error").and_then(Value::as_str) {
        return Err(ModelError::Failed(format!("Ollama: {e}")));
    }
    let msg = &body["message"];
    let mut calls = Vec::new();
    for c in msg["tool_calls"].as_array().into_iter().flatten() {
        let f = &c["function"];
        let Some(name) = f["name"].as_str() else {
            continue;
        };
        // Usually an object; some models send a JSON string.
        let args = match &f["arguments"] {
            Value::Object(m) => m.clone(),
            Value::String(s) => serde_json::from_str(s).unwrap_or_default(),
            _ => Map::new(),
        };
        calls.push(Call {
            id: format!("call_{}", new_id()),
            name: name.into(),
            args,
        });
    }
    Ok(ModelReply {
        text: strip_thinking(msg["content"].as_str().unwrap_or_default()),
        calls,
        usage: Usage {
            input: body["prompt_eval_count"].as_u64().unwrap_or(0),
            output: body["eval_count"].as_u64().unwrap_or(0),
            ..Usage::default()
        },
    })
}

pub fn claude_messages(turns: &[Turn]) -> Vec<Value> {
    let mut out = Vec::new();
    for t in turns {
        match t {
            Turn::User(text) => out.push(json!({"role": "user", "content": text})),
            Turn::Assistant { text, calls } => {
                let mut content = Vec::new();
                if !text.is_empty() {
                    content.push(json!({"type": "text", "text": text}));
                }
                for c in calls {
                    content.push(
                        json!({"type": "tool_use", "id": c.id, "name": c.name, "input": c.args}),
                    );
                }
                if !content.is_empty() {
                    out.push(json!({"role": "assistant", "content": content}));
                }
            }
            Turn::Results(results) => out.push(json!({
                "role": "user",
                "content": results.iter().map(|r| json!({
                    "type": "tool_result",
                    "tool_use_id": r.id,
                    "content": r.content,
                    "is_error": r.is_error,
                })).collect::<Vec<_>>(),
            })),
        }
    }
    out
}

pub fn parse_claude(body: &Value) -> Result<ModelReply, ModelError> {
    if body["type"] == "error" {
        let msg = body["error"]["message"].as_str().unwrap_or("unknown error");
        return Err(ModelError::Failed(format!("Claude API: {msg}")));
    }
    let mut text = Vec::new();
    let mut calls = Vec::new();
    for block in body["content"].as_array().into_iter().flatten() {
        match block["type"].as_str() {
            Some("text") => text.push(block["text"].as_str().unwrap_or_default().to_string()),
            Some("tool_use") => calls.push(Call {
                id: block["id"].as_str().unwrap_or_default().into(),
                name: block["name"].as_str().unwrap_or_default().into(),
                args: block["input"].as_object().cloned().unwrap_or_default(),
            }),
            _ => {}
        }
    }
    let mut text = text.join("\n").trim().to_string();
    match body["stop_reason"].as_str() {
        Some("refusal") => {
            calls.clear();
            if text.is_empty() {
                text = "I can't help with that one.".into();
            }
        }
        Some("max_tokens") if calls.is_empty() => text.push_str(" (cut off)"),
        _ => {}
    }
    let u = &body["usage"];
    Ok(ModelReply {
        text,
        calls,
        usage: Usage {
            input: u["input_tokens"].as_u64().unwrap_or(0),
            output: u["output_tokens"].as_u64().unwrap_or(0),
            cache_write: u["cache_creation_input_tokens"].as_u64().unwrap_or(0),
            cache_read: u["cache_read_input_tokens"].as_u64().unwrap_or(0),
        },
    })
}

/// USD per million tokens (input, output). Unknown models are priced high, to be safe.
fn price(model: &str) -> (f64, f64) {
    match model {
        m if m.starts_with("claude-haiku-4-5") => (1.0, 5.0),
        m if m.starts_with("claude-sonnet-5") => (2.0, 10.0),
        m if m.starts_with("claude-opus-5-5") => (4.0, 20.0),
        _ => (5.0, 25.0),
    }
}

pub fn cost(model: &str, u: &Usage) -> f64 {
    let (input, output) = price(model);
    (u.input as f64 * input
        + u.cache_write as f64 * input * 1.25
        + u.cache_read as f64 * input * 0.1
        + u.output as f64 * output)
        / 1e6
}

fn month() -> String {
    kernel_protocol::now_ts()[..7].to_string()
}

/// Where a request goes: the local model (Ollama URLs in order), or Claude.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Brain {
    Local,
    Claude,
}

impl Assistant {
    pub fn new(cfg: AssistantConfig) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("kerneld/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5))
            // A local model may need to load first; a long tool-using answer takes a while.
            .timeout(Duration::from_secs(300))
            .build()
            .expect("HTTP client");
        Self {
            cfg,
            http,
            conversations: Mutex::default(),
        }
    }

    fn claude_available(&self, node: &Node) -> Result<(), String> {
        if self.cfg.anthropic_api_key.is_empty() {
            return Err("no Claude API key is set ([assistant] anthropic_api_key)".into());
        }
        let spent = node.activity.usage(&month());
        if spent >= self.cfg.monthly_budget_usd {
            return Err(format!(
                "this month's Claude budget (${:.2}) is used up",
                self.cfg.monthly_budget_usd
            ));
        }
        Ok(())
    }

    async fn ask_ollama(
        &self,
        system: &str,
        turns: &[Turn],
        tools: &[Tool],
    ) -> Result<ModelReply, ModelError> {
        let body = json!({
            "model": self.cfg.model,
            "stream": false,
            "messages": ollama_messages(system, turns),
            "tools": tools.iter().map(|t| json!({
                "type": "function",
                "function": {"name": t.name, "description": t.description, "parameters": t.schema},
            })).collect::<Vec<_>>(),
            "options": {"num_ctx": 16384},
        });
        let mut last = ModelError::Unreachable("no Ollama address is set".into());
        for url in &self.cfg.ollama_urls {
            let resp = match self
                .http
                .post(format!("{}/api/chat", url.trim_end_matches('/')))
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) if e.is_connect() => {
                    last = ModelError::Unreachable(format!("Ollama isn't running at {url}"));
                    continue;
                }
                Err(e) => return Err(ModelError::Failed(format!("Ollama: {e}"))),
            };
            if resp.status().as_u16() == 502 {
                // The VRAM proxy is up, but Ollama behind it isn't.
                last = ModelError::Unreachable(format!("Ollama isn't running behind {url}"));
                continue;
            }
            let status = resp.status();
            let value: Value = resp
                .json()
                .await
                .map_err(|e| ModelError::Failed(format!("Ollama: {e}")))?;
            if !status.is_success() {
                let msg = value["error"]
                    .as_str()
                    .unwrap_or("request failed")
                    .to_string();
                return Err(ModelError::Failed(format!("Ollama ({status}): {msg}")));
            }
            return parse_ollama(&value);
        }
        Err(last)
    }

    async fn ask_claude(
        &self,
        system: &str,
        turns: &[Turn],
        tools: &[Tool],
    ) -> Result<ModelReply, ModelError> {
        let body = json!({
            "model": self.cfg.claude_model,
            "max_tokens": 4096,
            "system": system,
            "tools": tools.iter().map(|t| json!({
                "name": t.name, "description": t.description, "input_schema": t.schema,
            })).collect::<Vec<_>>(),
            "messages": claude_messages(turns),
        });
        let resp = self
            .http
            .post(format!(
                "{}/v1/messages",
                self.cfg.anthropic_api.trim_end_matches('/')
            ))
            .header("x-api-key", &self.cfg.anthropic_api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| ModelError::Unreachable(format!("couldn't reach the Claude API ({e})")))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| ModelError::Failed(format!("Claude API: {e}")))?;
        if !status.is_success() && value["type"] != "error" {
            return Err(ModelError::Failed(format!("Claude API answered {status}")));
        }
        parse_claude(&value)
    }

    /// Run one tool call. Returns what the model sees, plus what the person sees.
    async fn run_tool(
        node: &Arc<Node>,
        tools: &[Tool],
        call: &Call,
        conversation: &str,
    ) -> (CallResult, Option<ChatStep>, Option<ChatApproval>) {
        let result = |content: String, is_error: bool| CallResult {
            id: call.id.clone(),
            name: call.name.clone(),
            content,
            is_error,
        };
        if call.name == STATUS_TOOL {
            let wanted = call
                .args
                .get("module")
                .and_then(Value::as_str)
                .unwrap_or("");
            let status = node
                .catalog()
                .into_iter()
                .find(|m| m.id == wanted)
                .map(|m| m.status);
            return match status {
                Some(s) => (
                    result(truncate(&json!(s).to_string(), 6000), false),
                    None,
                    None,
                ),
                None => (result(format!("no module '{wanted}'"), true), None, None),
            };
        }
        let Some(tool) = tools.iter().find(|t| t.name == call.name) else {
            return (
                result(format!("there is no tool called '{}'", call.name), true),
                None,
                None,
            );
        };
        let req = ActionInvoke {
            module: tool.module.clone(),
            action: tool.action.clone(),
            params: call.args.clone(),
            actor: Actor {
                kind: ActorKind::Assistant,
                reference: Some(format!("chat {conversation}")),
            },
            approval_id: None,
        };
        let res = node.invoke(req).await;
        let step = |ok: bool, summary: String| ChatStep {
            module: tool.module.clone(),
            action: tool.action.clone(),
            params: call.args.clone(),
            ok,
            summary: truncate(&summary, 300),
        };
        match (res.result, res.error) {
            (_, Some(e)) if e.code == ErrorCode::NeedsApproval => (
                result(
                    "Not run yet: this needs the user's approval. They now see an Approve button \
                     for it. Tell them what it will do."
                        .into(),
                    false,
                ),
                Some(step(false, "waiting for your approval".into())),
                Some(ChatApproval {
                    module: tool.module.clone(),
                    action: tool.action.clone(),
                    params: call.args.clone(),
                    label: tool.label.clone(),
                }),
            ),
            (_, Some(e)) => {
                let msg = format!("{}: {}", e.code.as_str(), e.message);
                (result(msg.clone(), true), Some(step(false, msg)), None)
            }
            (value, None) => {
                let text = value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "done".into());
                (
                    result(truncate(&text, 6000), false),
                    Some(step(true, text)),
                    None,
                )
            }
        }
    }

    /// Answer one message, running tools as needed.
    pub async fn chat(&self, node: &Arc<Node>, req: ChatSend) -> Result<ChatReply, String> {
        let conversation = req.conversation.clone().unwrap_or_else(new_id);
        let mut turns = self.history(&conversation);
        turns.push(Turn::User(req.text.clone()));

        let mut brain = match req.provider.as_deref() {
            Some("claude") => {
                self.claude_available(node)?;
                Brain::Claude
            }
            _ => Brain::Local,
        };
        let may_fall_back = req.provider.as_deref() != Some("local");

        let tools = tools(node);
        let system = system_prompt(node, self.cfg.persona);
        let (mut steps, mut approvals) = (Vec::new(), Vec::new());
        let mut usage = Usage::default();
        let mut final_text = String::new();

        for step in 0..=self.cfg.max_steps {
            let reply = match brain {
                Brain::Local => {
                    match self.ask_ollama(&system, &turns, &tools).await {
                        Ok(r) => r,
                        Err(local) if may_fall_back => {
                            self.claude_available(node)
                            .map_err(|why| format!("The local model failed ({local}), and Claude can't step in: {why}."))?;
                            tracing::info!(error = %local, "local model failed; asking Claude");
                            brain = Brain::Claude;
                            self.ask_claude(&system, &turns, &tools)
                                .await
                                .map_err(|e| e.to_string())?
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                }
                Brain::Claude => self
                    .ask_claude(&system, &turns, &tools)
                    .await
                    .map_err(|e| e.to_string())?,
            };
            if brain == Brain::Claude {
                usage.input += reply.usage.input;
                usage.output += reply.usage.output;
                usage.cache_write += reply.usage.cache_write;
                usage.cache_read += reply.usage.cache_read;
            }
            let calls = if step == self.cfg.max_steps {
                Vec::new()
            } else {
                reply.calls
            };
            turns.push(Turn::Assistant {
                text: reply.text.clone(),
                calls: calls.clone(),
            });
            if calls.is_empty() {
                final_text = reply.text;
                if step == self.cfg.max_steps {
                    final_text
                        .push_str(&format!(" (stopped after {} actions)", self.cfg.max_steps));
                }
                break;
            }
            let mut results = Vec::new();
            for call in &calls {
                let (r, s, a) = Self::run_tool(node, &tools, call, &conversation).await;
                results.push(r);
                steps.extend(s);
                approvals.extend(a);
            }
            turns.push(Turn::Results(results));
        }

        let model = match brain {
            Brain::Local => self.cfg.model.clone(),
            Brain::Claude => self.cfg.claude_model.clone(),
        };
        let cost_usd = if brain == Brain::Claude {
            cost(&model, &usage)
        } else {
            0.0
        };
        if cost_usd > 0.0
            && let Err(e) = node
                .activity
                .add_usage(&month(), cost_usd, usage.input, usage.output)
        {
            tracing::warn!(error = %e, "couldn't record Claude spend");
        }
        self.remember(&conversation, turns);
        Ok(ChatReply {
            conversation,
            text: if final_text.is_empty() {
                "Done.".into()
            } else {
                final_text
            },
            steps,
            approvals,
            provider: if brain == Brain::Claude {
                "claude"
            } else {
                "local"
            }
            .into(),
            model,
            cost_usd,
        })
    }

    fn history(&self, id: &str) -> Vec<Turn> {
        let convs = self.conversations.lock().expect("conversations");
        convs.get(id).map(|c| c.turns.clone()).unwrap_or_default()
    }

    fn remember(&self, id: &str, turns: Vec<Turn>) {
        let mut convs = self.conversations.lock().expect("conversations");
        convs.retain(|_, c| c.used.elapsed() < IDLE_FORGET);
        if convs.len() >= KEEP_CONVERSATIONS
            && !convs.contains_key(id)
            && let Some(oldest) = convs
                .iter()
                .min_by_key(|(_, c)| c.used)
                .map(|(k, _)| k.clone())
        {
            convs.remove(&oldest);
        }
        convs.insert(
            id.into(),
            Conversation {
                turns: trim(turns),
                used: Instant::now(),
            },
        );
    }

    /// This month's Claude spend, for the Node page.
    pub fn spend(&self, node: &Node) -> f64 {
        node.activity.usage(&month())
    }
}

/// Keep the last turns, starting at a user message (tool results must follow their calls).
pub fn trim(mut turns: Vec<Turn>) -> Vec<Turn> {
    if turns.len() <= KEEP_TURNS {
        return turns;
    }
    let mut start = turns.len() - KEEP_TURNS;
    while start < turns.len() && !matches!(turns[start], Turn::User(_)) {
        start += 1;
    }
    turns.drain(..start);
    turns
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_and_schemas() {
        assert_eq!(
            tool_name("pc-monitor", "power.sleep"),
            "pc-monitor__power_sleep"
        );
        let params = json!({
            "name": {"type": "string", "description": "Who"},
            "times": {"type": "int", "default": 1, "min": 1, "max": 3},
            "tone": {"type": "enum", "options": ["warm", "dry"], "default": "warm"},
        });
        let s = param_schema(params.as_object().unwrap());
        assert_eq!(
            s["properties"]["times"],
            json!({"type": "integer", "minimum": 1, "maximum": 3, "default": 1})
        );
        assert_eq!(s["properties"]["tone"]["enum"], json!(["warm", "dry"]));
        assert_eq!(s["required"], json!(["name"]));
    }

    #[test]
    fn strips_thinking() {
        assert_eq!(
            strip_thinking("<think>hmm\nok</think>\n\nIt's up."),
            "It's up."
        );
        assert_eq!(strip_thinking("plain"), "plain");
    }

    #[test]
    fn reads_both_apis() {
        let ollama = json!({"message": {"content": "", "tool_calls": [
            {"function": {"name": "minecraft__server_start", "arguments": {}}},
            {"function": {"name": "x__y", "arguments": "{\"a\": 1}"}},
        ]}, "prompt_eval_count": 900, "eval_count": 12});
        let r = parse_ollama(&ollama).unwrap();
        assert_eq!(r.calls.len(), 2);
        assert_eq!(r.calls[1].args["a"], 1);

        let claude = json!({"content": [
            {"type": "text", "text": "Starting it."},
            {"type": "tool_use", "id": "toolu_1", "name": "minecraft__server_start", "input": {}},
        ], "stop_reason": "tool_use", "usage": {"input_tokens": 1200, "output_tokens": 40}});
        let r = parse_claude(&claude).unwrap();
        assert_eq!(
            (r.text.as_str(), r.calls[0].id.as_str()),
            ("Starting it.", "toolu_1")
        );
        assert!((cost("claude-haiku-4-5", &r.usage) - (1200.0 + 40.0 * 5.0) / 1e6).abs() < 1e-12);

        let refused = json!({"content": [], "stop_reason": "refusal", "usage": {}});
        assert_eq!(
            parse_claude(&refused).unwrap().text,
            "I can't help with that one."
        );
        let err = json!({"type": "error", "error": {"type": "authentication_error", "message": "invalid x-api-key"}});
        assert!(parse_claude(&err).is_err());
    }

    #[test]
    fn history_converts_and_trims() {
        let call = Call {
            id: "call_1".into(),
            name: "a__b".into(),
            args: Map::new(),
        };
        let turns = vec![
            Turn::User("hi".into()),
            Turn::Assistant {
                text: String::new(),
                calls: vec![call],
            },
            Turn::Results(vec![CallResult {
                id: "call_1".into(),
                name: "a__b".into(),
                content: "ok".into(),
                is_error: false,
            }]),
            Turn::Assistant {
                text: "done".into(),
                calls: vec![],
            },
        ];
        let c = claude_messages(&turns);
        assert_eq!(c[1]["content"][0]["type"], "tool_use");
        assert_eq!(c[2]["content"][0]["tool_use_id"], "call_1");
        let o = ollama_messages("sys", &turns);
        assert_eq!(o[2]["tool_calls"][0]["function"]["name"], "a__b");
        assert_eq!(o[3]["role"], "tool");

        let mut long = Vec::new();
        for i in 0..30 {
            long.push(Turn::User(format!("{i}")));
            long.push(Turn::Assistant {
                text: "x".into(),
                calls: vec![],
            });
        }
        let kept = trim(long);
        assert!(kept.len() <= KEEP_TURNS && matches!(kept[0], Turn::User(_)));
    }
}
