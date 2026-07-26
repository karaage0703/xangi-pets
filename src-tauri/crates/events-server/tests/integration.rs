// Integration tests for the embedded HTTP server + bubble pipeline. Events
// flow into the local bus via `process_event` (called by the pull-mode SSE
// client in production); these tests drive `process_event` directly and
// assert that the HTTP-side endpoints (`/api/pet/bubbles`, `/health`)
// reflect the resulting state.

use futures_util::StreamExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use xangi_events_server::{
    events, http_server, http_server::AppState, make_state_with_extra_pet_dirs,
    thread_state::ThreadStore,
};

async fn spawn_server() -> (SocketAddr, AppState) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = http_server::make_state();
    let app = http_server::build_router(state.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    // Give the listener a moment to start accepting.
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, state)
}

#[tokio::test]
async fn validate_rejects_bad_events() {
    assert!(events::validate(&json!({})).is_err(), "missing type");
    assert!(events::validate(&json!({"type": "bogus"})).is_err());
    // Schema v2 removed agent.thinking / agent.talking / agent.idle.
    assert!(
        events::validate(&json!({"type": "agent.thinking", "thread_id": "x", "turn_id": "u"}))
            .is_err(),
        "agent.thinking is no longer accepted",
    );
    assert!(
        events::validate(&json!({"type": "agent.idle", "thread_id": "x", "turn_id": "u"})).is_err(),
        "agent.idle is no longer accepted",
    );
    assert!(
        events::validate(&json!({"type": "turn.started"})).is_err(),
        "needs thread_id"
    );
    assert!(
        events::validate(&json!({"type": "turn.started", "thread_id": "x"})).is_err(),
        "needs turn_id",
    );
    assert!(
        events::validate(&json!({"type": "turn.started", "thread_id": "x", "turn_id": "u1"}))
            .is_ok()
    );
    assert!(
        events::validate(&json!({"type": "turn.aborted", "thread_id": "x", "turn_id": "u"}))
            .is_ok(),
        "turn.aborted is accepted",
    );
    assert!(
        events::validate(&json!({"type": "message.delta", "thread_id": "x", "turn_id": "u1"}))
            .is_err(),
        "delta needs text"
    );
    assert!(
        events::validate(&json!({"type": "agent.error", "thread_id": "x", "turn_id": "u1"}))
            .is_err(),
        "error needs message"
    );
}

#[tokio::test]
async fn thread_state_lifecycle() {
    let store = ThreadStore::new();

    // turn.started -> bubble.open
    let out = store.apply(&events::stamp(json!({
        "type": "turn.started",
        "thread_id": "T",
        "turn_id": "u1",
    })));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["type"], "bubble.open");

    // delta accumulates
    let out = store.apply(&events::stamp(json!({
        "type": "message.delta",
        "thread_id": "T",
        "turn_id": "u1",
        "text": "hello",
    })));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["type"], "bubble.delta");
    assert_eq!(out[0]["text"], "hello");

    let out = store.apply(&events::stamp(json!({
        "type": "message.delta",
        "thread_id": "T",
        "turn_id": "u1",
        "text": " world",
    })));
    assert_eq!(out[0]["text"], " world");

    // turn.complete -> bubble.close with combined text as last_message default
    // (v2: also force state -> idle since agent.idle no longer exists)
    let out = store.apply(&events::stamp(json!({
        "type": "turn.complete",
        "thread_id": "T",
        "turn_id": "u1",
    })));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["type"], "bubble.close");
    assert_eq!(out[0]["last_message"], "hello world");
    assert_eq!(store.aggregate_state(), "idle");
}

#[tokio::test]
async fn turn_aborted_closes_with_partial_text() {
    let store = ThreadStore::new();
    store.apply(&events::stamp(json!({
        "type": "turn.started", "thread_id": "T", "turn_id": "u1",
    })));
    store.apply(&events::stamp(json!({
        "type": "message.delta", "thread_id": "T", "turn_id": "u1", "text": "partial",
    })));
    let out = store.apply(&events::stamp(json!({
        "type": "turn.aborted", "thread_id": "T", "turn_id": "u1",
    })));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["type"], "bubble.close");
    assert_eq!(out[0]["last_message"], "partial");
    assert_eq!(out[0]["aborted"], true);
    assert_eq!(store.aggregate_state(), "idle");
}

#[tokio::test]
async fn bind_with_autoshift_advances_when_in_use() {
    // Bind one listener on an ephemeral port to act as "the port is in use".
    let blocker = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let occupied = blocker.local_addr().unwrap();

    // Now ask bind_with_autoshift to start at that same port. It should
    // discover the conflict and shift up by 1.
    let listener = xangi_events_server::bind_with_autoshift(occupied, 5)
        .await
        .expect("autoshift should find a free port");
    let bound = listener.local_addr().unwrap();
    assert_ne!(bound, occupied, "bound addr must differ from occupied");
    assert_eq!(
        bound.port(),
        occupied.port() + 1,
        "should have shifted exactly one slot",
    );
    drop(blocker);
    drop(listener);
}

#[tokio::test]
async fn bind_with_autoshift_returns_err_when_no_free_port() {
    // Hold 3 consecutive ports to exhaust the small range.
    let l1 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let p1 = l1.local_addr().unwrap().port();
    // Try to find two more consecutive ports we can occupy. This is best-effort:
    // if the next port is already taken by some other process, just skip the test.
    let l2 = match tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], p1 + 1))).await {
        Ok(l) => l,
        Err(_) => return,
    };
    let l3 = match tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], p1 + 2))).await {
        Ok(l) => l,
        Err(_) => return,
    };
    let result = xangi_events_server::bind_with_autoshift(
        SocketAddr::from(([127, 0, 0, 1], p1)),
        3, // tries: p1, p1+1, p1+2 — all occupied
    )
    .await;
    assert!(result.is_err(), "should fail when range is fully occupied");
    drop((l1, l2, l3));
}

#[tokio::test]
async fn turn_complete_reads_text_field() {
    let store = ThreadStore::new();
    store.apply(&events::stamp(json!({
        "type": "turn.started", "thread_id": "T", "turn_id": "u1",
    })));
    // v2: final text is in `text`, not `last_message`.
    let out = store.apply(&events::stamp(json!({
        "type": "turn.complete",
        "thread_id": "T",
        "turn_id": "u1",
        "text": "final v2 text",
    })));
    assert_eq!(out[0]["last_message"], "final v2 text");
}

#[tokio::test]
async fn delta_without_started_opens_implicit_bubble() {
    let store = ThreadStore::new();
    let out = store.apply(&events::stamp(json!({
        "type": "message.delta",
        "thread_id": "T",
        "turn_id": "u-implicit",
        "text": "hi",
    })));
    assert_eq!(out.len(), 2);
    assert_eq!(out[0]["type"], "bubble.open");
    assert_eq!(out[1]["type"], "bubble.delta");
}

#[tokio::test]
async fn end_to_end_process_event_and_bubble_sse() {
    let (addr, state) = spawn_server().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // Subscribe to bubbles SSE first so we don't miss anything.
    let bubbles_resp = client
        .get(format!("{base}/api/pet/bubbles"))
        .send()
        .await
        .unwrap();
    assert_eq!(bubbles_resp.status(), 200);
    let mut bubbles_stream = bubbles_resp.bytes_stream();

    // Drive a typical schema-v2 turn sequence directly through process_event,
    // exactly the way the pull-mode SSE client does in production.
    for ev in [
        json!({"type": "turn.started",  "thread_id": "discord:test", "turn_id": "u-1"}),
        json!({"type": "message.delta", "thread_id": "discord:test", "turn_id": "u-1", "text": "こん"}),
        json!({"type": "message.delta", "thread_id": "discord:test", "turn_id": "u-1", "text": "にちは"}),
        json!({"type": "turn.complete", "thread_id": "discord:test", "turn_id": "u-1", "text": "こんにちは"}),
    ] {
        http_server::process_event(&state, ev).expect("valid event accepted");
    }

    // Drain bubble events and look for bubble.close with the right last_message.
    let mut buf = String::new();
    let mut saw_close = false;
    let mut saw_open = false;
    let mut saw_delta = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(800);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), bubbles_stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                buf.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(end) = buf.find("\n\n") {
                    let frame = buf[..end].to_string();
                    buf.drain(..end + 2);
                    for line in frame.lines() {
                        if let Some(rest) = line.strip_prefix("data: ") {
                            if let Ok(ev) = serde_json::from_str::<Value>(rest) {
                                match ev["type"].as_str() {
                                    Some("bubble.open") => saw_open = true,
                                    Some("bubble.delta") => saw_delta += 1,
                                    Some("bubble.close") => {
                                        saw_close = true;
                                        assert_eq!(ev["last_message"], "こんにちは");
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        if saw_close {
            break;
        }
    }
    assert!(saw_open, "expected bubble.open");
    assert_eq!(saw_delta, 2, "expected 2 deltas, got {saw_delta}");
    assert!(saw_close, "expected bubble.close before timeout");
}

#[tokio::test]
async fn process_event_rejects_invalid() {
    let (_addr, state) = spawn_server().await;
    let err = http_server::process_event(&state, json!({"type": "wrong_type", "thread_id": "T"}))
        .expect_err("unknown event type should be rejected");
    assert!(err.contains("unknown event type"), "err = {err}");
}

#[tokio::test]
async fn removed_push_event_routes_stay_unavailable() {
    let (addr, _state) = spawn_server().await;
    let client = reqwest::Client::new();

    let post = client
        .post(format!("http://{addr}/events"))
        .json(&json!([{
            "type": "turn.started",
            "thread_id": "T",
            "turn_id": "u1"
        }]))
        .send()
        .await
        .unwrap();
    assert_eq!(post.status(), reqwest::StatusCode::NOT_FOUND);

    let get = client
        .get(format!("http://{addr}/events"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), reqwest::StatusCode::NOT_FOUND);
}

// These tests mutate process-wide env (HOME / XANGI_PET_DIR). Cargo runs
// integration tests with multiple threads by default, so we serialise via
// a shared mutex and recover from poisoning so one panicking test doesn't
// permanently lock out the rest.
fn env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    match env_lock().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Repoint HOME at a fresh tempdir so the host's real `~/.xangi/pets` and
/// `~/.codex/pets` don't accidentally satisfy assertions about an empty
/// search path. The returned tempdir guard must outlive the test body.
struct IsolatedHome {
    _temp: tempfile::TempDir,
    prev_home: Option<String>,
    prev_pet_dir: Option<String>,
}

impl IsolatedHome {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let prev_home = std::env::var("HOME").ok();
        let prev_pet_dir = std::env::var("XANGI_PET_DIR").ok();
        std::env::set_var("HOME", temp.path());
        std::env::remove_var("XANGI_PET_DIR");
        Self {
            _temp: temp,
            prev_home,
            prev_pet_dir,
        }
    }
}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        match &self.prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match &self.prev_pet_dir {
            Some(v) => std::env::set_var("XANGI_PET_DIR", v),
            None => std::env::remove_var("XANGI_PET_DIR"),
        }
    }
}

async fn spawn_server_with_state(state: AppState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = http_server::build_router(state);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

#[tokio::test]
async fn extra_pet_dir_is_appended_at_lowest_priority() {
    let _g = lock_env();
    let _home = IsolatedHome::new();

    let extra = PathBuf::from("/nonexistent/bundled/default-pets");
    let state = make_state_with_extra_pet_dirs(vec![extra.clone()]);
    let dirs: Vec<PathBuf> = state.pet_dirs.iter().cloned().collect();
    assert!(dirs.len() >= 3, "expected default 2 + extra, got {dirs:?}");
    assert_eq!(dirs.last().unwrap(), &extra, "extra must be at end");
}

#[tokio::test]
async fn extra_pet_dir_dedup_when_already_in_defaults() {
    let _g = lock_env();
    let _home = IsolatedHome::new();

    let home = std::env::var("HOME").unwrap();
    let dup = PathBuf::from(&home).join(".xangi").join("pets");
    let state = make_state_with_extra_pet_dirs(vec![dup.clone()]);
    let count = state.pet_dirs.iter().filter(|p| **p == dup).count();
    assert_eq!(count, 1, "duplicate dir should not appear twice");
}

#[tokio::test]
async fn xangi_pet_dir_override_skips_extras() {
    let _g = lock_env();
    let _home = IsolatedHome::new();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("XANGI_PET_DIR", temp.path());

    let extra = PathBuf::from("/should/not/be/included");
    let state = make_state_with_extra_pet_dirs(vec![extra.clone()]);
    let dirs: Vec<PathBuf> = state.pet_dirs.iter().cloned().collect();
    assert_eq!(dirs.len(), 1, "explicit override must be exclusive");
    assert!(!dirs.contains(&extra), "extra leaked past XANGI_PET_DIR");
}

#[tokio::test]
async fn list_pets_includes_bundled_default() {
    let _g = lock_env();
    let _home = IsolatedHome::new();

    // Lay down a fake bundled pet dir with a single sprite folder.
    let bundle = tempfile::tempdir().unwrap();
    let xangi_dir = bundle.path().join("xangi");
    std::fs::create_dir_all(&xangi_dir).unwrap();
    std::fs::write(
        xangi_dir.join("pet.json"),
        r#"{"id":"xangi","displayName":"xangi","spritesheetPath":"spritesheet.webp"}"#,
    )
    .unwrap();
    std::fs::write(xangi_dir.join("spritesheet.webp"), b"fake-sprite-bytes").unwrap();

    let state = make_state_with_extra_pet_dirs(vec![bundle.path().to_path_buf()]);
    let addr = spawn_server_with_state(state).await;
    let body: Value = reqwest::get(format!("http://{addr}/api/pet/list"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pets: Vec<String> = body["pets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        pets.iter().any(|p| p == "xangi"),
        "expected xangi in pets list (got {pets:?})",
    );
}

#[tokio::test]
async fn get_asset_falls_back_to_bundled_default() {
    let _g = lock_env();
    let _home = IsolatedHome::new();

    // Bundled fake pet — HOME is repointed at a fresh tempdir so neither
    // ~/.xangi/pets nor ~/.codex/pets exist; the request should fall
    // through to this dir.
    let bundle = tempfile::tempdir().unwrap();
    let xangi_dir = bundle.path().join("xangi");
    std::fs::create_dir_all(&xangi_dir).unwrap();
    let pet_json = r#"{"id":"xangi","displayName":"xangi","spritesheetPath":"spritesheet.webp"}"#;
    std::fs::write(xangi_dir.join("pet.json"), pet_json).unwrap();

    let state = make_state_with_extra_pet_dirs(vec![bundle.path().to_path_buf()]);
    let addr = spawn_server_with_state(state).await;
    let resp = reqwest::get(format!("http://{addr}/api/pet/asset/xangi/pet.json"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert_eq!(body, pet_json, "served bytes should match bundled file");
}

#[tokio::test]
async fn aggregate_state_derives_from_turn_lifecycle() {
    let (addr, state) = spawn_server().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // turn.started → "thinking"
    http_server::process_event(
        &state,
        json!({"type": "turn.started", "thread_id": "discord:agg", "turn_id": "u-agg-1"}),
    )
    .unwrap();
    let health: Value = client
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["aggregateState"], "thinking");

    // first message.delta → "talking"
    http_server::process_event(
        &state,
        json!({"type": "message.delta", "thread_id": "discord:agg", "turn_id": "u-agg-1", "text": "hi"}),
    )
    .unwrap();
    let health: Value = client
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["aggregateState"], "talking");

    // turn.complete → "idle"
    http_server::process_event(
        &state,
        json!({"type": "turn.complete", "thread_id": "discord:agg", "turn_id": "u-agg-1", "text": "hi"}),
    )
    .unwrap();
    let health: Value = client
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["aggregateState"], "idle");
}
