//! Agent discovery, workspace assets and routing through existing xangi APIs.
use std::collections::HashMap;
use std::time::Duration;

use crate::session_filters::{source, SessionSource};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::watch;

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}

async fn get_bytes(url: reqwest::Url, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = client()?
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len() + chunk.len() > limit {
            return Err("upstream response too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn api_url(base: &str, path: &str) -> Result<reqwest::Url, String> {
    reqwest::Url::parse(&format!("{}{path}", base.trim_end_matches('/'))).map_err(|e| e.to_string())
}

pub async fn list_agents(base: &str) -> Result<Value, String> {
    let bytes = get_bytes(api_url(base, "/api/agents")?, 2 * 1024 * 1024).await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !value["agents"].is_array() {
        return Err("xangi returned no Agent list".into());
    }
    Ok(value)
}

pub async fn require_agent(base: &str, id: &str) -> Result<Value, String> {
    list_agents(base).await?["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["id"].as_str() == Some(id))
        .cloned()
        .ok_or_else(|| "選択したAgentが見つかりません".into())
}

#[derive(Serialize)]
pub struct AgentPet {
    pub meta: Value,
    pub image: Vec<u8>,
}

pub async fn load_agent_pet(base: &str, id: &str, directory: &str) -> Result<AgentPet, String> {
    // Only workspace-relative directories; xangi validates real-path containment.
    let directory = directory.trim();
    let directory = directory.strip_prefix("./").unwrap_or(directory);
    if directory.is_empty()
        || directory.starts_with('/')
        || directory.contains(['\\', ':'])
        || directory
            .split('/')
            .any(|part| part == ".." || part.is_empty())
    {
        return Err("スプライトにはworkspace内の相対ディレクトリを指定してください".into());
    }
    let agent = require_agent(base, id).await?;
    let workspace = agent["workspaceId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("default");
    let asset_url = |file: &str| -> Result<reqwest::Url, String> {
        let mut url = api_url(base, "/api/workspace-file")?;
        url.query_pairs_mut()
            .append_pair("workspaceId", workspace)
            .append_pair("path", &format!("{directory}/{file}"));
        Ok(url)
    };
    let meta_bytes = get_bytes(asset_url("pet.json")?, 64 * 1024).await?;
    let meta: Value = serde_json::from_slice(&meta_bytes).map_err(|e| e.to_string())?;
    if !meta.is_object() {
        return Err("pet.json must be an object".into());
    }
    if meta["spritesheetPath"]
        .as_str()
        .unwrap_or("spritesheet.webp")
        != "spritesheet.webp"
    {
        return Err("workspaceのspritesheetPathはspritesheet.webpを指定してください".into());
    }
    let image = get_bytes(asset_url("spritesheet.webp")?, 16 * 1024 * 1024).await?;
    if image.len() < 12 || &image[..4] != b"RIFF" || &image[8..12] != b"WEBP" {
        return Err("スプライトがWebP形式ではありません".into());
    }
    Ok(AgentPet { meta, image })
}

/// Older xangi streams carry thread IDs but no Agent ID. Resolve ownership
/// once per turn against live sessions, then pin it through that turn.
pub struct AgentRouter {
    base: String,
    agent: String,
    turns: HashMap<(String, String), (bool, SessionSource)>,
    filters: Option<watch::Receiver<Vec<SessionSource>>>,
}

impl AgentRouter {
    pub fn new(base: String, agent: String) -> Self {
        Self {
            base,
            agent,
            turns: HashMap::new(),
            filters: None,
        }
    }

    pub fn with_filters(
        base: String,
        agent: Option<String>,
        filters: watch::Receiver<Vec<SessionSource>>,
    ) -> Self {
        let mut router = Self::new(base, agent.unwrap_or_default());
        router.filters = Some(filters);
        router
    }

    fn allowed(&self, accepted: bool, source: SessionSource) -> bool {
        accepted
            && self
                .filters
                .as_ref()
                .is_none_or(|filters| filters.borrow().contains(&source))
    }

    pub async fn accepts(&mut self, event: &Value) -> Result<bool, String> {
        if let Some(agent) = event["agent_id"].as_str() {
            if !self.agent.is_empty() && agent != self.agent {
                return Ok(false);
            }
            if self.filters.is_none() {
                return Ok(agent == self.agent);
            }
        }
        let thread = event["thread_id"].as_str().unwrap_or_default();
        let turn = event["turn_id"].as_str().unwrap_or_default();
        let key = (thread.to_string(), turn.to_string());
        if let Some((accepted, source)) = self.turns.get(&key) {
            return Ok(self.allowed(*accepted, *source));
        }
        // Fail closed once per turn, avoiding a network retry per token.
        if self.turns.len() >= 512 {
            self.turns.clear();
        }
        self.turns.insert(key.clone(), (false, SessionSource::Chat));
        let mut offset = 0;
        let (accepted, origin) = loop {
            let mut url = api_url(&self.base, "/api/sessions")?;
            if !self.agent.is_empty() && self.agent != "xangi:default" {
                url.query_pairs_mut().append_pair("agentId", &self.agent);
            }
            url.query_pairs_mut()
                .append_pair("lifecycle", "open")
                .append_pair("limit", "100")
                .append_pair("offset", &offset.to_string());
            let bytes = get_bytes(url, 8 * 1024 * 1024).await?;
            let page: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let sessions = page["sessions"]
                .as_array()
                .ok_or("invalid sessions response")?;
            if let Some(session) = sessions
                .iter()
                .find(|session| session_matches(session, thread, &self.agent))
            {
                break (true, source(session));
            }
            if page["meta"]["hasMore"] != true {
                break (false, SessionSource::Chat);
            }
            let next = page["meta"]["nextOffset"]
                .as_u64()
                .ok_or("missing sessions offset")?;
            if next <= offset || next > 100_000 {
                return Err("invalid sessions pagination".into());
            }
            offset = next;
        };
        self.turns.insert(key, (accepted, origin));
        Ok(self.allowed(accepted, origin))
    }
}

fn session_matches(session: &Value, thread: &str, agent: &str) -> bool {
    if !agent.is_empty()
        && session["selectedAgentId"].as_str().is_none()
        && session["projectId"].as_str().is_some()
    {
        // A legacy project's default agent cannot be inferred from this row.
        return false;
    }
    if session["selectedAgentId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .unwrap_or("xangi:default")
        != agent
        && !agent.is_empty()
    {
        return false;
    }
    crate::session_monitor::thread_id(session).as_deref() == Some(thread)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Query, routing::get, Json, Router};
    use serde_json::json;

    async fn stub() -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route("/api/agents", get(|| async { Json(json!({"agents":[{"id":"a","workspaceId":"workspace-a"}]})) }))
            .route("/api/sessions", get(|Query(query): Query<HashMap<String,String>>| async move {
                assert_eq!(query.get("agentId").map(String::as_str), Some("a"));
                assert_eq!(query.get("lifecycle").map(String::as_str), Some("open"));
                if query.get("offset").map(String::as_str) == Some("0") {
                    Json(json!({"sessions":[],"meta":{"hasMore":true,"nextOffset":100}}))
                } else {
                    Json(json!({"sessions":[{"id":"owned","platform":"web","selectedAgentId":"a"}],"meta":{"hasMore":false}}))
                }
            }))
            .route("/api/workspace-file", get(|Query(query): Query<HashMap<String,String>>| async move {
                assert_eq!(query.get("workspaceId").map(String::as_str), Some("workspace-a"));
                match query.get("path").map(String::as_str) {
                    Some("assets/xangi-pets/pet.json") => br#"{"id":"a","spritesheetPath":"spritesheet.webp"}"#.to_vec(),
                    Some("assets/xangi-pets/spritesheet.webp") => b"RIFF1234WEBP".to_vec(),
                    _ => panic!("unexpected asset path"),
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (base, handle)
    }

    #[tokio::test]
    async fn routes_owned_turns_and_rejects_others_after_pagination() {
        let (base, server) = stub().await;
        let mut router = AgentRouter::new(base, "a".into());
        assert!(router
            .accepts(&json!({"thread_id":"web:owned","turn_id":"1"}))
            .await
            .unwrap());
        assert!(!router
            .accepts(&json!({"thread_id":"web:other","turn_id":"2"}))
            .await
            .unwrap());
        assert!(!router
            .accepts(&json!({"agent_id":"b","thread_id":"web:owned","turn_id":"3"}))
            .await
            .unwrap());
        server.abort();
        // The same turn remains pinned even if the discovery API goes away.
        assert!(router
            .accepts(&json!({"thread_id":"web:owned","turn_id":"1","type":"turn.complete"}))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn loads_assets_from_the_selected_agents_workspace() {
        let (base, server) = stub().await;
        let pet = load_agent_pet(&base, "a", "./assets/xangi-pets")
            .await
            .unwrap();
        assert_eq!(pet.meta["id"], "a");
        assert_eq!(pet.image, b"RIFF1234WEBP");
        assert!(load_agent_pet(&base, "missing", "assets/xangi-pets")
            .await
            .is_err());
        for path in [
            "../secret",
            "assets/../../secret",
            "/tmp/pet",
            "C:\\pet",
            "https://elsewhere",
            "",
        ] {
            assert!(load_agent_pet(&base, "a", path).await.is_err(), "{path}");
        }
        server.abort();
    }

    #[tokio::test]
    async fn discovery_failure_does_not_accept_an_unknown_turn() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let mut router = AgentRouter::new(base, "a".into());
        assert!(router
            .accepts(&json!({"thread_id":"web:owned","turn_id":"1"}))
            .await
            .is_err());
    }
    #[test]
    fn session_routing_requires_both_agent_and_thread() {
        let web = json!({"id":"one", "platform":"web", "selectedAgentId":"a"});
        assert!(session_matches(&web, "web:one", "a"));
        assert!(!session_matches(&web, "web:one", "b"));
        assert!(!session_matches(&web, "web:two", "a"));
        let discord = json!({"id":"app-one", "platform":"discord", "contextKey":"channel", "selectedAgentId":"b"});
        assert!(session_matches(&discord, "discord:channel", "b"));
        assert!(!session_matches(&discord, "discord:app-one", "b"));
        assert!(session_matches(
            &json!({"id":"default-session","platform":"web"}),
            "web:default-session",
            "xangi:default"
        ));
        assert!(session_matches(
            &json!({"platform":"line","contextKey":"line:room","selectedAgentId":"a"}),
            "line:room",
            "a"
        ));
        assert!(session_matches(
            &json!({"platform":"telegram","contextKey":"telegram:chat:-123","selectedAgentId":"a"}),
            "telegram:-123",
            "a"
        ));
    }
}
