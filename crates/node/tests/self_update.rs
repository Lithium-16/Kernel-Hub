//! Full self-update cycle against a fake GitHub: an installed kerneld is told to update,
//! hands off to its helper, exits, and comes back up from the swapped-in build.

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use kernel_node::update::{ASSET, exe_name};
use kernel_protocol::{
    ActionInvoke, Actor, ActorKind, ClientInfo, Empty, Envelope, Hello, ModuleInfo, Payload,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::Message;

const TOKEN: &str = "update-test-token";
const GH_TOKEN: &str = "github-read-token";

#[derive(Clone, Default)]
struct Fake {
    base: String,
    zip: Vec<u8>,
    sha: String,
    authorized: Arc<Mutex<Vec<bool>>>,
}

fn saw_token(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == format!("Bearer {GH_TOKEN}"))
}

async fn releases(State(f): State<Fake>, headers: HeaderMap) -> impl IntoResponse {
    f.authorized.lock().unwrap().push(saw_token(&headers));
    axum::Json(json!([
        {
            "tag_name": "node-build-5",
            "draft": false,
            "prerelease": false,
            "html_url": "https://github.com/o/r/releases/tag/node-build-5",
            "published_at": "2026-09-25T00:00:00Z",
            "assets": [
                { "name": ASSET, "url": format!("{}/assets/1", f.base) },
                { "name": format!("{ASSET}.sha256"), "url": format!("{}/assets/2", f.base) }
            ]
        },
        { "tag_name": "node-build-4", "assets": [] }
    ]))
}

async fn asset(
    State(f): State<Fake>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    headers: HeaderMap,
) -> axum::response::Response {
    f.authorized.lock().unwrap().push(saw_token(&headers));
    let wants_file = headers
        .get("accept")
        .is_some_and(|v| v == "application/octet-stream");
    if !wants_file {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    }
    match id {
        1 => f.zip.clone().into_response(),
        2 => format!("{}  {ASSET}\n", f.sha).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The "new build": the same kerneld binary plus a marker file.
fn build_zip(exe: &Path) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        // Stored, not deflated: compressing a debug build would take most of the test's time.
        let exec = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o755);
        w.start_file(exe_name(), exec).unwrap();
        w.write_all(&std::fs::read(exe).unwrap()).unwrap();
        w.start_file("VERSION", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(b"new").unwrap();
        w.finish().unwrap();
    }
    buf.into_inner()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn request(ws: &mut Ws, payload: Payload) -> Payload {
    let env = Envelope::new(payload);
    ws.send(Message::Text(env.encode().into())).await.unwrap();
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(120), ws.next())
            .await
            .expect("reply in time")
            .expect("open socket")
            .expect("valid frame");
        if let Message::Text(t) = msg {
            let reply = Envelope::decode(&t).unwrap();
            if reply.re.as_deref() == Some(env.id.as_str()) {
                return reply.payload;
            }
        }
    }
}

/// Connect and say hello, retrying until the node is up (or `timeout` passes).
async fn connect(addr: SocketAddr, timeout: Duration) -> Ws {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok((mut ws, _)) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await {
            let hello = Payload::Hello(Hello {
                client: ClientInfo {
                    name: "update-test".into(),
                    version: "0".into(),
                },
                token: TOKEN.into(),
            });
            if matches!(request(&mut ws, hello).await, Payload::Welcome(_)) {
                return ws;
            }
        }
        assert!(Instant::now() < deadline, "node did not come up on {addr}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn node_module(ws: &mut Ws) -> ModuleInfo {
    match request(ws, Payload::CatalogGet(Empty {})).await {
        Payload::Catalog(c) => c.modules.into_iter().find(|m| m.id == "node").unwrap(),
        other => panic!("expected catalog, got {other:?}"),
    }
}

async fn invoke(ws: &mut Ws, action: &str, kind: ActorKind) -> kernel_protocol::ActionResult {
    let req = ActionInvoke {
        module: "node".into(),
        action: action.into(),
        params: Default::default(),
        actor: Actor {
            kind,
            reference: Some("test".into()),
        },
        approval_id: None,
    };
    match request(ws, Payload::ActionInvoke(req)).await {
        Payload::ActionResult(r) => r,
        other => panic!("expected action.result, got {other:?}"),
    }
}

/// Stops whichever kerneld holds the node lock when the test ends, pass or fail.
struct KillOnDrop(PathBuf);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        kill(&self.0);
        if std::thread::panicking() {
            // Show what the node and the update helper logged.
            let logs = self.0.parent().unwrap().join("logs");
            for entry in std::fs::read_dir(&logs).into_iter().flatten().flatten() {
                let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
                eprintln!("---- {}\n{text}", entry.path().display());
            }
        }
    }
}

fn kill(pid_file: &Path) {
    let Ok(pid) = std::fs::read_to_string(pid_file) else {
        return;
    };
    let pid = pid.trim();
    let _ = if cfg!(windows) {
        Command::new("taskkill").args(["/F", "/PID", pid]).status()
    } else {
        Command::new("kill").arg(pid).status()
    };
}

#[tokio::test(flavor = "multi_thread")]
async fn installs_an_update_and_comes_back() {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_kerneld"));
    let root = tempfile::tempdir().unwrap();
    let root = root.path().to_path_buf();
    let app = root.join("app");
    std::fs::create_dir_all(app.join("modules")).unwrap();
    std::fs::copy(&exe, app.join(exe_name())).unwrap();
    std::fs::write(app.join("VERSION"), "old").unwrap();

    // Fake GitHub.
    let zip = build_zip(&exe);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let fake = Fake {
        base: base.clone(),
        sha: hex::encode(Sha256::digest(&zip)),
        zip,
        authorized: Arc::default(),
    };
    let routes = Router::new()
        .route("/repos/o/r/releases", get(releases))
        .route("/assets/{id}", get(asset))
        .with_state(fake.clone());
    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });

    let port = free_port();
    let config = root.join("node.toml");
    std::fs::write(
        &config,
        format!(
            r#"node_id = "pluto"
node_name = "Pluto"
token = "{TOKEN}"
listen = "127.0.0.1:{port}"
modules_dir = "app/modules"
data_dir = "data"

[update]
repo = "o/r"
token = "{GH_TOKEN}"
api = "{base}"
check_interval_h = 0
"#
        ),
    )
    .unwrap();
    let _cleanup = KillOnDrop(root.join("data/kerneld.pid"));

    let mut old = Command::new(app.join(exe_name()))
        .arg("--config")
        .arg(&config)
        .spawn()
        .unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    {
        let mut ws = connect(addr, Duration::from_secs(20)).await;
        let status = node_module(&mut ws).await.status.unwrap();
        assert_eq!(status["build"], 0);
        assert_eq!(status["update"], "unknown");

        // The assistant can look but not install.
        let checked = invoke(&mut ws, "update.check", ActorKind::Assistant).await;
        assert!(checked.ok, "{checked:?}");
        assert_eq!(checked.result.unwrap()["latest_build"], 5);
        let denied = invoke(&mut ws, "update.install", ActorKind::Assistant).await;
        assert!(!denied.ok);

        let installed = invoke(&mut ws, "update.install", ActorKind::User).await;
        assert!(installed.ok, "{installed:?}");
        assert_eq!(installed.result.unwrap()["installing"], 5);

        // The old process exits by itself...
        let deadline = Instant::now() + Duration::from_secs(30);
        while old.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "old kerneld did not exit");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // ...and the helper brings up the swapped-in build.
        let mut ws = connect(addr, Duration::from_secs(60)).await;
        assert_eq!(std::fs::read_to_string(app.join("VERSION")).unwrap(), "new");
        assert_eq!(
            std::fs::read_to_string(root.join("app.previous/VERSION")).unwrap(),
            "old"
        );
        let outcome: Value =
            serde_json::from_slice(&std::fs::read(root.join("data/update-result.json")).unwrap())
                .unwrap();
        assert_eq!(outcome["ok"], true, "{outcome}");
        let status = node_module(&mut ws).await.status.unwrap();
        assert!(status.contains_key("last_update"), "{status:?}");
        assert!(!status.contains_key("last_update_error"), "{status:?}");

        // Every GitHub request carried the token.
        let auth = fake.authorized.lock().unwrap().clone();
        assert!(auth.len() >= 3 && auth.iter().all(|a| *a), "{auth:?}");
    }
    let _ = old.kill();
}
