//! The connection to the relay: one thread with its own tokio runtime, reconnecting forever.

use crate::config::Config;
use crate::protocol::{Envelope, Msg, PROTOCOL, Person};
use futures::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_tungstenite::tungstenite::Message as WsMessage;

pub enum NetEvent {
    Connected {
        me: Person,
        friends: Vec<Person>,
    },
    Disconnected,
    Message(Envelope),
    Upgrade,
    /// Another login with the same token took over; this client stays quiet.
    Replaced,
    Uploaded {
        file_id: String,
    },
    UploadFailed {
        file_id: String,
        error: String,
    },
}

pub enum NetCmd {
    Send(Envelope),
    Upload {
        file_id: String,
        to: String,
        name: String,
        bytes: Vec<u8>,
    },
    Shutdown,
}

pub struct Net {
    pub cmds: UnboundedSender<NetCmd>,
    pub events: std::sync::mpsc::Receiver<NetEvent>,
}

pub fn backoff(attempt: u32, jitter: f32) -> Duration {
    let base = 2f32.powi(attempt.min(6) as i32).min(60.0);
    if base >= 60.0 {
        return Duration::from_secs(60);
    }
    Duration::from_millis(
        (f64::from(base) * (0.8 + 0.4 * f64::from(jitter.clamp(0.0, 1.0))) * 1000.0).round() as u64,
    )
}

pub struct Outbox {
    path: PathBuf,
    items: Vec<Envelope>,
}

fn mail_id(e: &Envelope) -> Option<&str> {
    match &e.msg {
        Msg::Mail { mail_id, .. } => Some(mail_id),
        _ => None,
    }
}

impl Outbox {
    pub fn open(path: PathBuf) -> Outbox {
        let items = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("outbox unreadable ({e}); moved aside");
                let _ = std::fs::rename(&path, path.with_extension("json.bad"));
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        Outbox { path, items }
    }

    fn write(&self) {
        if let Err(e) = crate::files::atomic_write(
            &self.path,
            &serde_json::to_vec(&self.items).unwrap_or_default(),
        ) {
            tracing::error!("couldn't save the outbox: {e}");
        }
    }

    pub fn push(&mut self, e: Envelope) {
        let Some(id) = mail_id(&e) else { return };
        if self.items.iter().any(|i| mail_id(i) == Some(id)) {
            return;
        }
        self.items.push(e);
        self.write();
    }

    pub fn remove_mail(&mut self, id: &str) {
        self.items.retain(|i| mail_id(i) != Some(id));
        self.write();
    }

    pub fn items(&self) -> &[Envelope] {
        &self.items
    }
}

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()?)
}

pub async fn download(config: &Config, file_id: &str) -> anyhow::Result<Vec<u8>> {
    let r = client()?
        .get(format!("{}/{file_id}", config.files_url()))
        .bearer_auth(&config.token)
        .send()
        .await?
        .error_for_status()?;
    Ok(r.bytes().await?.to_vec())
}

async fn upload(
    config: &Config,
    file_id: &str,
    to: &str,
    name: &str,
    bytes: Vec<u8>,
) -> anyhow::Result<()> {
    client()?
        .put(format!("{}/{file_id}", config.files_url()))
        .query(&[("to", to), ("name", name)])
        .bearer_auth(&config.token)
        .body(bytes)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

impl Net {
    pub fn start(
        config: Config,
        data_dir: PathBuf,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Net {
        let (cmd_tx, cmd_rx) = unbounded_channel();
        let (ev_tx, ev_rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("petlink-net".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                rt.block_on(run(
                    config,
                    data_dir,
                    cmd_rx,
                    ev_tx,
                    std::sync::Arc::new(wake),
                ));
            })
            .expect("net thread");
        Net {
            cmds: cmd_tx,
            events: ev_rx,
        }
    }
}

type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;
type Emit = std::sync::Arc<dyn Fn(NetEvent) + Send + Sync>;

/// Uploads run on their own task, not inside a relay session: whatever happens to the
/// connection, each one ends in exactly one `Uploaded` or `UploadFailed`.
fn spawn_upload(
    config: &Config,
    emit: &Emit,
    file_id: String,
    to: String,
    name: String,
    bytes: Vec<u8>,
) {
    let (config, emit) = (config.clone(), emit.clone());
    tokio::spawn(async move {
        emit(match upload(&config, &file_id, &to, &name, bytes).await {
            Ok(()) => NetEvent::Uploaded { file_id },
            Err(e) => NetEvent::UploadFailed {
                file_id,
                error: format!("{e:#}"),
            },
        });
    });
}

/// What a relay close code means for this client.
fn stop_for_close(code: u16) -> anyhow::Result<Stop> {
    match code {
        4426 => Ok(Stop::Upgrade),
        4409 => Ok(Stop::Replaced),
        4401 => anyhow::bail!("the relay refused the token"),
        _ => Ok(Stop::Lost),
    }
}

async fn run(
    config: Config,
    data_dir: PathBuf,
    mut cmds: UnboundedReceiver<NetCmd>,
    events: std::sync::mpsc::Sender<NetEvent>,
    wake: Wake,
) {
    let emit: Emit = std::sync::Arc::new(move |e: NetEvent| {
        let _ = events.send(e);
        wake();
    });
    let mut outbox = Outbox::open(data_dir.join("outbox.json"));
    let mut attempt = 0u32;
    loop {
        match session(&config, &mut cmds, &emit, &mut outbox, &mut attempt).await {
            Ok(Stop::Shutdown) => return,
            Ok(Stop::Upgrade) => {
                emit(NetEvent::Upgrade);
                return;
            }
            Ok(Stop::Replaced) => {
                emit(NetEvent::Replaced);
                return;
            }
            Ok(Stop::Lost) => {}
            Err(e) => tracing::info!("relay connection failed: {e:#}"),
        }
        emit(NetEvent::Disconnected);
        let wait = backoff(attempt, rand::random());
        attempt += 1;
        // Keep accepting mail while offline; it waits in the outbox.
        let deadline = tokio::time::sleep(wait);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                cmd = cmds.recv() => match cmd {
                    None | Some(NetCmd::Shutdown) => return,
                    Some(NetCmd::Send(e)) => outbox.push(e),
                    Some(NetCmd::Upload { file_id, to, name, bytes }) => spawn_upload(&config, &emit, file_id, to, name, bytes),
                },
            }
        }
    }
}

enum Stop {
    Shutdown,
    Lost,
    Upgrade,
    Replaced,
}

async fn session(
    config: &Config,
    cmds: &mut UnboundedReceiver<NetCmd>,
    emit: &Emit,
    outbox: &mut Outbox,
    attempt: &mut u32,
) -> anyhow::Result<Stop> {
    let (ws, _) = tokio_tungstenite::connect_async(config.relay_url.as_str()).await?;
    let (mut tx, mut rx) = ws.split();
    let hello = Envelope::new(
        "",
        Msg::Hello {
            token: config.token.clone(),
            client_version: option_env!("PETLINK_BUILD").unwrap_or("dev").into(),
        },
    );
    tx.send(WsMessage::text(serde_json::to_string(&hello)?))
        .await?;
    loop {
        tokio::select! {
            frame = rx.next() => {
                let Some(frame) = frame else { return Ok(Stop::Lost) };
                let text = match frame? {
                    WsMessage::Text(t) => t,
                    WsMessage::Close(Some(c)) => return stop_for_close(u16::from(c.code)),
                    WsMessage::Close(_) => return Ok(Stop::Lost),
                    _ => continue,
                };
                let env: Envelope = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(e) => { tracing::warn!("unknown message from the relay: {e}"); continue }
                };
                if env.v != PROTOCOL { return Ok(Stop::Upgrade) }
                match &env.msg {
                    Msg::Welcome { me, friends } => {
                        *attempt = 0;
                        emit(NetEvent::Connected { me: me.clone(), friends: friends.clone() });
                        for item in outbox.items().to_vec() {
                            tx.send(WsMessage::text(serde_json::to_string(&item)?)).await?;
                        }
                    }
                    Msg::Error { code, .. } if code == "upgrade" => return Ok(Stop::Upgrade),
                    Msg::Delivered { mail_id } => { outbox.remove_mail(mail_id); emit(NetEvent::Message(env)) }
                    _ => emit(NetEvent::Message(env)),
                }
            }
            cmd = cmds.recv() => match cmd {
                None | Some(NetCmd::Shutdown) => { let _ = tx.send(WsMessage::Close(None)).await; return Ok(Stop::Shutdown) }
                Some(NetCmd::Send(e)) => {
                    outbox.push(e.clone());
                    tx.send(WsMessage::text(serde_json::to_string(&e)?)).await?;
                }
                Some(NetCmd::Upload { file_id, to, name, bytes }) => spawn_upload(config, emit, file_id, to, name, bytes),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Envelope, Msg};
    use std::time::Duration;

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff(0, 0.5), Duration::from_secs(1));
        assert_eq!(backoff(3, 0.5), Duration::from_secs(8));
        assert_eq!(backoff(20, 0.5), Duration::from_secs(60));
        assert!(
            backoff(3, 0.0) >= Duration::from_millis(6400)
                && backoff(3, 1.0) <= Duration::from_millis(9600)
        );
    }

    fn mail(id: &str) -> Envelope {
        Envelope::new(
            "u_fr",
            Msg::Mail {
                mail_id: id.into(),
                text: "hi".into(),
                files: vec![],
            },
        )
    }

    #[test]
    fn close_codes_decide_whether_to_reconnect() {
        assert!(matches!(stop_for_close(4409), Ok(Stop::Replaced)));
        assert!(matches!(stop_for_close(4426), Ok(Stop::Upgrade)));
        assert!(stop_for_close(4401).is_err());
        assert!(matches!(stop_for_close(1000), Ok(Stop::Lost)));
    }

    #[test]
    fn an_upload_with_no_relay_still_ends_in_upload_failed() {
        let config = Config {
            relay_url: "ws://127.0.0.1:9/ws".into(),
            token: "t".into(),
            scale: 2,
            bow_hue: None,
            peek: false,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let emit: Emit = std::sync::Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        });
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            spawn_upload(
                &config,
                &emit,
                "f1".into(),
                "u".into(),
                "a.txt".into(),
                vec![1],
            );
            let got = loop {
                if let Ok(e) = rx.try_recv() {
                    break e;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            };
            assert!(matches!(got, NetEvent::UploadFailed { file_id, .. } if file_id == "f1"));
        });
    }

    #[test]
    fn outbox_survives_a_restart_and_keeps_only_mail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outbox.json");
        let mut o = Outbox::open(path.clone());
        o.push(mail("m1"));
        o.push(mail("m2"));
        o.push(Envelope::new("u_fr", Msg::VisitStart));
        o.push(mail("m1")); // same mail again: kept once
        drop(o);
        let mut o = Outbox::open(path.clone());
        assert_eq!(o.items().len(), 2);
        o.remove_mail("m1");
        assert_eq!(Outbox::open(path).items().len(), 1);
    }

    #[test]
    fn a_corrupt_outbox_starts_empty_and_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outbox.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(Outbox::open(path.clone()).items().is_empty());
        assert!(dir.path().join("outbox.json.bad").exists());
    }
}
