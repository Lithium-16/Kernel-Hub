//! Contract test: every zod fixture must round-trip through the Rust types unchanged.

use std::path::PathBuf;

use kernel_protocol::{Envelope, Payload};
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/protocol/fixtures")
}

#[test]
fn fixtures_round_trip() {
    let mut count = 0;
    for entry in std::fs::read_dir(fixtures_dir()).expect("fixtures dir") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let original: Value = serde_json::from_str(&text).unwrap();
        let env = Envelope::decode(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let back: Value = serde_json::from_str(&env.encode()).unwrap();
        assert_eq!(back, original, "{} did not round-trip", path.display());
        count += 1;
    }
    assert!(count >= 9, "expected at least 9 fixtures, found {count}");
}

#[test]
fn rejects_other_versions() {
    let text = r#"{"v":2,"id":"x","ts":"2026-09-24T20:11:02Z","type":"catalog.get","body":{}}"#;
    assert!(Envelope::decode(text).is_err());
}

#[test]
fn rejects_unknown_types() {
    let text = r#"{"v":1,"id":"x","ts":"2026-09-24T20:11:02Z","type":"nope","body":{}}"#;
    assert!(Envelope::decode(text).is_err());
}

#[test]
fn reply_sets_re_and_fresh_id() {
    let req = Envelope::new(Payload::CatalogGet(Default::default()));
    let rep = Envelope::reply(
        &req.id,
        Payload::Catalog(kernel_protocol::Catalog { modules: vec![] }),
    );
    assert_eq!(rep.re.as_deref(), Some(req.id.as_str()));
    assert_ne!(rep.id, req.id);
    assert!(rep.ts.ends_with('Z'));
}
