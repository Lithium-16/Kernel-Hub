//! A minimal MCP client over a child process's stdio (newline-delimited JSON-RPC 2.0).
//!
//! It covers what the node needs from a module: the initialize handshake, `ping`,
//! `tools/call` and `resources/read`. Requests from the server are answered with
//! "method not found", except `ping`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use kernel_protocol::{ErrorCode, ErrorInfo};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc, oneshot, watch};

pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
pub const STATUS_URI: &str = "kernel://status";

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("module connection closed")]
    Closed,
    #[error("request timed out")]
    Timeout,
    #[error("rpc error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("protocol error: {0}")]
    Protocol(String),
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, McpError>>>>>;

pub struct McpClient {
    out: mpsc::Sender<String>,
    pending: Pending,
    next_id: AtomicU64,
    closed: watch::Sender<bool>,
}

impl McpClient {
    /// Start reader and writer tasks for a module's stdout/stdin.
    pub fn start<R, W>(reader: R, writer: W) -> Arc<Self>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (out, mut rx) = mpsc::channel::<String>(64);
        let (closed, mut closed_rx) = watch::channel(false);
        let pending: Pending = Arc::default();

        tokio::spawn(async move {
            let mut writer = writer;
            loop {
                let line = tokio::select! {
                    line = rx.recv() => line,
                    _ = closed_rx.changed() => None,
                };
                let Some(line) = line else { break };
                if writer.write_all(line.as_bytes()).await.is_err()
                    || writer.write_all(b"\n").await.is_err()
                    || writer.flush().await.is_err()
                {
                    break;
                }
            }
            // Dropping the writer closes the module's stdin, which asks it to exit.
            let _ = writer.shutdown().await;
        });

        let reader_pending = pending.clone();
        let reply = out.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    tracing::warn!(line, "module sent non-JSON on stdout");
                    continue;
                };
                handle_incoming(msg, &reader_pending, &reply).await;
            }
            for (_, tx) in reader_pending.lock().await.drain() {
                let _ = tx.send(Err(McpError::Closed));
            }
        });

        Arc::new(Self {
            out,
            pending,
            next_id: AtomicU64::new(1),
            closed,
        })
    }

    /// Close the module's stdin. MCP stdio servers exit when their input ends.
    pub fn close(&self) {
        let _ = self.closed.send(true);
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if self.out.send(msg.to_string()).await.is_err() {
            self.pending.lock().await.remove(&id);
            return Err(McpError::Closed);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(McpError::Closed),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(McpError::Timeout)
            }
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.out
            .send(msg.to_string())
            .await
            .map_err(|_| McpError::Closed)
    }

    pub async fn initialize(&self, timeout: Duration) -> Result<Value, McpError> {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": "kerneld", "version": env!("CARGO_PKG_VERSION")},
                }),
                timeout,
            )
            .await?;
        self.notify("notifications/initialized", json!({})).await?;
        Ok(result)
    }

    pub async fn ping(&self, timeout: Duration) -> Result<(), McpError> {
        self.request("ping", json!({}), timeout).await.map(|_| ())
    }

    /// Call an action's tool. The outer error is transport-level, the inner one is the
    /// module's own `{code, message}` error.
    pub async fn call_tool(
        &self,
        name: &str,
        args: &Map<String, Value>,
        timeout: Duration,
    ) -> Result<Result<Value, ErrorInfo>, McpError> {
        let result = self
            .request(
                "tools/call",
                json!({"name": name, "arguments": args}),
                timeout,
            )
            .await?;
        Ok(parse_tool_result(&result))
    }

    pub async fn read_status(&self, timeout: Duration) -> Result<Map<String, Value>, McpError> {
        let result = self
            .request("resources/read", json!({"uri": STATUS_URI}), timeout)
            .await?;
        let text = result["contents"][0]["text"]
            .as_str()
            .ok_or_else(|| McpError::Protocol("status resource has no text".into()))?;
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => Ok(map),
            _ => Err(McpError::Protocol("status is not a JSON object".into())),
        }
    }
}

async fn handle_incoming(msg: Value, pending: &Pending, reply: &mpsc::Sender<String>) {
    let method = msg.get("method").and_then(Value::as_str);
    let id = msg.get("id").cloned();
    match (method, id) {
        (None, Some(id)) => {
            let Some(id) = id.as_u64() else { return };
            let Some(tx) = pending.lock().await.remove(&id) else {
                return;
            };
            let outcome = if let Some(err) = msg.get("error") {
                Err(McpError::Rpc {
                    code: err["code"].as_i64().unwrap_or(0),
                    message: err["message"].as_str().unwrap_or("").to_owned(),
                })
            } else {
                Ok(msg.get("result").cloned().unwrap_or(Value::Null))
            };
            let _ = tx.send(outcome);
        }
        (Some(method), Some(id)) => {
            let response = if method == "ping" {
                json!({"jsonrpc": "2.0", "id": id, "result": {}})
            } else {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}})
            };
            let _ = reply.send(response.to_string()).await;
        }
        _ => {}
    }
}

fn parse_tool_result(result: &Value) -> Result<Value, ErrorInfo> {
    let structured = result.get("structuredContent");
    let text = result["content"][0]["text"].as_str();
    if result["isError"].as_bool().unwrap_or(false) {
        let body = structured
            .and_then(|s| s.get("error").cloned())
            .or_else(|| text.and_then(|t| serde_json::from_str(t).ok()))
            .unwrap_or(Value::Null);
        let code = body["code"]
            .as_str()
            .and_then(ErrorCode::parse)
            .unwrap_or(ErrorCode::ModuleFailed);
        let message = body["message"]
            .as_str()
            .or(text)
            .unwrap_or("the module reported an error")
            .to_owned();
        return Err(ErrorInfo::new(code, message));
    }
    Ok(structured
        .and_then(|s| s.get("result").cloned())
        .or_else(|| text.and_then(|t| serde_json::from_str(t).ok()))
        .unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_success_and_errors() {
        let ok = json!({"content":[{"type":"text","text":"{\"a\":1}"}],"structuredContent":{"result":{"a":1}}});
        assert_eq!(parse_tool_result(&ok), Ok(json!({"a": 1})));
        let text_only = json!({"content":[{"type":"text","text":"42"}]});
        assert_eq!(parse_tool_result(&text_only), Ok(json!(42)));
        let err = json!({"isError":true,"content":[{"type":"text","text":"{\"code\":\"busy\",\"message\":\"later\"}"}]});
        assert_eq!(
            parse_tool_result(&err),
            Err(ErrorInfo::new(ErrorCode::Busy, "later"))
        );
        let odd = json!({"isError":true,"content":[{"type":"text","text":"boom"}]});
        assert_eq!(
            parse_tool_result(&odd).unwrap_err().code,
            ErrorCode::ModuleFailed
        );
    }

    #[tokio::test]
    async fn matches_responses_and_answers_server_pings() {
        let (client_side, server_side) = tokio::io::duplex(4096);
        let (c_read, c_write) = tokio::io::split(client_side);
        let (s_read, mut s_write) = tokio::io::split(server_side);
        let client = McpClient::start(c_read, c_write);

        tokio::spawn(async move {
            let mut lines = BufReader::new(s_read).lines();
            // Ask the client for a ping first; it must answer.
            s_write
                .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"s1\",\"method\":\"ping\"}\n")
                .await
                .unwrap();
            while let Ok(Some(line)) = lines.next_line().await {
                let msg: Value = serde_json::from_str(&line).unwrap();
                if msg["id"] == "s1" {
                    assert_eq!(msg["result"], json!({}));
                    continue;
                }
                let reply = json!({"jsonrpc":"2.0","id":msg["id"],"result":{"echo":msg["method"]}});
                s_write
                    .write_all(format!("{reply}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });

        let r = client
            .request("tools/list", json!({}), Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(r["echo"], "tools/list");
    }

    #[tokio::test]
    async fn closed_connection_fails_pending_requests() {
        let (client_side, server_side) = tokio::io::duplex(1024);
        let (c_read, c_write) = tokio::io::split(client_side);
        let client = McpClient::start(c_read, c_write);
        drop(server_side);
        let err = client
            .request("ping", json!({}), Duration::from_secs(2))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::Closed), "{err:?}");
    }
}
