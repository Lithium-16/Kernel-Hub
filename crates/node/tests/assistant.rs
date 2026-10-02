//! The assistant end to end: a real kerneld with the hello module, a fake Ollama and a fake
//! Claude API, driven over WebSocket like the app does.
//!
//! Needs `KERNEL_TEST_PYTHON` (see e2e.rs); skipped without it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use futures::{SinkExt, StreamExt};
use kernel_node::config::{AssistantConfig, Config, SupervisorConfig};
use kernel_protocol::{
    ActivityQuery, ActorKind, ChatReply, ChatSend, ClientInfo, Empty, Envelope, Hello, ModuleState,
    Payload,
};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

const TOKEN: &str = "assistant-test-token";

#[derive(Default)]
struct Fakes {
    ollama_down: AtomicBool,
    ollama_seen: Mutex<Vec<Value>>,
    claude_seen: Mutex<Vec<Value>>,
}

/// First answer: call two tools. After tool results: a final text.
async fn ollama(
    State(f): State<Arc<Fakes>>,
    axum::Json(body): axum::Json<Value>,
) -> axum::response::Response {
    if f.ollama_down.load(Ordering::SeqCst) {
        return (StatusCode::BAD_GATEWAY, "Ollama isn't running").into_response();
    }
    f.ollama_seen.lock().unwrap().push(body.clone());
    let last = body["messages"].as_array().unwrap().last().unwrap().clone();
    let reply = if last["role"] == "tool" {
        json!({"message": {"role": "assistant", "content": "<think>ok</think>Said hi. Resetting needs your OK."}, "done": true})
    } else {
        json!({"message": {"role": "assistant", "content": "", "tool_calls": [
            {"function": {"name": "hello__greet_say", "arguments": {"name": "Pluto"}}},
            {"function": {"name": "hello__counter_reset", "arguments": {}}},
        ]}, "done": true})
    };
    axum::Json(reply).into_response()
}

async fn claude(
    State(f): State<Arc<Fakes>>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> axum::response::Response {
    assert_eq!(headers["x-api-key"], "test-key");
    assert_eq!(headers["anthropic-version"], "2023-06-01");
    f.claude_seen.lock().unwrap().push(body.clone());
    let last = body["messages"].as_array().unwrap().last().unwrap().clone();
    let reply = if last["content"][0]["type"] == "tool_result" {
        json!({"type": "message", "content": [{"type": "text", "text": "Checked: you've said hello."}],
               "stop_reason": "end_turn", "usage": {"input_tokens": 2000, "output_tokens": 30}})
    } else {
        json!({"type": "message", "content": [
                  {"type": "text", "text": "Let me look."},
                  {"type": "tool_use", "id": "toolu_1", "name": "kernel_status", "input": {"module": "hello"}}],
               "stop_reason": "tool_use", "usage": {"input_tokens": 1800, "output_tokens": 25}})
    };
    axum::Json(reply).into_response()
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn request(ws: &mut Ws, payload: Payload) -> Payload {
    let env = Envelope::new(payload);
    ws.send(Message::Text(env.encode().into())).await.unwrap();
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(60), ws.next())
            .await
            .expect("reply")
            .unwrap()
            .unwrap();
        if let Message::Text(t) = msg {
            let reply = Envelope::decode(&t).unwrap();
            if reply.re.as_deref() == Some(env.id.as_str()) {
                return reply.payload;
            }
        }
    }
}

async fn chat(ws: &mut Ws, text: &str, conversation: Option<String>, provider: &str) -> ChatReply {
    let req = ChatSend {
        conversation,
        text: text.into(),
        provider: Some(provider.into()),
    };
    match request(ws, Payload::ChatSend(req)).await {
        Payload::ChatReply(r) => r,
        other => panic!("expected chat.reply, got {other:?}"),
    }
}

async fn start_node(
    modules_dir: PathBuf,
    enabled: &[&str],
    assistant: AssistantConfig,
    data: &std::path::Path,
) -> (kernel_node::Running, Ws) {
    let python = std::env::var_os("KERNEL_TEST_PYTHON").unwrap();
    let cfg = Config {
        node_id: "test-node".into(),
        node_name: "Test node".into(),
        listen: "127.0.0.1:0".parse().unwrap(),
        token: TOKEN.into(),
        modules_dir,
        data_dir: data.to_path_buf(),
        python: PathBuf::from(python).to_string_lossy().into_owned(),
        node: "node".into(),
        enabled_modules: enabled.iter().map(|m| m.to_string()).collect(),
        supervisor: SupervisorConfig {
            status_interval_ms: 200,
            ..SupervisorConfig::default()
        },
        allow_lan: false,
        update: Default::default(),
        notify: Default::default(),
        schedule: vec![],
        on_event: vec![],
        when: vec![],
        assistant,
        path: None,
    };
    let running = kernel_node::start(cfg).await.expect("node starts");
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", running.addr))
        .await
        .unwrap();
    let hello = Hello {
        client: ClientInfo {
            name: "test".into(),
            version: "0".into(),
        },
        token: TOKEN.into(),
    };
    assert!(matches!(
        request(&mut ws, Payload::Hello(hello)).await,
        Payload::Welcome(_)
    ));
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let Payload::Catalog(c) = request(&mut ws, Payload::CatalogGet(Empty {})).await else {
            panic!()
        };
        if enabled.iter().all(|id| {
            c.modules
                .iter()
                .any(|m| m.id == *id && m.state == ModuleState::Running)
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{enabled:?} never started: {:?}",
            c.modules
                .iter()
                .map(|m| (&m.id, m.state, &m.status))
                .collect::<Vec<_>>()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    (running, ws)
}

fn repo_modules() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules")
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        if name == "__pycache__" || name == "tests" {
            continue;
        }
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(&name));
        } else {
            std::fs::copy(e.path(), to.join(&name)).unwrap();
        }
    }
}

fn offered(seen: &Value) -> Vec<String> {
    seen["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn assistant_runs_tools_asks_for_approval_and_falls_back() {
    if std::env::var_os("KERNEL_TEST_PYTHON").is_none() {
        eprintln!("skipping: set KERNEL_TEST_PYTHON to a Python with kernel_sdk installed");
        return;
    }
    let fakes = Arc::new(Fakes::default());
    let ollama_url = serve(
        Router::new()
            .route("/api/chat", post(ollama))
            .with_state(fakes.clone()),
    )
    .await;
    let claude_url = serve(
        Router::new()
            .route("/v1/messages", post(claude))
            .with_state(fakes.clone()),
    )
    .await;

    let data = tempfile::tempdir().unwrap();
    let assistant = AssistantConfig {
        // The first address is down (like the VRAM proxy when the module is off).
        ollama_urls: vec!["http://127.0.0.1:1".into(), ollama_url],
        anthropic_api_key: "test-key".into(),
        anthropic_api: claude_url,
        ..AssistantConfig::default()
    };
    let (running, mut ws) = start_node(repo_modules(), &["hello"], assistant, data.path()).await;

    // Local model: a safe action runs, a confirm action becomes an Approve button.
    let r = chat(
        &mut ws,
        "Say hi to Pluto and reset the counter",
        None,
        "auto",
    )
    .await;
    assert_eq!(
        (r.provider.as_str(), r.model.as_str(), r.cost_usd),
        ("local", "qwen3:8b", 0.0)
    );
    assert_eq!(r.text, "Said hi. Resetting needs your OK.");
    assert_eq!(r.steps.len(), 2);
    assert!(
        r.steps[0].ok && r.steps[0].summary.contains("Hello, Pluto!"),
        "{:?}",
        r.steps[0]
    );
    assert!(!r.steps[1].ok);
    assert_eq!(r.approvals.len(), 1);
    assert_eq!(
        (
            r.approvals[0].action.as_str(),
            r.approvals[0].label.as_str()
        ),
        ("counter.reset", "Reset counter")
    );
    {
        let seen = fakes.ollama_seen.lock().unwrap();
        let tools: Vec<&str> = seen[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();
        assert!(tools.contains(&"hello__greet_say") && tools.contains(&"kernel_status"));
        assert!(
            !tools.contains(&"hello__debug_crash"),
            "button-only actions are never offered"
        );
        assert!(
            seen[0]["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("tsundere")
        );
    }

    // The same conversation remembers what was said.
    let r2 = chat(&mut ws, "Again", Some(r.conversation.clone()), "auto").await;
    assert_eq!(r2.conversation, r.conversation);
    let history_len = fakes.ollama_seen.lock().unwrap().last().unwrap()["messages"]
        .as_array()
        .unwrap()
        .len();
    assert!(history_len > 6, "{history_len}");

    // Ollama goes down: Claude answers instead, and its cost is counted.
    fakes.ollama_down.store(true, Ordering::SeqCst);
    let r3 = chat(&mut ws, "Did I say hello?", None, "auto").await;
    assert_eq!(
        (r3.provider.as_str(), r3.model.as_str()),
        ("claude", "claude-haiku-4-5")
    );
    assert_eq!(r3.text, "Checked: you've said hello.");
    let expected = (1800.0 + 2000.0 + (25.0 + 30.0) * 5.0) / 1e6;
    assert!((r3.cost_usd - expected).abs() < 1e-12, "{}", r3.cost_usd);
    assert_eq!(
        fakes.claude_seen.lock().unwrap()[1]["messages"][2]["content"][0]["tool_use_id"],
        "toolu_1"
    );
    let spend = running.node.assistant.spend(&running.node);
    assert!((spend - expected).abs() < 1e-12);

    // "local only" doesn't fall back.
    let Payload::Error(e) = request(
        &mut ws,
        Payload::ChatSend(ChatSend {
            conversation: None,
            text: "hi".into(),
            provider: Some("local".into()),
        }),
    )
    .await
    else {
        panic!("expected an error")
    };
    assert!(e.message.contains("Ollama"), "{}", e.message);

    // What the assistant ran is in the activity log, as the assistant.
    let Payload::Activity(a) = request(
        &mut ws,
        Payload::ActivityQuery(ActivityQuery {
            limit: Some(20),
            module: Some("hello".into()),
        }),
    )
    .await
    else {
        panic!()
    };
    assert!(
        a.entries
            .iter()
            .any(|e| e.action == "greet.say" && e.actor.kind == ActorKind::Assistant)
    );

    running.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn laya_reads_status_early_and_shortens_the_tool_list() {
    if std::env::var_os("KERNEL_TEST_PYTHON").is_none() {
        eprintln!("skipping: set KERNEL_TEST_PYTHON to a Python with kernel_sdk installed");
        return;
    }
    let fakes = Arc::new(Fakes::default());
    let ollama_url = serve(
        Router::new()
            .route("/api/chat", post(ollama))
            .with_state(fakes.clone()),
    )
    .await;
    // hello, and a stand-in for LAYA with canned decisions.
    let modules = tempfile::tempdir().unwrap();
    copy_dir(&repo_modules().join("hello"), &modules.path().join("hello"));
    copy_dir(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fake_laya"),
        &modules.path().join("laya"),
    );
    let data = tempfile::tempdir().unwrap();
    let assistant = AssistantConfig {
        ollama_urls: vec![ollama_url],
        laya_tools: 2,
        ..AssistantConfig::default()
    };
    let (running, mut ws) = start_node(
        modules.path().to_path_buf(),
        &["hello", "laya"],
        assistant,
        data.path(),
    )
    .await;

    // Sure it's a status question: the status is read before the model is asked, and the
    // model sees two buttons (LAYA's first two) plus the status tool.
    let r = chat(&mut ws, "how is hello doing?", None, "auto").await;
    assert!(r.steps.is_empty(), "a status read isn't a button press");
    let route = r.route.clone().unwrap_or_default();
    assert!(
        route.starts_with("LAYA: 2 of ")
            && route.ends_with(" buttons, checked Hello first (97% sure)"),
        "{route}"
    );
    {
        let seen = fakes.ollama_seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "one model call instead of two");
        let messages = seen[0]["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        assert_eq!(last["role"], "tool");
        assert!(
            last["content"].as_str().unwrap().contains("greetings"),
            "{last}"
        );
        assert_eq!(offered(&seen[0]).len(), 3);
        assert_eq!(offered(&seen[0])[0], "kernel_status");
    }

    // Sure of a button: it is NOT pressed; the model decides (and the fake model calls
    // greet.say and counter.reset, as always).
    let r2 = chat(&mut ws, "count them", Some(r.conversation.clone()), "auto").await;
    let route2 = r2.route.clone().unwrap_or_default();
    assert!(
        route2.starts_with("LAYA: 2 of ") && route2.ends_with(" buttons"),
        "{route2}"
    );
    assert!(r2.steps.iter().all(|s| s.action != "greet.count"));
    assert!(
        offered(&fakes.ollama_seen.lock().unwrap()[1]).contains(&"hello__greet_count".to_string()),
        "LAYA's first guess is offered"
    );

    // What LAYA was asked: hello's buttons and its status, never LAYA's own; the message
    // before as context.
    let deadline = Instant::now() + Duration::from_secs(5);
    let laya = loop {
        let m = running
            .node
            .catalog()
            .into_iter()
            .find(|m| m.id == "laya")
            .unwrap();
        if m.status.as_ref().is_some_and(|s| s["seen"][1].is_object()) {
            break m;
        }
        assert!(Instant::now() < deadline, "LAYA's status never caught up");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let asked = &laya.status.unwrap()["seen"][1];
    assert_eq!(asked["context"], "how is hello doing?");
    let options: Vec<&str> = asked["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_str().unwrap())
        .collect();
    assert!(options.contains(&"status:hello") && options.contains(&"hello__greet_say"));
    assert!(!options.iter().any(|o| o.contains("laya")), "{options:?}");
    assert!(!options.contains(&"hello__debug_crash"));

    running.shutdown().await;
}
