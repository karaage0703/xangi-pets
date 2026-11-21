// Embedded HTTP server: replaces the standalone Node sidecar in server/.
//
// Endpoints exposed by the pet's local server:
//
//   GET  /api/pet/bubbles  SSE: aggregated bubble.* lifecycle for the pet UI
//   GET  /api/pet/state    SSE: derived single-state ("idle"/"thinking"/
//                          "talking"/"error") — pure-derivation from the
//                          thread store.
//   GET  /api/pet/list     list pet sprite directories under XANGI_PET_DIR
//   GET  /api/pet/asset/:name/:file
//                          serve sprites from $XANGI_PET_DIR (default ~/.codex/pets)
//   GET  /health           diag
//
// Events flow into the bus via `process_event`, called from the pull-mode
// SSE client (`sse_client.rs`) which subscribes to xangi's
// `GET /api/events/stream`. The previous push-mode `POST /events` /
// `GET /events` raw event stream were removed once the pull path was
// the only consumer (see notes/* for the migration).

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{sse::Event as SseEvent, IntoResponse, Sse},
    routing::get,
    Json, Router,
};
use futures::stream::Stream;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::wrappers::{BroadcastStream, IntervalStream};
use tokio_stream::StreamExt;
use tower_http::cors::{Any, CorsLayer};

use crate::events::{stamp, validate};
use crate::thread_state::ThreadStore;

const CHANNEL_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<ThreadStore>,
    pub bubble_bus: broadcast::Sender<Value>,
    pub state_bus: broadcast::Sender<Value>,
    /// Search path for pet sprites. Earlier entries take priority. Defaults
    /// to `~/.xangi/pets/` first, then `~/.codex/pets/` as fallback so a
    /// user who already has Codex hatch-pet sprites can reuse them without
    /// copying anywhere new.
    pub pet_dirs: Arc<Vec<PathBuf>>,
}

impl AppState {
    /// Backwards-compat accessor that returns the primary (highest priority)
    /// pet directory. Used by health/diag output.
    pub fn primary_pet_dir(&self) -> &PathBuf {
        self.pet_dirs
            .first()
            .expect("pet_dirs is never empty by construction")
    }
}

pub fn pet_dirs() -> Vec<PathBuf> {
    pet_dirs_with_extra(&[])
}

/// Like `pet_dirs()` but appends `extra` after the standard locations.
/// `XANGI_PET_DIR` still wins exclusively when set, so a user pointing at a
/// custom dir doesn't have the bundled fallback silently injected. Used by
/// the Tauri host to surface the app's bundled default pet (`xangi`) as a
/// last-resort source so first-run users see something instead of the
/// "no sprites configured" hint.
pub fn pet_dirs_with_extra(extra: &[PathBuf]) -> Vec<PathBuf> {
    if let Ok(v) = std::env::var("XANGI_PET_DIR") {
        // explicit override wins; only this dir is searched
        return vec![PathBuf::from(v)];
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let h = PathBuf::from(home);
    let mut dirs = vec![
        h.join(".xangi").join("pets"),
        h.join(".codex").join("pets"),
    ];
    for d in extra {
        if !dirs.contains(d) {
            dirs.push(d.clone());
        }
    }
    dirs
}

/// Bind a TCP listener with automatic port-shift on AddrInUse. Useful for
/// running multiple xangi-pets instances side-by-side (each gets its own
/// port, picking sequentially). Returns the listener so the caller can read
/// the actual bound `local_addr()` for display / Tauri command response.
pub async fn bind_with_autoshift(
    addr: SocketAddr,
    tries: u16,
) -> std::io::Result<tokio::net::TcpListener> {
    let mut current = addr;
    let limit = tries.max(1);
    for _ in 0..limit {
        match tokio::net::TcpListener::bind(current).await {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                let next = current.port().wrapping_add(1);
                if next == 0 {
                    return Err(e);
                }
                current.set_port(next);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        format!(
            "no free port in range {}..{}",
            addr.port(),
            current.port()
        ),
    ))
}

/// Run the HTTP server on an already-bound listener.
pub async fn serve_listener(
    listener: tokio::net::TcpListener,
    state: AppState,
) -> std::io::Result<()> {
    let app = build_router(state);
    axum::serve(listener, app).await
}

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health))
        .route("/api/pet/bubbles", get(get_bubbles_sse))
        .route("/api/pet/state", get(get_state_sse))
        .route("/api/pet/list", get(list_pets))
        .route("/api/pet/asset/:name/:file", get(get_asset))
        .with_state(state)
        .layer(cors)
}

pub async fn serve(addr: SocketAddr, state: AppState) -> std::io::Result<()> {
    // Default tries = 10. Callers that want fine control should use
    // bind_with_autoshift + serve_listener directly to read the bound port.
    let listener = bind_with_autoshift(addr, 10).await?;
    if let Ok(bound) = listener.local_addr() {
        if bound != addr {
            eprintln!(
                "xangi-events: requested {} was in use, bound {} instead",
                addr, bound
            );
        }
    }
    serve_listener(listener, state).await
}

// ---------- handlers ----------

async fn health(State(s): State<AppState>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "petDir": s.primary_pet_dir().display().to_string(),
        "petDirs": s.pet_dirs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "threads": s.store.snapshot(),
        "aggregateState": s.store.aggregate_state(),
        "subscribers": {
            "bubbles": s.bubble_bus.receiver_count(),
            "state": s.state_bus.receiver_count(),
        }
    }))
}

/// Process a single event from xangi into the local bubble + state buses.
/// Called only by the pull-mode SSE client (`sse_client.rs`), which
/// subscribes to xangi's `GET /api/events/stream`.
///
/// Returns `Ok(())` on accept, `Err(reason)` on validation failure.
pub fn process_event(s: &AppState, raw: Value) -> Result<(), String> {
    validate(&raw)?;
    let stamped = stamp(raw);

    // Update per-thread state, broadcast derived bubble events.
    for derived in s.store.apply(&stamped) {
        let _ = s.bubble_bus.send(derived);
    }

    // Aggregate single-state for the pet sprite row.
    let _ = s.state_bus.send(json!({
        "state": s.store.aggregate_state(),
        "thread_id": stamped.get("thread_id"),
        "ts": stamped.get("ts"),
    }));

    Ok(())
}

fn sse_from_rx(
    rx: broadcast::Receiver<Value>,
    initial: Vec<Value>,
    filter: impl Fn(&Value) -> bool + Send + Sync + 'static,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let initial_stream = futures::stream::iter(initial.into_iter().map(|v| {
        Ok::<_, Infallible>(SseEvent::default().data(serde_json::to_string(&v).unwrap_or_default()))
    }));
    let live = BroadcastStream::new(rx).filter_map(move |res| {
        let v = res.ok()?;
        if !filter(&v) {
            return None;
        }
        Some(Ok::<_, Infallible>(
            SseEvent::default().data(serde_json::to_string(&v).unwrap_or_default()),
        ))
    });
    let stream = initial_stream.chain(live);
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(25))
            .text("ping"),
    )
}

async fn get_bubbles_sse(
    State(s): State<AppState>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = s.bubble_bus.subscribe();

    // Send open bubbles as snapshot so a fresh client catches up.
    let mut snapshot: Vec<Value> = Vec::new();
    for thread in s.store.snapshot() {
        if let Some(bubble) = thread.get("bubble") {
            if !bubble.is_null() {
                snapshot.push(json!({
                    "type": "bubble.snapshot",
                    "thread_id": thread.get("thread_id"),
                    "thread_label": thread.get("thread_label"),
                    "turn_id": bubble.get("turn_id"),
                    "text": bubble.get("text"),
                    "ts": bubble.get("last_delta_at"),
                }));
            }
        }
    }

    sse_from_rx(rx, snapshot, |_| true)
}

async fn get_state_sse(
    State(s): State<AppState>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let stream = state_updates(s).map(|value| {
        Ok(SseEvent::default().data(serde_json::to_string(&value).unwrap_or_default()))
    });
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(25))
            .text("ping"),
    )
}

fn state_updates(s: AppState) -> impl Stream<Item = Value> {
    let events = BroadcastStream::new(s.state_bus.subscribe()).map(|_| ());
    // The first tick supplies the snapshot. Subsequent ticks let the error
    // expression expire even when upstream is silent. Dropping the SSE stream
    // also drops its timer; no background task survives a disconnected client.
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let ticks = IntervalStream::new(interval).map(|_| ());
    let mut previous = None;
    events.merge(ticks).filter_map(move |_| {
        // Recompute rather than replay queued states, including after lag.
        let state = s.store.aggregate_state();
        if previous == Some(state) {
            return None;
        }
        previous = Some(state);
        Some(json!({ "state": state, "ts": crate::events::now_ms() }))
    })
}

async fn list_pets(State(s): State<AppState>) -> Json<Value> {
    // De-duplicate pet names that show up under multiple search paths
    // (e.g. the user has the same name in both ~/.xangi/pets and ~/.codex/pets).
    let mut seen = std::collections::BTreeSet::<String>::new();
    for dir in s.pet_dirs.iter() {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        seen.insert(name.to_string());
                    }
                }
            }
        }
    }
    Json(json!({ "pets": seen.into_iter().collect::<Vec<_>>() }))
}

async fn get_asset(
    State(s): State<AppState>,
    Path((name, file)): Path<(String, String)>,
) -> impl IntoResponse {
    fn safe(s: &str, allow_dot: bool) -> bool {
        if s.is_empty() {
            return false;
        }
        s.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '_' || c == '-' || (allow_dot && c == '.')
        })
    }
    if !safe(&name, false) || !safe(&file, true) {
        return (StatusCode::BAD_REQUEST, "invalid path").into_response();
    }

    // Try each pet_dir in priority order. First hit wins, so users can
    // override individual sprites by placing them in ~/.xangi/pets/ even if
    // ~/.codex/pets/ also has the same name.
    for dir in s.pet_dirs.iter() {
        let canonical_root = match dir.canonicalize() {
            Ok(p) => p,
            Err(_) => continue, // dir doesn't exist on disk yet, try next
        };
        let target = dir.join(&name).join(&file);
        let canonical_target = match target.canonicalize() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if !canonical_target.starts_with(&canonical_root) {
            return (StatusCode::BAD_REQUEST, "path traversal").into_response();
        }
        match tokio::fs::read(&canonical_target).await {
            Ok(bytes) => {
                let mime = mime_guess::from_path(&canonical_target)
                    .first_or_octet_stream()
                    .to_string();
                let mut headers = HeaderMap::new();
                headers.insert(
                    header::CONTENT_TYPE,
                    mime.parse().unwrap_or(header::HeaderValue::from_static(
                        "application/octet-stream",
                    )),
                );
                return (StatusCode::OK, headers, Body::from(bytes)).into_response();
            }
            Err(_) => continue,
        }
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}

pub fn make_state() -> AppState {
    make_state_with_extra_pet_dirs(vec![])
}

/// Variant of `make_state()` that lets the caller (Tauri host) inject extra
/// pet search dirs after the default `~/.xangi/pets` / `~/.codex/pets` pair.
/// The extras are appended at lowest priority so user-managed sprites always
/// win over the bundled default.
pub fn make_state_with_extra_pet_dirs(extra: Vec<PathBuf>) -> AppState {
    let (bubble_bus, _) = broadcast::channel::<Value>(CHANNEL_CAPACITY);
    let (state_bus, _) = broadcast::channel::<Value>(CHANNEL_CAPACITY);
    AppState {
        store: Arc::new(ThreadStore::new()),
        bubble_bus,
        state_bus,
        pet_dirs: Arc::new(pet_dirs_with_extra(&extra)),
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;
    use futures::FutureExt;

    #[tokio::test(start_paused = true)]
    async fn state_stream_recovers_without_another_upstream_event() {
        let state = make_state();
        process_event(
            &state,
            json!({
                "type": "agent.error", "thread_id": "failed", "turn_id": "turn",
                "message": "failure",
            }),
        )
        .unwrap();
        let mut stream = Box::pin(state_updates(state.clone()));
        assert_eq!(stream.next().await.unwrap()["state"], "error");
        tokio::time::advance(Duration::from_secs(29)).await;
        assert!(
            stream.next().now_or_never().is_none(),
            "unchanged states are not resent"
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(stream.next().await.unwrap()["state"], "idle");
        // Reconnecting must not restart the expression deadline.
        let mut reconnected = Box::pin(state_updates(state.clone()));
        assert_eq!(reconnected.next().await.unwrap()["state"], "idle");
        drop(stream);
        drop(reconnected);
        assert_eq!(state.state_bus.receiver_count(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn live_activity_interrupts_error_and_expiry_keeps_activity() {
        let state = make_state();
        let mut stream = Box::pin(state_updates(state.clone()));
        assert_eq!(stream.next().await.unwrap()["state"], "idle");
        for (ty, thread, expected) in [
            ("agent.error", "failed", "error"),
            ("turn.started", "working", "thinking"),
            ("message.delta", "working", "talking"),
        ] {
            process_event(
                &state,
                json!({
                    "type": ty, "thread_id": thread, "turn_id": "turn",
                    "message": "failure", "text": "hello",
                }),
            )
            .unwrap();
            assert_eq!(stream.next().await.unwrap()["state"], expected);
        }
        tokio::time::advance(Duration::from_secs(31)).await;
        assert!(stream.next().now_or_never().is_none());
        process_event(
            &state,
            json!({
                "type": "turn.complete", "thread_id": "working", "turn_id": "turn",
            }),
        )
        .unwrap();
        assert_eq!(stream.next().await.unwrap()["state"], "idle");
    }
}
