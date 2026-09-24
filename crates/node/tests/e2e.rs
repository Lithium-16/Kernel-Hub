//! End-to-end: a real kerneld running the real `hello` module, driven over WebSocket.
//!
//! Needs a Python with `kernel_sdk` installed, given as `KERNEL_TEST_PYTHON`
//! (for example `.venv/bin/python` after `uv pip install -e sdk/python`).
//! The test is skipped when the variable is not set.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use kernel_node::config::{Config, SupervisorConfig};
use kernel_protocol::{
    ActionInvoke, ActionResult, ActivityQuery, ActivityResult, Actor, ActorKind, ClientInfo, Empty,
    Envelope, ErrorCode, Hello, ModuleInfo, ModuleState, Payload,
};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const TOKEN: &str = "e2e-test-token";

struct Client {
    ws: Ws,
}

impl Client {
    async fn connect(addr: std::net::SocketAddr) -> Self {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .expect("connect");
        Self { ws }
    }

    async fn send(&mut self, payload: Payload) -> String {
        let env = Envelope::new(payload);
        self.ws
            .send(Message::Text(env.encode().into()))
            .await
            .expect("send");
        env.id
    }

    async fn recv(&mut self) -> Option<Envelope> {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(20), self.ws.next())
                .await
                .expect("recv timeout")?;
            match msg {
                Ok(Message::Text(t)) => return Some(Envelope::decode(&t).expect("valid envelope")),
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => continue,
            }
        }
    }

    async fn request(&mut self, payload: Payload) -> Payload {
        let id = self.send(payload).await;
        let env = self.recv().await.expect("reply");
        assert_eq!(
            env.re.as_deref(),
            Some(id.as_str()),
            "reply must reference the request"
        );
        env.payload
    }

    async fn hello(&mut self, token: &str) -> Payload {
        self.request(Payload::Hello(Hello {
            client: ClientInfo {
                name: "e2e".into(),
                version: "0".into(),
            },
            token: token.into(),
        }))
        .await
    }

    async fn catalog(&mut self) -> Vec<ModuleInfo> {
        match self.request(Payload::CatalogGet(Empty {})).await {
            Payload::Catalog(c) => c.modules,
            other => panic!("expected catalog, got {other:?}"),
        }
    }

    async fn invoke(&mut self, kind: ActorKind, action: &str, params: Value) -> ActionResult {
        self.invoke_on("hello", kind, action, params).await
    }

    async fn invoke_on(
        &mut self,
        module: &str,
        kind: ActorKind,
        action: &str,
        params: Value,
    ) -> ActionResult {
        let req = ActionInvoke {
            module: module.into(),
            action: action.into(),
            params: params.as_object().cloned().unwrap_or_default(),
            actor: Actor {
                kind,
                reference: Some("e2e".into()),
            },
            approval_id: None,
        };
        match self.request(Payload::ActionInvoke(req)).await {
            Payload::ActionResult(r) => r,
            other => panic!("expected action.result, got {other:?}"),
        }
    }

    async fn wait_for_state(&mut self, state: ModuleState) -> ModuleInfo {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let hello = self
                .catalog()
                .await
                .into_iter()
                .find(|m| m.id == "hello")
                .expect("hello in catalog");
            if hello.state == state {
                return hello;
            }
            assert!(
                Instant::now() < deadline,
                "hello never reached {state:?} (last: {:?})",
                hello.state
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

fn error_code(r: &ActionResult) -> Option<ErrorCode> {
    r.error.as_ref().map(|e| e.code)
}

#[tokio::test(flavor = "multi_thread")]
async fn node_runs_hello_module_end_to_end() {
    let Some(python) = std::env::var_os("KERNEL_TEST_PYTHON") else {
        eprintln!("skipping: set KERNEL_TEST_PYTHON to a Python with kernel_sdk installed");
        return;
    };
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
            ping_interval_ms: 500,
            ping_misses: 2,
            status_interval_ms: 200,
            start_timeout_ms: 20_000,
            backoff_initial_ms: 200,
            backoff_max_ms: 1_000,
            ..SupervisorConfig::default()
        },
    };
    let running = kernel_node::start(cfg).await.expect("node starts");

    // A wrong token is refused and the connection closes.
    let mut intruder = Client::connect(running.addr).await;
    match intruder.hello("wrong-token-here").await {
        Payload::Error(e) => assert_eq!(e.code, ErrorCode::Unauthorized),
        other => panic!("expected an error, got {other:?}"),
    }
    assert!(
        intruder.recv().await.is_none(),
        "connection should close after a failed hello"
    );

    let mut c = Client::connect(running.addr).await;
    match c.hello(TOKEN).await {
        Payload::Welcome(w) => {
            assert_eq!(w.node.id, "test-node");
            assert!(w.capabilities.contains(&"actions".to_string()));
        }
        other => panic!("expected welcome, got {other:?}"),
    }

    let hello = c.wait_for_state(ModuleState::Running).await;
    assert_eq!(hello.actions.len(), 4);

    // A button press works, and parameter validation comes from the module.
    let r = c
        .invoke(ActorKind::User, "greet.say", json!({"name": "Pluto"}))
        .await;
    assert!(r.ok, "{r:?}");
    assert_eq!(r.result.unwrap()["message"], "Hello, Pluto!");
    let r = c
        .invoke(ActorKind::User, "greet.say", json!({"name": 5}))
        .await;
    assert_eq!(error_code(&r), Some(ErrorCode::InvalidParams));

    // The assistant may run safe actions only.
    assert!(
        c.invoke(ActorKind::Assistant, "greet.say", json!({}))
            .await
            .ok
    );
    let r = c
        .invoke(ActorKind::Assistant, "counter.reset", json!({}))
        .await;
    assert_eq!(error_code(&r), Some(ErrorCode::NeedsApproval));
    let r = c
        .invoke(ActorKind::Assistant, "debug.crash", json!({}))
        .await;
    assert_eq!(error_code(&r), Some(ErrorCode::NotPermitted));
    // A human can press the confirm action directly.
    assert!(
        c.invoke(ActorKind::Phone, "counter.reset", json!({}))
            .await
            .ok
    );

    // The module enforces the action's own timeout.
    let r = c
        .invoke(ActorKind::User, "slow.wait", json!({"seconds": 3}))
        .await;
    assert_eq!(error_code(&r), Some(ErrorCode::Timeout));

    let r = c.invoke_on("nope", ActorKind::User, "a.b", json!({})).await;
    assert_eq!(error_code(&r), Some(ErrorCode::InvalidParams));

    // Status is polled from the module.
    assert!(c.invoke(ActorKind::User, "greet.say", json!({})).await.ok);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let m = c.catalog().await.remove(0);
        if m.status
            .as_ref()
            .and_then(|s| s.get("greetings"))
            .and_then(Value::as_u64)
            == Some(1)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "status never showed the greeting: {:?}",
            m.status
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // A crash is reported, then the supervisor restarts the module.
    let r = c.invoke(ActorKind::User, "debug.crash", json!({})).await;
    assert_eq!(error_code(&r), Some(ErrorCode::ModuleFailed), "{r:?}");
    c.wait_for_state(ModuleState::Running).await;
    let r = c.invoke(ActorKind::User, "greet.say", json!({})).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(
        r.result.unwrap()["count"],
        1,
        "a fresh process starts counting again"
    );

    // Everything is in the activity log, newest first, with who did it.
    let activity = match c
        .request(Payload::ActivityQuery(ActivityQuery {
            limit: Some(50),
            module: Some("hello".into()),
        }))
        .await
    {
        Payload::Activity(a) => a.entries,
        other => panic!("expected activity, got {other:?}"),
    };
    assert_eq!(activity.len(), 10);
    assert_eq!(activity[0].action, "greet.say");
    let denied: Vec<_> = activity
        .iter()
        .filter(|e| e.result == ActivityResult::Denied)
        .collect();
    assert_eq!(denied.len(), 2);
    assert!(denied.iter().all(|e| e.actor.kind == ActorKind::Assistant));
    assert!(
        activity
            .iter()
            .any(|e| e.actor.kind == ActorKind::Phone && e.action == "counter.reset")
    );

    let node = running.node.clone();
    running.shutdown().await;
    assert_eq!(node.supervisor.catalog()[0].state, ModuleState::Stopped);
}
