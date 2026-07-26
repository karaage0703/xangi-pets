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
) -> Result<PetInboxResponse, PetInboxError> {
    let trimmed_url = base_url.trim().trim_end_matches('/');
    if trimmed_url.is_empty() {
        return Err(PetInboxError::EmptyUrl);
    }
    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return Err(PetInboxError::EmptyText);
    }
    let url = format!("{trimmed_url}/api/pet/inbox");

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| PetInboxError::Request(e.to_string()))?;

    let mut req = client.post(&url).json(&PetInboxRequest { text: trimmed_text });
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

    #[tokio::test]
    async fn empty_url_returns_error() {
        let err = post_pet_message("", "hello", None).await.unwrap_err();
        assert!(matches!(err, PetInboxError::EmptyUrl));
    }

    #[tokio::test]
    async fn empty_text_returns_error() {
        let err = post_pet_message("http://localhost:1", "   ", None)
            .await
            .unwrap_err();
        assert!(matches!(err, PetInboxError::EmptyText));
    }
}
