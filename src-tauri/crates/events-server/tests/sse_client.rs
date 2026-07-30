// Integration test for the pull-mode SSE client.
//
// Spins up a fake xangi-side SSE endpoint, points spawn_pull_client at it,
// and verifies that:
//   - the `event: ready` prelude is silently skipped (not pushed to the bus)
//   - subsequent `data:` events flow through `process_event` and end up on
//     the bubble bus (i.e. xangi's external event wire format is parsed
//     and turned into the same derived bubbles as the legacy POST /events)
//   - non-JSON `data:` frames are logged-and-skipped, not fatal
//
// Each assertion uses a fresh AppState + per-test ephemeral port to keep
// the integration parallelism-safe.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    response::sse::{Event as SseEvent, KeepAlive, Sse},
    routing::get,
    Router,
};
use futures::stream::{self, Stream, StreamExt};
use serde_json::json;
use xangi_events_server::{
    make_state, spawn_pull_client, spawn_pull_client_with_callbacks, PullConnectionState,
};

#[derive(Clone)]
struct SseFrame {
    event_type: Option<String>,
    data: String,
}

impl SseFrame {
    fn into_event(self) -> SseEvent {
        let mut ev = SseEvent::default();
        if let Some(t) = self.event_type {
            ev = ev.event(t);
        }
        ev.data(self.data)
    }
}

/// Stand up a minimal `/api/events/stream` endpoint that emits the given
/// frames, then keeps the connection open (so the client doesn't immediately
/// reconnect and race the bus assertion).
async fn spawn_fake_xangi(frames: Vec<SseFrame>) -> SocketAddr {
    let frames = Arc::new(frames);
    let app = Router::new().route(
        "/api/events/stream",
        get({
            let frames = frames.clone();
            move || {
                // Clone the Vec into the request handler so we own a 'static
                // owned iterator (axum's Sse expects the stream to be 'static).
                let owned: Vec<SseFrame> = (*frames).clone();
                async move {
                    let stream: std::pin::Pin<
                        Box<dyn Stream<Item = Result<SseEvent, Infallible>> + Send>,
                    > = Box::pin(
                        stream::iter(owned.into_iter())
                            .then(|f| async move {
                                tokio::time::sleep(Duration::from_millis(10)).await;
                                Ok::<_, Infallible>(f.into_event())
                            })
                            .chain(stream::pending::<Result<SseEvent, Infallible>>()),
                    );
                    Sse::new(stream).keep_alive(KeepAlive::default())
                }
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

#[tokio::test]
async fn pull_client_skips_ready_prelude_and_forwards_events() {
    let frames = vec![
        SseFrame {
            event_type: Some("ready".into()),
            data: r#"{"instance_id":"xangi-test","host_hint":"unit"}"#.into(),
        },
        SseFrame {
            event_type: None,
            data: serde_json::to_string(&json!({
                "type": "turn.started",
                "thread_id": "discord:42",
                "turn_id": "t1",
                "user_text": "ping",
            }))
            .unwrap(),
        },
        SseFrame {
            event_type: None,
            data: serde_json::to_string(&json!({
                "type": "message.delta",
                "thread_id": "discord:42",
                "turn_id": "t1",
                "text": "pong",
                "full_text": "pong",
            }))
            .unwrap(),
        },
        SseFrame {
            event_type: None,
            data: serde_json::to_string(&json!({
                "type": "turn.complete",
                "thread_id": "discord:42",
                "turn_id": "t1",
                "text": "pong",
            }))
            .unwrap(),
        },
    ];
    let addr = spawn_fake_xangi(frames).await;

    let state = make_state();
    let mut bubble_rx = state.bubble_bus.subscribe();
    let url = format!("http://{addr}/api/events/stream");

    let handle = spawn_pull_client(state, url);

    let mut got_open_or_delta = false;
    let mut got_close = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), bubble_rx.recv()).await {
            Ok(Ok(v)) => {
                let ty = v.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if ty == "bubble.open" || ty == "bubble.delta" {
                    got_open_or_delta = true;
                }
                if ty == "bubble.close" {
                    got_close = true;
                }
                if got_open_or_delta && got_close {
                    break;
                }
            }
            Ok(Err(_)) | Err(_) => continue,
        }
    }
    handle.shutdown().await;

    assert!(
        got_open_or_delta,
        "expected bubble.open/delta to be derived from message.delta",
    );
    assert!(
        got_close,
        "expected bubble.close to be derived from turn.complete",
    );
}

#[tokio::test]
async fn pull_client_skips_invalid_json_frames() {
    let frames = vec![
        SseFrame {
            event_type: Some("ready".into()),
            data: r#"{"instance_id":"xangi-test","host_hint":"unit"}"#.into(),
        },
        SseFrame {
            event_type: None,
            data: "this-is-not-json".into(),
        },
        SseFrame {
            event_type: None,
            data: serde_json::to_string(&json!({
                "type": "turn.started",
                "thread_id": "web:s1",
                "turn_id": "u1",
            }))
            .unwrap(),
        },
    ];
    let addr = spawn_fake_xangi(frames).await;

    let state = make_state();
    // Listen on bubble_bus for the derived bubble.open: a turn.started event
    // forwarded by the pull client → process_event → bubble_bus.send.
    let mut bubble_rx = state.bubble_bus.subscribe();
    let url = format!("http://{addr}/api/events/stream");
    let handle = spawn_pull_client(state, url);

    let mut saw_valid = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), bubble_rx.recv()).await {
            Ok(Ok(v)) => {
                if v.get("type").and_then(|v| v.as_str()) == Some("bubble.open") {
                    saw_valid = true;
                    break;
                }
            }
            Ok(Err(_)) | Err(_) => continue,
        }
    }
    handle.shutdown().await;

    assert!(
        saw_valid,
        "expected the valid frame to still produce a bubble.open despite the bad one",
    );
}

#[tokio::test]
async fn pull_client_reports_handshake_and_only_accepted_events() {
    let frames = vec![
        SseFrame {
            event_type: Some("ready".into()),
            data: r#"{"instance_id":"xangi-test","host_hint":"unit"}"#.into(),
        },
        SseFrame {
            event_type: None,
            data: "not-json".into(),
        },
        SseFrame {
            event_type: None,
            data: serde_json::to_string(&json!({
                "type": "turn.started",
                "thread_id": "discord:42",
                "turn_id": "callback-1",
            }))
            .unwrap(),
        },
    ];
    let addr = spawn_fake_xangi(frames).await;
    let states = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));

    let states_for_callback = states.clone();
    let events_for_callback = events.clone();
    let handle = spawn_pull_client_with_callbacks(
        make_state(),
        format!("http://{addr}/api/events/stream"),
        Arc::new(move |state| states_for_callback.lock().unwrap().push(state)),
        Arc::new(move |event| events_for_callback.lock().unwrap().push(event.clone())),
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let connected = states
            .lock()
            .unwrap()
            .contains(&PullConnectionState::Connected);
        let received = events.lock().unwrap().len() == 1;
        if connected && received {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for connection and event callbacks"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    handle.shutdown().await;

    let observed_states = states.lock().unwrap();
    assert_eq!(
        observed_states.first(),
        Some(&PullConnectionState::Connecting)
    );
    assert!(observed_states.contains(&PullConnectionState::Connected));

    let observed_events = events.lock().unwrap();
    assert_eq!(observed_events.len(), 1, "ready and invalid JSON are skipped");
    assert_eq!(
        observed_events[0].get("turn_id").and_then(|value| value.as_str()),
        Some("callback-1")
    );
}
