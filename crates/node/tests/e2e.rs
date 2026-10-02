//! End-to-end: a real kerneld running the real `hello` module, driven over WebSocket.
//!
//! Needs a Python with `kernel_sdk` installed, given as `KERNEL_TEST_PYTHON`
//! (for example `.venv/bin/python` after `uv pip install -e sdk/python`).
//! The test is skipped when the variable is not set.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use kernel_node::config::{Config, NotifyConfig, SupervisorConfig};
use kernel_protocol::{
    ActionInvoke, ActionResult, ActivityQuery, ActivityResult, Actor, ActorKind, ClientInfo, Empty,
    Envelope, ErrorCode, EventsQuery, Hello, ModuleInfo, ModuleState, Payload,
};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const TOKEN: &str = "e2e-test-token";

struct Client {
    ws: Ws,
    /// Events pushed by the node while waiting for replies.
    pushed: Vec<kernel_protocol::NodeEvent>,
}

impl Client {
    async fn connect(addr: std::net::SocketAddr) -> Self {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .expect("connect");
        Self {
            ws,
            pushed: Vec::new(),
        }
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
        let env = loop {
            let env = self.recv().await.expect("reply");
            match env.payload {
                Payload::Event(e) => self.pushed.push(e),
                _ => break env,
            }
        };
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

/// Stands in for a Discord webhook and keeps every message it is sent.
struct FakeDiscord {
    url: String,
    got: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl FakeDiscord {
    async fn start() -> Self {
        use axum::{Json, Router, extract::State, routing::post};
        type Got = std::sync::Arc<std::sync::Mutex<Vec<String>>>;
        async fn hook(State(got): State<Got>, Json(body): Json<Value>) {
            got.lock()
                .unwrap()
                .push(body["content"].as_str().unwrap_or_default().to_owned());
        }
        let got: Got = Default::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/hook", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/hook", post(hook))
            .with_state(got.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, got }
    }

    async fn wait_for(&self, n: usize) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let got = self.got.lock().unwrap().clone();
            if got.len() >= n {
                return got;
            }
            assert!(Instant::now() < deadline, "Discord got {got:?}");
            tokio::time::sleep(Duration::from_millis(50)).await;
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
    let discord = FakeDiscord::start().await;
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
        allow_lan: false,
        update: Default::default(),
        notify: NotifyConfig {
            discord_webhook: discord.url.clone(),
            min_level: "warn".into(),
            include: vec!["hello.test.*".into()],
            mute: vec![],
        },
        schedule: vec![],
        on_event: vec![
            toml::from_str(
                "name = 'Reset on test events'\nevent = 'hello.test.*'\nmodule = 'hello'\naction = 'counter.reset'\napproved = true",
            )
            .unwrap(),
        ],
        when: vec![],
        assistant: Default::default(),
        path: None,
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
    assert_eq!(hello.actions.len(), 6);

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

    // Quiet reads work but stay out of the activity log.
    let r = c.invoke(ActorKind::User, "greet.count", json!({})).await;
    assert_eq!(r.result.unwrap()["greetings"], 1);

    // Everything else is in the activity log, newest first, with who did it.
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

    // Events: reported by the module, pushed to clients, kept, and sent to Discord.
    let r = c
        .invoke(
            ActorKind::User,
            "event.emit",
            json!({"message": "Steve joined @everyone"}),
        )
        .await;
    assert!(r.ok, "{r:?}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !c.pushed.iter().any(|e| e.kind == "test.event") {
        assert!(Instant::now() < deadline, "event was never pushed");
        if let Some(Envelope {
            payload: Payload::Event(e),
            ..
        }) = c.recv().await
        {
            c.pushed.push(e);
        }
    }
    let events = match c
        .request(Payload::EventsQuery(EventsQuery {
            limit: None,
            module: Some("hello".into()),
        }))
        .await
    {
        Payload::Events(e) => e.events,
        other => panic!("expected events, got {other:?}"),
    };
    assert_eq!(events[0].kind, "test.event");
    assert_eq!(events[0].data["source"], "hello");
    assert_eq!(events[0].node_id, "test-node");
    let text = discord.wait_for(1).await.remove(0);
    assert!(text.contains("Test node · Hello"), "{text}");
    assert!(text.contains("@\u{200b}everyone"), "{text}");

    // The event also triggered the [[on_event]] automation, as an automation.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let log = match c
            .request(Payload::ActivityQuery(ActivityQuery {
                limit: Some(5),
                module: Some("hello".into()),
            }))
            .await
        {
            Payload::Activity(a) => a.entries,
            other => panic!("expected activity, got {other:?}"),
        };
        if let Some(e) = log.iter().find(|e| e.actor.kind == ActorKind::Automation) {
            assert_eq!(e.action, "counter.reset");
            assert_eq!(
                e.result,
                ActivityResult::Ok,
                "approved = true lets confirm actions run"
            );
            assert_eq!(
                e.actor.reference.as_deref(),
                Some("automation: Reset on test events")
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the automation never ran: {log:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Upkeep: logs, diagnostics and backups from the app.
    let r = c
        .invoke_on(
            "node",
            ActorKind::User,
            "logs.tail",
            json!({"module": "hello", "lines": 5}),
        )
        .await;
    assert!(r.ok, "{r:?}");
    let r = c
        .invoke_on(
            "node",
            ActorKind::User,
            "logs.tail",
            json!({"module": "../../etc"}),
        )
        .await;
    assert_eq!(error_code(&r), Some(ErrorCode::InvalidParams));
    let r = c
        .invoke_on("node", ActorKind::Assistant, "diag.bundle", json!({}))
        .await;
    let saved = r.result.expect("diagnostics saved")["saved"]
        .as_str()
        .unwrap()
        .to_owned();
    let names: Vec<String> = zip::ZipArchive::new(std::fs::File::open(&saved).unwrap())
        .unwrap()
        .file_names()
        .map(str::to_owned)
        .collect();
    assert!(
        names.contains(&"modules.json".to_string())
            && names.contains(&"logs/hello.log".to_string()),
        "{names:?}"
    );
    let r = c
        .invoke_on("node", ActorKind::User, "backup.now", json!({}))
        .await;
    assert!(std::path::Path::new(r.result.unwrap()["saved"].as_str().unwrap()).is_file());

    // Settings from the app: read, validated, saved, and the module restarts with them.
    let r = c
        .invoke_on(
            "node",
            ActorKind::User,
            "settings.get",
            json!({"module": "hello"}),
        )
        .await;
    let fields = r.result.unwrap()["fields"].clone();
    assert_eq!(fields[0]["key"], "greeting");
    assert_eq!(fields[0]["note"], "How greetings start.");
    let set = |values: &str| json!({"module": "hello", "values": values});
    let r = c
        .invoke_on(
            "node",
            ActorKind::Assistant,
            "settings.set",
            set(r#"{"greeting": "Hi"}"#),
        )
        .await;
    assert_eq!(error_code(&r), Some(ErrorCode::NotPermitted), "button only");
    let r = c
        .invoke_on(
            "node",
            ActorKind::User,
            "settings.set",
            set(r#"{"greeting": 5}"#),
        )
        .await;
    assert_eq!(error_code(&r), Some(ErrorCode::InvalidParams));
    let r = c
        .invoke_on(
            "node",
            ActorKind::User,
            "settings.set",
            set(r#"{"greeting": "Hi"}"#),
        )
        .await;
    assert_eq!(r.result.unwrap()["restarted"], true);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let r = c
            .invoke(ActorKind::User, "greet.say", json!({"name": "Pluto"}))
            .await;
        if r.result
            .as_ref()
            .is_some_and(|v| v["message"] == "Hi, Pluto!")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "never picked up the new setting: {r:?}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Schedules run as automations: confirm actions only when the schedule is approved.
    let scheduled = |approved| {
        let node = running.node.clone();
        async move {
            let req = ActionInvoke {
                module: "hello".into(),
                action: "counter.reset".into(),
                params: Default::default(),
                actor: Actor {
                    kind: ActorKind::Automation,
                    reference: Some("schedule: test".into()),
                },
                approval_id: None,
            };
            node.invoke_scheduled(req, approved).await
        }
    };
    assert_eq!(
        error_code(&scheduled(false).await),
        Some(ErrorCode::NeedsApproval)
    );
    assert!(scheduled(true).await.ok);
    assert!(running.node.has_action("hello", "counter.reset"));
    assert!(running.node.has_action("node", "update.check"));
    assert!(!running.node.has_action("hello", "nope.nope"));

    let node = running.node.clone();
    running.shutdown().await;
    assert_eq!(node.supervisor.catalog()[0].state, ModuleState::Stopped);
}

fn plain_config(python: &std::ffi::OsStr, data: &std::path::Path) -> Config {
    Config {
        node_id: "test-node".into(),
        node_name: "Test node".into(),
        listen: "127.0.0.1:0".parse().unwrap(),
        token: TOKEN.into(),
        modules_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
        data_dir: data.to_path_buf(),
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
        assistant: Default::default(),
        path: None,
    }
}

/// The app's switches: modules on or off, and auto-update, kept across restarts.
#[tokio::test(flavor = "multi_thread")]
async fn switches_from_the_app_stick() {
    let Some(python) = std::env::var_os("KERNEL_TEST_PYTHON") else {
        eprintln!("skipping: set KERNEL_TEST_PYTHON to a Python with kernel_sdk installed");
        return;
    };
    let data = tempfile::tempdir().unwrap();
    let running = kernel_node::start(plain_config(&python, data.path()))
        .await
        .expect("node starts");
    let mut c = Client::connect(running.addr).await;
    assert!(matches!(c.hello(TOKEN).await, Payload::Welcome(_)));
    c.wait_for_state(ModuleState::Running).await;

    // The web page for updating from a browser is served next to /ws.
    let page = reqwest::get(format!("http://{}/", running.addr))
        .await
        .unwrap();
    assert!(page.status().is_success());
    assert!(
        page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert!(page.text().await.unwrap().contains("Update everything"));

    let list = c
        .invoke_on("node", ActorKind::User, "modules.list", json!({}))
        .await;
    let modules = list.result.unwrap()["modules"].as_array().unwrap().clone();
    let find = |id: &str| modules.iter().find(|m| m["id"] == id).cloned();
    assert_eq!(find("hello").unwrap()["enabled"], true);
    assert_eq!(find("hello").unwrap()["state"], "running");
    assert_eq!(find("flowrace").unwrap()["enabled"], false);
    assert!(find("node").is_none());

    // Button-only: the assistant can't flip them.
    let denied = c
        .invoke_on(
            "node",
            ActorKind::Assistant,
            "modules.enable",
            json!({"module": "flowrace"}),
        )
        .await;
    assert_eq!(error_code(&denied), Some(ErrorCode::NotPermitted));

    let auto = c
        .invoke_on(
            "node",
            ActorKind::User,
            "update.auto",
            json!({"enabled": true}),
        )
        .await;
    assert!(auto.ok, "{auto:?}");
    let node = c
        .catalog()
        .await
        .into_iter()
        .find(|m| m.id == "node")
        .unwrap();
    assert_eq!(node.status.unwrap()["auto_install"], true);

    let off = c
        .invoke_on(
            "node",
            ActorKind::User,
            "modules.enable",
            json!({"module": "hello", "enabled": false}),
        )
        .await;
    // No config file in tests, so it can't restart itself; it says so.
    assert!(
        off.ok
            && off.result.as_ref().unwrap()["message"]
                .as_str()
                .unwrap()
                .starts_with("saved"),
        "{off:?}"
    );
    let bad = c
        .invoke_on(
            "node",
            ActorKind::User,
            "modules.enable",
            json!({"module": "nope"}),
        )
        .await;
    assert_eq!(error_code(&bad), Some(ErrorCode::InvalidParams));
    running.shutdown().await;

    // Started again: hello stays off although node.toml lists it, and auto-update stays on.
    let running = kernel_node::start(plain_config(&python, data.path()))
        .await
        .expect("node starts again");
    let mut c = Client::connect(running.addr).await;
    assert!(matches!(c.hello(TOKEN).await, Payload::Welcome(_)));
    let catalog = c.catalog().await;
    assert!(catalog.iter().all(|m| m.id != "hello"), "{catalog:?}");
    let node = catalog.into_iter().find(|m| m.id == "node").unwrap();
    assert_eq!(node.status.unwrap()["auto_install"], true);
    running.shutdown().await;
}
