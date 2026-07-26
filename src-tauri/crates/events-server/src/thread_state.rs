// Per-thread state aggregator (Rust port of server/lib/threadState.js).
//
// Translates the raw event stream into:
//   - per-thread state: idle | thinking | talking | error
//   - bubble lifecycle (open / delta / close / error) the pet UI consumes

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::events::now_ms;

#[derive(Debug, Clone)]
pub struct Bubble {
    pub turn_id: String,
    pub text: String,
    pub started_at: u64,
    pub last_delta_at: u64,
}

#[derive(Debug, Clone)]
pub struct Thread {
    pub thread_id: String,
    pub state: String, // "idle" | "thinking" | "talking" | "error"
    /// Human-readable label for the thread (e.g. Discord channel name like
    /// "#general"). Optional — falls back to a shortened thread_id
    /// in the UI when missing. Updated whenever a publisher sends one
    /// alongside an event, so renames eventually catch up.
    pub label: Option<String>,
    pub bubble: Option<Bubble>,
}

#[derive(Default)]
pub struct ThreadStore {
    inner: Mutex<HashMap<String, Thread>>,
}

impl ThreadStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Vec<Value> {
        let map = self.inner.lock().unwrap();
        map.values()
            .map(|t| {
                let bubble = t.bubble.as_ref().map(|b| {
                    json!({
                        "turn_id": b.turn_id,
                        "text": b.text,
                        "started_at": b.started_at,
                        "last_delta_at": b.last_delta_at,
                    })
                });
                json!({
                    "thread_id": t.thread_id,
                    "thread_label": t.label,
                    "state": t.state,
                    "bubble": bubble,
                })
            })
            .collect()
    }

    pub fn aggregate_state(&self) -> &'static str {
        let map = self.inner.lock().unwrap();
        let mut saw_error = false;
        let mut saw_talking = false;
        let mut saw_thinking = false;
        for t in map.values() {
            match t.state.as_str() {
                "error" => saw_error = true,
                "talking" => saw_talking = true,
                "thinking" => saw_thinking = true,
                _ => {}
            }
        }
        if saw_error {
            "error"
        } else if saw_talking {
            "talking"
        } else if saw_thinking {
            "thinking"
        } else {
            "idle"
        }
    }

    /// Apply a stamped event to the store and return the bubble events to broadcast.
    pub fn apply(&self, e: &Value) -> Vec<Value> {
        let ty = e.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let thread_id = e
            .get("thread_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let turn_id = e
            .get("turn_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let ts = e.get("ts").and_then(|v| v.as_u64()).unwrap_or_else(now_ms);
        let incoming_label = e
            .get("thread_label")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut map = self.inner.lock().unwrap();
        let t = map.entry(thread_id.clone()).or_insert_with(|| Thread {
            thread_id: thread_id.clone(),
            state: "idle".to_string(),
            label: None,
            bubble: None,
        });
        // Update the label whenever the publisher sends one. Channel renames
        // (or first-time discovery) catch up on the next event.
        if let Some(label) = incoming_label {
            t.label = Some(label);
        }
        let label_payload = t.label.clone();

        // Schema v2: the publisher only sends the 5 operational events
        // turn.started / message.delta / turn.complete / turn.aborted /
        // agent.error. State labels (thinking / talking / idle) are derived
        // here, not transmitted on the wire.
        let mut out: Vec<Value> = Vec::new();
        match ty {
            "turn.started" => {
                t.state = "thinking".to_string();
                t.bubble = Some(Bubble {
                    turn_id: turn_id.clone(),
                    text: String::new(),
                    started_at: ts,
                    last_delta_at: ts,
                });
                out.push(json!({
                    "type": "bubble.open",
                    "thread_id": thread_id,
                    "thread_label": label_payload,
                    "turn_id": turn_id,
                    "ts": ts,
                }));
            }
            "message.delta" => {
                let text = e.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let needs_open = match &t.bubble {
                    Some(b) => b.turn_id != turn_id,
                    None => true,
                };
                if needs_open {
                    t.bubble = Some(Bubble {
                        turn_id: turn_id.clone(),
                        text: String::new(),
                        started_at: ts,
                        last_delta_at: ts,
                    });
                    out.push(json!({
                        "type": "bubble.open",
                        "thread_id": thread_id,
                        "thread_label": label_payload,
                        "turn_id": turn_id,
                        "ts": ts,
                    }));
                }
                if let Some(b) = t.bubble.as_mut() {
                    b.text.push_str(text);
                    b.last_delta_at = ts;
                }
                // First delta moves us from thinking → talking.
                t.state = "talking".to_string();
                out.push(json!({
                    "type": "bubble.delta",
                    "thread_id": thread_id,
                    "thread_label": label_payload,
                    "turn_id": turn_id,
                    "text": text,
                    "ts": ts,
                }));
            }
            "turn.complete" => {
                // v2: final text is in `text`. We still accept `last_message`
                // from older publishers as a courtesy.
                let final_text = e
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        e.get("last_message")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .or_else(|| t.bubble.as_ref().map(|b| b.text.clone()))
                    .unwrap_or_default();
                out.push(json!({
                    "type": "bubble.close",
                    "thread_id": thread_id,
                    "thread_label": label_payload,
                    "turn_id": turn_id,
                    "last_message": final_text,
                    "ts": ts,
                }));
                t.bubble = None;
                t.state = "idle".to_string();
            }
            "turn.aborted" => {
                // User cancelled the turn. Close the bubble using whatever
                // partial text we'd accumulated so the user can still see
                // what was said before the cancel.
                let partial = t
                    .bubble
                    .as_ref()
                    .map(|b| b.text.clone())
                    .unwrap_or_default();
                out.push(json!({
                    "type": "bubble.close",
                    "thread_id": thread_id,
                    "thread_label": label_payload,
                    "turn_id": turn_id,
                    "last_message": partial,
                    "aborted": true,
                    "ts": ts,
                }));
                t.bubble = None;
                t.state = "idle".to_string();
            }
            "agent.error" => {
                let msg = e.get("message").and_then(|v| v.as_str()).unwrap_or("");
                t.state = "error".to_string();
                out.push(json!({
                    "type": "bubble.error",
                    "thread_id": thread_id,
                    "thread_label": label_payload,
                    "turn_id": turn_id,
                    "message": msg,
                    "ts": ts,
                }));
            }
            _ => {}
        }
        out
    }
}
