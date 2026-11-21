//! The monitor's session list is the authority for temporary pet lifetimes.
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use crate::session_filters::{self, SessionSource};
use futures_util::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use serde::Serialize;
use serde_json::Value;
use tokio::{sync::watch, task::JoinHandle};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPet {
    pub persistent: bool,
    pub session_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub title: String,
    pub state: String,
    pub summary: String,
}

pub struct SessionSet {
    filters: Vec<SessionSource>,
    rows: BTreeMap<String, Value>,
}

impl Default for SessionSet {
    fn default() -> Self {
        Self {
            rows: BTreeMap::new(),
            filters: session_filters::defaults(),
        }
    }
}

impl SessionSet {
    /// Only authoritative snapshots/removals and activity snapshots change
    /// lifetimes. A socket disconnect alone never means that work completed.
    pub fn apply(&mut self, event: &str, payload: Value) -> bool {
        match event {
            "sessions" => {
                let Some(rows) = payload["sessions"].as_array() else {
                    return false;
                };
                self.rows.clear();
                for row in rows {
                    self.upsert(row.clone());
                }
            }
            "session_changes" => {
                if let Some(removed) = payload["removed"].as_array() {
                    for id in removed.iter().filter_map(Value::as_str) {
                        self.rows.remove(id);
                    }
                }
                if let Some(upsert) = payload["upsert"].as_array() {
                    for row in upsert {
                        self.upsert(row.clone());
                    }
                }
            }
            "activity_snapshot" => {
                let Some(thread) = payload["threadId"].as_str() else {
                    return false;
                };
                let activity = &payload["activity"];
                if !activity["active"].is_boolean() {
                    return false;
                }
                for row in self.rows.values_mut() {
                    if row["isCurrent"] == false || thread_id(row).as_deref() != Some(thread) {
                        continue;
                    }
                    if activity["updatedAt"].as_u64().unwrap_or(0)
                        < row["activity"]["updatedAt"].as_u64().unwrap_or(0)
                    {
                        continue;
                    }
                    row["isActive"] = activity["active"].clone();
                    row["activity"] = activity.clone();
                }
            }
            _ => return false,
        }
        true
    }

    pub fn set_filters(&mut self, filters: Vec<SessionSource>) {
        self.filters = filters;
    }

    fn upsert(&mut self, row: Value) {
        if let Some(id) = row["id"].as_str().filter(|id| !id.is_empty()) {
            self.rows.insert(id.to_string(), row);
        }
    }

    pub fn active(&self) -> Vec<SessionPet> {
        self.rows
            .values()
            .filter(|row| row["isActive"] == true && row["lifecycle"] != "closed")
            .filter(|row| {
                self.filters
                    .iter()
                    .any(|source| *source == crate::session_filters::source(row))
            })
            .map(|row| SessionPet {
                persistent: false,
                session_id: row["id"].as_str().unwrap().to_string(),
                agent_id: row["selectedAgentId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .unwrap_or("xangi:default")
                    .to_string(),
                agent_name: String::new(),
                title: row["title"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("実行中のセッション")
                    .chars()
                    .take(160)
                    .collect(),
                state: match row["activity"]["state"].as_str() {
                    Some("streaming") => "talking",
                    _ => "thinking",
                }
                .into(),
                summary: row["activity"]["summary"]
                    .as_str()
                    .unwrap_or("実行中")
                    .chars()
                    .take(240)
                    .collect(),
            })
            .collect()
    }
}

pub(crate) fn thread_id(row: &Value) -> Option<String> {
    let platform = row["platform"].as_str()?;
    if row["scope"] == "scheduler" {
        return Some(format!("{platform}-schedule:{}", row["id"].as_str()?));
    }
    let context = row["contextKey"].as_str().unwrap_or("");
    let id = match platform {
        "web" => row["id"].as_str()?,
        "line" => context.strip_prefix("line:").unwrap_or(context),
        "telegram" => context
            .strip_prefix("telegram:chat:")
            .or_else(|| context.strip_prefix("telegram:dm:"))
            .unwrap_or(context),
        _ => context,
    };
    Some(format!("{platform}:{id}"))
}

pub struct SessionMonitorHandle {
    task: JoinHandle<()>,
}

impl SessionMonitorHandle {
    pub async fn shutdown(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

/// `mode=changes` supplies the complete initial list, not the first page.
/// Every reconnection replaces the list to remove windows completed offline.
pub fn spawn_session_monitor(
    base_url: &str,
    mut filters: watch::Receiver<Vec<SessionSource>>,
    on_sessions: Arc<dyn Fn(Vec<SessionPet>) + Send + Sync>,
) -> SessionMonitorHandle {
    let base_url = base_url.to_string();
    let url = format!(
        "{}/api/sessions/stream?mode=changes",
        base_url.trim_end_matches('/')
    );
    let task = tokio::spawn(async move {
        let roster = crate::agents::list_agents(&base_url).await.ok();
        let mut sessions = SessionSet::default();
        let mut has_snapshot = false;
        let publish = |sessions: &SessionSet| {
            let mut active = sessions.active();
            for pet in &mut active {
                pet.agent_name = roster
                    .as_ref()
                    .and_then(|roster| roster["agents"].as_array())
                    .and_then(|agents| agents.iter().find(|agent| agent["id"] == pet.agent_id))
                    .and_then(|agent| agent["name"].as_str())
                    .unwrap_or("Agent")
                    .to_string();
            }
            on_sessions(active);
        };
        let mut backoff = 1;
        loop {
            sessions.set_filters(filters.borrow_and_update().clone());
            let mut source = EventSource::get(&url);
            let mut ready = false;
            loop {
                tokio::select! {
                    changed = filters.changed() => {
                        if changed.is_err() { source.close(); return; }
                        sessions.set_filters(filters.borrow_and_update().clone());
                        if has_snapshot { publish(&sessions); }
                    }
                    event = source.next() => {
                        match event {
                            Some(Ok(Event::Open)) => { backoff = 1; }
                            Some(Ok(Event::Message(message))) => {
                                let Ok(payload) = serde_json::from_str::<Value>(&message.data) else {
                                    continue;
                                };
                                if message.event == "sessions" && payload["sessions"].is_array() {
                                    ready = true;
                                    has_snapshot = true;
                                }
                                if ready && sessions.apply(&message.event, payload) {
                                    publish(&sessions);
                                }
                            }
                            Some(Err(error)) => {
                                eprintln!("xangi-pets: session monitor reconnecting: {error}");
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
            source.close();
            let delay = tokio::time::sleep(Duration::from_secs(backoff));
            tokio::pin!(delay);
            loop {
                tokio::select! {
                    _ = &mut delay => break,
                    changed = filters.changed() => {
                        if changed.is_err() { return; }
                        sessions.set_filters(filters.borrow_and_update().clone());
                        if has_snapshot { publish(&sessions); }
                    }
                }
            }
            backoff = (backoff * 2).min(30);
        }
    });
    SessionMonitorHandle { task }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(id: &str, agent: &str, active: bool) -> Value {
        json!({"id":id,"platform":"web","selectedAgentId":agent,"isActive":active,"lifecycle":"open","isCurrent":true})
    }

    #[test]
    fn local_filters_hide_automation_and_update_without_session_changes() {
        let mut set = SessionSet::default();
        let mut automation = row("auto", "a", true);
        automation["executionOrigin"] = json!("automation");
        let mut scheduler = row("schedule", "a", true);
        scheduler["scope"] = json!("scheduler");
        let mut chat = row("chat", "a", true);
        chat["platform"] = json!("discord");
        set.apply("sessions", json!({"sessions":[row("web","a",true), automation, scheduler, chat], "monitorFilters":["chat","web"]}));
        assert_eq!(set.active().len(), 2);
        set.set_filters(vec![SessionSource::Scheduler]);
        assert_eq!(
            set.active()
                .iter()
                .map(|p| p.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["auto", "schedule"]
        );
        set.set_filters(vec![]);
        assert!(set.active().is_empty());
        set.apply(
            "activity_snapshot",
            json!({"threadId":"web:auto","activity":{"active":true,"updatedAt":20}}),
        );
        assert!(set.active().is_empty());
        set.set_filters(vec![SessionSource::Web]);
        assert_eq!(set.active()[0].session_id, "web");
    }

    #[test]
    fn server_filters_never_override_local_selection() {
        let mut set = SessionSet::default();
        let mut automation = row("auto", "a", true);
        automation["executionOrigin"] = json!("automation");
        let snapshot =
            json!({"sessions":[row("web","a",true), automation], "monitorFilters":["scheduler"]});
        set.apply("sessions", snapshot.clone());
        assert_eq!(set.active()[0].session_id, "web");
        set.set_filters(vec![SessionSource::Scheduler]);
        set.apply("session_changes", json!({"monitorFilters":["web"]}));
        set.apply("sessions", snapshot);
        assert_eq!(set.active()[0].session_id, "auto");
    }

    #[test]
    fn two_sessions_of_one_agent_get_two_pets_and_complete_independently() {
        let mut set = SessionSet::default();
        set.apply(
            "sessions",
            json!({"sessions":[row("one","a",true),row("two","a",true),row("idle","b",false)]}),
        );
        assert_eq!(set.active().len(), 2);
        set.apply(
            "session_changes",
            json!({"upsert":[row("one","a",false)],"removed":[]}),
        );
        assert_eq!(set.active()[0].session_id, "two");
        set.apply("session_changes", json!({"upsert":[],"removed":["two"]}));
        assert!(set.active().is_empty());
    }

    #[test]
    fn reconnect_snapshot_replaces_old_sessions_and_a_reused_session_can_run_again() {
        let mut set = SessionSet::default();
        set.apply("sessions", json!({"sessions":[row("one","a",true)]}));
        // Repeated snapshots never duplicate a session.
        set.apply("sessions", json!({"sessions":[row("one","a",true)]}));
        assert_eq!(set.active().len(), 1);
        set.apply("sessions", json!({"sessions":[]}));
        assert!(set.active().is_empty());
        set.apply("session_changes", json!({"upsert":[row("one","a",true)]}));
        assert_eq!(set.active()[0].session_id, "one");
    }

    #[test]
    fn activity_completion_and_stale_updates_follow_monitor_state() {
        let mut set = SessionSet::default();
        set.apply("sessions", json!({"sessions":[row("one","a",true)]}));
        set.apply("activity_snapshot", json!({"threadId":"web:one","activity":{"active":true,"state":"streaming","updatedAt":20}}));
        assert_eq!(set.active()[0].state, "talking");
        set.apply(
            "activity_snapshot",
            json!({"threadId":"web:one","activity":{"active":false,"updatedAt":10}}),
        );
        assert_eq!(set.active().len(), 1);
        set.apply("activity_snapshot", json!({"threadId":"web:one","activity":{"active":false,"state":"complete","updatedAt":30}}));
        assert!(set.active().is_empty());
        assert!(!set.apply(
            "activity",
            json!({"type":"turn.started","thread_id":"web:unknown"})
        ));
        assert!(set.active().is_empty());
    }
}
