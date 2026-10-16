//! Wire messages shared with the relay (modules/petlink/fixtures are the reference).

use serde::{Deserialize, Serialize};

pub const PROTOCOL: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    pub id: String,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(flatten)]
    pub msg: Msg,
}

impl Envelope {
    pub fn new(to: &str, msg: Msg) -> Self {
        Self {
            v: PROTOCOL,
            id: new_id(),
            from: String::new(),
            to: to.to_owned(),
            msg,
        }
    }
}

pub fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub bow_hue: u16,
    pub peek: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub online: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRef {
    pub file_id: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresenceState {
    Online,
    Away,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TouchKind {
    Pat,
    Poke,
    Yeet,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Msg {
    #[serde(rename = "hello")]
    Hello {
        token: String,
        client_version: String,
    },
    #[serde(rename = "welcome")]
    Welcome { me: Person, friends: Vec<Person> },
    #[serde(rename = "presence")]
    Presence { state: PresenceState },
    #[serde(rename = "mail")]
    Mail {
        mail_id: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        files: Vec<FileRef>,
    },
    #[serde(rename = "delivered")]
    Delivered { mail_id: String },
    #[serde(rename = "mail.read")]
    MailRead { mail_id: String },
    #[serde(rename = "peek.allow")]
    PeekAllow { on: bool },
    #[serde(rename = "peek.request")]
    PeekRequest { peek_id: String },
    #[serde(rename = "peek.result")]
    PeekResult {
        peek_id: String,
        #[serde(default)]
        file_id: String,
        #[serde(default)]
        denied: bool,
    },
    #[serde(rename = "visit.start")]
    VisitStart,
    #[serde(rename = "visit.end")]
    VisitEnd,
    #[serde(rename = "touch")]
    Touch { what: TouchKind },
    #[serde(rename = "error")]
    Error {
        code: String,
        message: String,
        #[serde(default)]
        r#ref: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn every_relay_fixture_round_trips() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/petlink/fixtures");
        let mut n = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            let want: serde_json::Value = serde_json::from_str(&text).unwrap();
            let env: Envelope =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(
                serde_json::to_value(&env).unwrap(),
                want,
                "{}",
                path.display()
            );
            n += 1;
        }
        assert_eq!(n, 13);
    }

    #[test]
    fn unknown_fields_and_kinds_do_not_crash() {
        let extra = r#"{"v":1,"id":"x","kind":"touch","from":"a","to":"b","what":"pat","later":1}"#;
        assert!(serde_json::from_str::<Envelope>(extra).is_ok());
        let unknown = r#"{"v":1,"id":"x","kind":"dance","from":"a","to":"b"}"#;
        assert!(serde_json::from_str::<Envelope>(unknown).is_err());
    }
}
