// Event schema validation + stamping.
//
// Wire format documented in docs/EVENTS.md and emitted by xangi.
// Every accepted event is re-serialized
// as `serde_json::Value` so SSE subscribers see exactly what was POSTed,
// plus a server-stamped `recv_ts`.

use serde_json::{json, Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// Event type strings the server recognises. Anything else is rejected.
///
/// Schema v2: operational events only. The
/// previous `agent.thinking` / `agent.talking` / `agent.idle` state labels
/// have been removed — consumers derive lifecycle state from turn.* +
/// message.delta. `turn.aborted` is the new cancel signal.
pub const KNOWN_TYPES: &[&str] = &[
    "turn.started",
    "message.delta",
    "turn.complete",
    "turn.aborted",
    "agent.error",
];

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Validate a single incoming event. Returns Err(message) on rejection.
pub fn validate(raw: &Value) -> Result<(), String> {
    let obj = raw.as_object().ok_or("event must be a JSON object")?;
    let ty = obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or("type is required")?;
    if !KNOWN_TYPES.contains(&ty) {
        return Err(format!("unknown event type: {ty}"));
    }
    let thread_id = obj
        .get("thread_id")
        .and_then(|v| v.as_str())
        .ok_or("thread_id is required")?;
    if thread_id.is_empty() {
        return Err("thread_id must not be empty".into());
    }

    // Every event in schema v2 carries a turn_id.
    let turn_id = obj
        .get("turn_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("turn_id is required for {ty}"))?;
    if turn_id.is_empty() {
        return Err("turn_id must not be empty".into());
    }

    // Type-specific required extras.
    match ty {
        "message.delta" => {
            let text = obj.get("text").and_then(|v| v.as_str());
            if text.is_none() {
                return Err("message.delta requires text".into());
            }
        }
        "agent.error" => {
            let msg = obj.get("message").and_then(|v| v.as_str());
            if msg.is_none() {
                return Err("agent.error requires message".into());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Stamp the event with `ts` (if missing) and always-set `recv_ts`.
pub fn stamp(mut raw: Value) -> Value {
    let now = now_ms();
    if let Some(obj) = raw.as_object_mut() {
        obj.entry("ts".to_string()).or_insert(json!(now));
        obj.insert("recv_ts".to_string(), json!(now));
    }
    raw
}

/// Helper to build an event object inline (used by snapshot / synthesis).
pub fn build(map: Map<String, Value>) -> Value {
    Value::Object(map)
}
