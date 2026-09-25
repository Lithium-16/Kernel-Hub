//! WebSocket API: `hello` first, then catalog, actions and activity.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use kernel_protocol::{Activity, Catalog, Envelope, ErrorCode, ErrorInfo, Payload, Welcome};
use tokio::sync::{mpsc, watch};

use crate::netfilter;
use crate::node::Node;

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const CAPABILITIES: &[&str] = &["catalog", "actions", "activity"];

#[derive(Clone)]
pub struct AppState {
    pub node: Arc<Node>,
    pub shutdown: watch::Receiver<bool>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Response {
    if !netfilter::is_allowed(addr.ip()) {
        tracing::warn!(%addr, "rejected connection from outside the LAN/tailnet");
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, addr, state))
}

fn error(re: Option<&str>, code: ErrorCode, message: impl Into<String>) -> Envelope {
    let payload = Payload::Error(ErrorInfo::new(code, message));
    match re {
        Some(id) => Envelope::reply(id, payload),
        None => Envelope::new(payload),
    }
}

async fn handle_socket(socket: WebSocket, addr: SocketAddr, state: AppState) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Envelope>(64);
    let writer = tokio::spawn(async move {
        while let Some(env) = rx.recv().await {
            if sink.send(Message::Text(env.encode().into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    if authenticate(&mut stream, &tx, &state).await {
        tracing::info!(%addr, "client connected");
        serve_client(&mut stream, &tx, &state).await;
        tracing::info!(%addr, "client disconnected");
    }
    drop(tx);
    let _ = writer.await;
}

async fn next_text(
    stream: &mut futures::stream::SplitStream<WebSocket>,
) -> Option<Result<String, ()>> {
    loop {
        match stream.next().await? {
            Ok(Message::Text(t)) => return Some(Ok(t.to_string())),
            Ok(Message::Binary(_)) => return Some(Err(())),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => continue,
        }
    }
}

async fn authenticate(
    stream: &mut futures::stream::SplitStream<WebSocket>,
    tx: &mpsc::Sender<Envelope>,
    state: &AppState,
) -> bool {
    let Ok(Some(Ok(text))) = tokio::time::timeout(HELLO_TIMEOUT, next_text(stream)).await else {
        let _ = tx
            .send(error(None, ErrorCode::Unauthorized, "expected hello"))
            .await;
        return false;
    };
    match Envelope::decode(&text) {
        Ok(Envelope {
            id,
            payload: Payload::Hello(hello),
            ..
        }) if state.node.check_token(&hello.token) => {
            let welcome = Welcome {
                node: state.node.info(),
                capabilities: CAPABILITIES.iter().map(|s| (*s).into()).collect(),
            };
            let _ = tx
                .send(Envelope::reply(&id, Payload::Welcome(welcome)))
                .await;
            true
        }
        Ok(env) => {
            let _ = tx
                .send(error(
                    Some(&env.id),
                    ErrorCode::Unauthorized,
                    "invalid token or missing hello",
                ))
                .await;
            false
        }
        Err(e) => {
            let _ = tx
                .send(error(None, ErrorCode::Unauthorized, e.to_string()))
                .await;
            false
        }
    }
}

async fn serve_client(
    stream: &mut futures::stream::SplitStream<WebSocket>,
    tx: &mpsc::Sender<Envelope>,
    state: &AppState,
) {
    let mut shutdown = state.shutdown.clone();
    loop {
        let text = tokio::select! {
            t = next_text(stream) => t,
            _ = shutdown.changed() => None,
        };
        let Some(text) = text else { return };
        let Ok(text) = text else {
            let _ = tx
                .send(error(
                    None,
                    ErrorCode::BadRequest,
                    "binary messages are not supported",
                ))
                .await;
            continue;
        };
        let env = match Envelope::decode(&text) {
            Ok(env) => env,
            Err(e) => {
                let _ = tx
                    .send(error(None, ErrorCode::BadRequest, e.to_string()))
                    .await;
                continue;
            }
        };
        match env.payload {
            Payload::CatalogGet(_) => {
                let catalog = Catalog {
                    modules: state.node.supervisor.catalog(),
                };
                let _ = tx
                    .send(Envelope::reply(&env.id, Payload::Catalog(catalog)))
                    .await;
            }
            Payload::ActionInvoke(req) => {
                // Actions can be slow; run them without blocking this connection.
                let node = state.node.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = node.invoke(req).await;
                    let _ = tx
                        .send(Envelope::reply(&env.id, Payload::ActionResult(result)))
                        .await;
                });
            }
            Payload::ActivityQuery(q) => {
                let reply = match state
                    .node
                    .activity
                    .query(q.limit.unwrap_or(50), q.module.as_deref())
                {
                    Ok(entries) => {
                        Envelope::reply(&env.id, Payload::Activity(Activity { entries }))
                    }
                    Err(e) => error(Some(&env.id), ErrorCode::Internal, e.to_string()),
                };
                let _ = tx.send(reply).await;
            }
            _ => {
                let _ = tx
                    .send(error(
                        Some(&env.id),
                        ErrorCode::BadRequest,
                        "unexpected message type",
                    ))
                    .await;
            }
        }
    }
}
