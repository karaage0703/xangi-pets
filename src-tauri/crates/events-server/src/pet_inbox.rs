// Client for `POST /api/pet/inbox` on the upstream xangi instance.
//
// The pet UI lets the user type a single line and send it to "their" xangi.
// This is the write-side counterpart to the read-side SSE pull client
// (`sse_client.rs`): both target the same xangi web-chat HTTP server, and
// the response to a successful POST flows back as `turn.started` /
// `message.delta` / `turn.complete` events on the existing SSE stream.
//
// Kept in this crate (rather than `src-tauri/src/lib.rs`) so the unit tests
// for it don't need to drag in the Tauri / webview build deps. The Tauri
// command in lib.rs just wires it up to the user-facing `send_pet_message`
// invocation.

use std::time::Duration;

use serde::{Deserialize, Serialize};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize)]
struct PetInboxRequest<'a> {
    text: &'a str,
    #[serde(rename = "appSessionId")]
    app_session_id: &'a str,
}

#[derive(Debug, Serialize)]
struct CreateSessionRequest {}

#[derive(Debug, Deserialize)]
struct CreateSessionResponse {
    #[serde(rename = "sessionId")]
    session_id: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PetInboxResponse {
    pub accepted: bool,
    pub instance_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PetInboxError {
    #[error("xangi URL is empty — set it with the `x` key or set_xangi_url first")]
    EmptyUrl,
    #[error("text is empty")]
    EmptyText,
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("xangi rejected the request (status {status}): {body}")]
    Rejected { status: u16, body: String },
    #[error("could not parse xangi response: {0}")]
    Parse(String),
}

fn client() -> Result<reqwest::Client, PetInboxError> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| PetInboxError::Request(e.to_string()))
}

/// Create a dedicated xangi Web session for this xangi-pets process.
///
/// xangi's pet inbox keeps backward compatibility for older senders by
/// reusing the most recently updated Web session when `appSessionId` is
/// omitted. xangi-pets must not take that fallback: it would append pet
/// messages to an unrelated browser/device session. The Tauri layer calls
/// this lazily for the first pet message and keeps the returned ID for the
/// lifetime of the app process.
pub async fn create_pet_session(
    base_url: &str,
    token: Option<&str>,
) -> Result<String, PetInboxError> {
    let trimmed_url = base_url.trim().trim_end_matches('/');
    if trimmed_url.is_empty() {
        return Err(PetInboxError::EmptyUrl);
    }
    let url = format!("{trimmed_url}/api/sessions");
    let client = client()?;
    let mut req = client.post(&url).json(&CreateSessionRequest {});
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| PetInboxError::Request(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(PetInboxError::Rejected {
            status: status.as_u16(),
            body,
        });
    }
    let parsed: CreateSessionResponse = resp
        .json()
        .await
        .map_err(|e| PetInboxError::Parse(e.to_string()))?;
    if parsed.session_id.trim().is_empty() {
        return Err(PetInboxError::Parse(
            "xangi returned an empty sessionId".into(),
        ));
    }
    Ok(parsed.session_id)
}

/// Send a single text line to `POST /api/pet/inbox` on the upstream xangi.
///
/// `base_url` is the xangi web-chat base (e.g. `http://localhost:18888`).
/// Trailing `/api/pet/inbox` is appended here, the caller should pass the
/// same URL it gave to `set_xangi_url`.
///
/// `token` is the optional `XANGI_PET_INBOX_TOKEN`. When set, it's sent as
/// `Authorization: Bearer <token>`. When `None`, the request is unauthenticated
/// and xangi will only accept it from loopback by default.
///
/// Returns the parsed 202 body (`{accepted, instance_id, thread_id, ...}`).
/// The agent response itself arrives later via the SSE pull stream.
pub async fn post_pet_message(
    base_url: &str,
    text: &str,
    token: Option<&str>,
    app_session_id: &str,
) -> Result<PetInboxResponse, PetInboxError> {
    let trimmed_url = base_url.trim().trim_end_matches('/');
    if trimmed_url.is_empty() {
        return Err(PetInboxError::EmptyUrl);
    }
    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return Err(PetInboxError::EmptyText);
    }
    let trimmed_session_id = app_session_id.trim();
    if trimmed_session_id.is_empty() {
        return Err(PetInboxError::Parse(
            "appSessionId is empty; create a dedicated session first".into(),
        ));
    }
    let url = format!("{trimmed_url}/api/pet/inbox");

    let client = client()?;

    let mut req = client.post(&url).json(&PetInboxRequest {
        text: trimmed_text,
        app_session_id: trimmed_session_id,
    });
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| PetInboxError::Request(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(PetInboxError::Rejected {
            status: status.as_u16(),
            body,
        });
    }
    let parsed: PetInboxResponse = resp
        .json()
        .await
        .map_err(|e| PetInboxError::Parse(e.to_string()))?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct CapturedRequests {
        inbox_bodies: Arc<Mutex<Vec<Value>>>,
    }

    async fn create_session_handler() -> Json<Value> {
        Json(json!({"ok": true, "sessionId": "pet-session-1"}))
    }

    async fn inbox_handler(
        State(captured): State<CapturedRequests>,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        captured.inbox_bodies.lock().unwrap().push(body);
        (
            StatusCode::ACCEPTED,
            Json(json!({
                "accepted": true,
                "instance_id": "pet",
                "thread_id": "web:pet-session-1",
                "turn_id": "turn-1",
                "session_id": "pet-session-1"
            })),
        )
    }

    async fn spawn_xangi_stub() -> (String, CapturedRequests) {
        let captured = CapturedRequests::default();
        let app = Router::new()
            .route("/api/sessions", post(create_session_handler))
            .route("/api/pet/inbox", post(inbox_handler))
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (base_url, captured)
    }

    #[tokio::test]
    async fn empty_url_returns_error() {
        let err = post_pet_message("", "hello", None, "session-1")
            .await
            .unwrap_err();
        assert!(matches!(err, PetInboxError::EmptyUrl));
    }

    #[tokio::test]
    async fn empty_text_returns_error() {
        let err = post_pet_message("http://localhost:1", "   ", None, "session-1")
            .await
            .unwrap_err();
        assert!(matches!(err, PetInboxError::EmptyText));
    }

    #[test]
    fn pet_message_always_serializes_its_session_id() {
        let body = serde_json::to_value(PetInboxRequest {
            text: "hello",
            app_session_id: "pet-session",
        })
        .unwrap();
        assert_eq!(body["text"], "hello");
        assert_eq!(body["appSessionId"], "pet-session");
    }

    #[tokio::test]
    async fn creates_a_dedicated_session_before_posting_pet_message() {
        let (base_url, captured) = spawn_xangi_stub().await;
        let session_id = create_pet_session(&base_url, None).await.unwrap();
        assert_eq!(session_id, "pet-session-1");

        let response = post_pet_message(&base_url, " hello ", None, &session_id)
            .await
            .unwrap();
        assert!(response.accepted);
        assert_eq!(response.session_id.as_deref(), Some("pet-session-1"));

        let bodies = captured.inbox_bodies.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0]["text"], "hello");
        assert_eq!(bodies[0]["appSessionId"], "pet-session-1");
    }
}
