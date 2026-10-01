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

#[tokio::test(flavor = "multi_thread")]
async fn assistant_runs_tools_asks_for_approval_and_falls_back() {
    let Some(python) = std::env::var_os("KERNEL_TEST_PYTHON") else {
        eprintln!("skipping: set KERNEL_TEST_PYTHON to a Python with kernel_sdk installed");
        return;
    };
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
    let cfg = Config {
        node_id: "test-node".into(),
        node_name: "Test node".into(),
        listen: "127.0.0.1:0".parse().unwrap(),
        token: TOKEN.into(),
        modules_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
        data_dir: data.path().to_path_buf(),
        python: PathBuf::from(python).to_string_lossy().into_owned(),
        node: "node".into(),
        enabled_modules: vec!["hello".into()],
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
        assistant: AssistantConfig {
            // The first address is down (like the VRAM proxy when the module is off).
            ollama_urls: vec!["http://127.0.0.1:1".into(), ollama_url],
            anthropic_api_key: "test-key".into(),
            anthropic_api: claude_url,
            ..AssistantConfig::default()
        },
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
        if c.modules
            .iter()
            .any(|m| m.id == "hello" && m.state == ModuleState::Running)
        {
            break;
        }
        assert!(Instant::now() < deadline, "hello never started");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

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
