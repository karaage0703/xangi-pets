// Pull-mode SSE client.
//
// Subscribes to a remote xangi instance's `GET /api/events/stream` and feeds
// every received event into our local bus by calling `process_event`. This
// replaces the old push model (xangi POSTed to our `POST /events`) — now the
// pet pulls from xangi instead, so:
//
// - xangi-side configuration is fixed (one URL, one port, no `XANGI_EVENTS_URLS`)
// - the pet works without opening any inbound port
// - multiple pets can subscribe to the same xangi without xangi-side changes
//
// xangi external event-stream wire format:
// - first frame is `event: ready` with `{instance_id, host_hint}` payload — we
//   ignore it (informational only)
// - subsequent frames are unnamed `data: { type, thread_id, ... }` events
// - 30s `: keepalive` comments are sent through transparently by EventSource

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::stream::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use serde_json::Value;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::http_server::{process_event, AppState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PullConnectionState {
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
}

pub type ConnectionCallback = Arc<dyn Fn(PullConnectionState) + Send + Sync>;
pub type EventCallback = Arc<dyn Fn(&Value) + Send + Sync>;

/// Initial reconnect delay when the upstream stream errors out.
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
/// Max reconnect delay (exponential backoff capped here).
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Handle to a running pull client. Drop or call `shutdown()` to stop it.
pub struct PullClientHandle {
    cancel: Arc<Notify>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl PullClientHandle {
    /// Stop the pull client and wait for the task to complete.
    pub async fn shutdown(&self) {
        self.cancel.notify_waiters();
        let task = self.task.lock().ok().and_then(|mut g| g.take());
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}

/// Spawn a background task that subscribes to `xangi_url` (an absolute URL
/// like `http://host:18888/api/events/stream`) and forwards every accepted
/// event into the local bus via `process_event`.
///
/// Reconnects with exponential backoff on stream error / server restart.
pub fn spawn_pull_client(state: AppState, xangi_url: String) -> PullClientHandle {
    spawn_pull_client_with_callbacks(
        state,
        xangi_url,
        Arc::new(|_| {}),
        Arc::new(|_| {}),
    )
}

pub fn spawn_pull_client_with_callbacks(
    state: AppState,
    xangi_url: String,
    on_connection: ConnectionCallback,
    on_event: EventCallback,
) -> PullClientHandle {
    let cancel = Arc::new(Notify::new());
    let cancel_for_task = cancel.clone();
    let task = tokio::spawn(async move {
        run(
            state,
            xangi_url,
            cancel_for_task,
            on_connection,
            on_event,
        )
        .await;
    });
    PullClientHandle {
        cancel,
        task: Mutex::new(Some(task)),
    }
}

async fn run(
    state: AppState,
    xangi_url: String,
    cancel: Arc<Notify>,
    on_connection: ConnectionCallback,
    on_event: EventCallback,
) {
    let mut backoff = INITIAL_BACKOFF;
    on_connection(PullConnectionState::Connecting);
    loop {
        let cancelled = tokio::select! {
            _ = cancel.notified() => true,
            outcome = run_once(
                &state,
                &xangi_url,
                on_connection.clone(),
                on_event.clone(),
            ) => {
                match outcome {
                    Ok(()) => {
                        // Server closed the stream cleanly. Reset backoff and
                        // try again — usually means xangi was restarted.
                        backoff = INITIAL_BACKOFF;
                    }
                    Err(err) => {
                        eprintln!(
                            "xangi-events pull: stream error from {xangi_url}: {err} (retry in {:?})",
                            backoff
                        );
                    }
                }
                false
            }
        };
        if cancelled {
            on_connection(PullConnectionState::Disconnected);
            break;
        }
        on_connection(PullConnectionState::Reconnecting);
        // Wait before reconnecting, but break out early if we get cancelled
        // mid-sleep.
        let cancelled_during_sleep = tokio::select! {
            _ = cancel.notified() => true,
            _ = tokio::time::sleep(backoff) => false,
        };
        if cancelled_during_sleep {
            break;
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn run_once(
    state: &AppState,
    xangi_url: &str,
    on_connection: ConnectionCallback,
    on_event: EventCallback,
) -> Result<(), String> {
    let mut es = EventSource::get(xangi_url);
    while let Some(ev) = es.next().await {
        match ev {
            Ok(Event::Open) => {
                // SSE handshake completed.
                on_connection(PullConnectionState::Connected);
            }
            Ok(Event::Message(msg)) => {
                // xangi sends a single `event: ready` first with
                // {instance_id, host_hint}. Informational only; skip it.
                if msg.event == "ready" {
                    continue;
                }
                let raw: Value = match serde_json::from_str(&msg.data) {
                    Ok(v) => v,
                    Err(err) => {
                        eprintln!(
                            "xangi-events pull: ignoring non-JSON frame from {xangi_url}: {err}"
                        );
                        continue;
                    }
                };
                if let Err(err) = process_event(state, raw.clone()) {
                    // Validation rejection — log and keep going. We don't
                    // want one bad event to kill the whole stream.
                    eprintln!("xangi-events pull: rejected event: {err}");
                } else {
                    on_event(&raw);
                }
            }
            Err(err) => {
                es.close();
                return Err(err.to_string());
            }
        }
    }
    Ok(())
}
