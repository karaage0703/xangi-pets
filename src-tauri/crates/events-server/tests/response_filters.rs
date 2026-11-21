use axum::{
    response::sse::{Event, Sse},
    routing::get,
    Json, Router,
};
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;
use xangi_events_server::{
    agents::AgentRouter,
    make_state,
    session_filters::{defaults, SessionSource},
    sse_client::spawn_agent_pull_client,
};

async fn fixture() -> (String, tokio::task::JoinHandle<()>) {
    let app = Router::new().route("/api/sessions", get(|| async { Json(json!({
        "sessions":[
            {"id":"feed","platform":"web","executionOrigin":"automation","selectedAgentId":"a"},
            {"id":"manual","platform":"web","executionOrigin":"manual","selectedAgentId":"a"},
            {"id":"chat","platform":"discord","contextKey":"channel","selectedAgentId":"a"}
        ],"meta":{"hasMore":false}
    })) })).route("/api/events/stream", get(|| async {
        let events: Vec<_> = ["web:feed", "web:manual", "discord:channel"].into_iter().flat_map(|thread| {
            ["turn.started", "turn.complete"].into_iter().map(move |ty| json!({
                "type":ty,"thread_id":thread,"turn_id":"turn","agent_id":"a","text":"response"
            }))
        }).collect();
        Sse::new(stream::iter(events.into_iter().map(|event| Ok::<_, Infallible>(Event::default().data(event.to_string()))))
            .chain(stream::pending()))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, task)
}

#[tokio::test]
async fn automation_off_blocks_feed_before_bubbles_state_and_notifications() {
    let (base, server) = fixture().await;
    let (_filters, receiver) = watch::channel(defaults());
    let state = make_state();
    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let collected = events.clone();
    let handle = spawn_agent_pull_client(
        state.clone(),
        format!("{base}/api/events/stream"),
        Arc::new(|_| {}),
        Arc::new(move |e| collected.lock().unwrap().push(e.clone())),
        Some(AgentRouter::with_filters(base, Some("a".into()), receiver)),
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while events.lock().unwrap().len() < 4 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    handle.shutdown().await;
    assert_eq!(events.lock().unwrap().len(), 4);
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .all(|e| e["thread_id"] != "web:feed"));
    assert_eq!(state.store.snapshot().len(), 2);
    assert!(state
        .store
        .snapshot()
        .iter()
        .all(|s| s["thread_id"] != "web:feed"));
    server.abort();
}

#[tokio::test]
async fn filter_changes_apply_to_cached_turns_with_or_without_agent_selection() {
    let (base, server) = fixture().await;
    for agent in [None, Some("a".into())] {
        let (filters, receiver) = watch::channel(defaults());
        let mut router = AgentRouter::with_filters(base.clone(), agent, receiver);
        let feed = json!({"thread_id":"web:feed","turn_id":"one","agent_id":"a"});
        assert!(!router.accepts(&feed).await.unwrap());
        filters.send_replace(vec![SessionSource::Scheduler]);
        assert!(router.accepts(&feed).await.unwrap());
        filters.send_replace(defaults());
        assert!(!router.accepts(&feed).await.unwrap());
        assert!(router
            .accepts(&json!({"thread_id":"web:manual","turn_id":"two"}))
            .await
            .unwrap());
        assert!(router
            .accepts(&json!({"thread_id":"discord:channel","turn_id":"three"}))
            .await
            .unwrap());
        filters.send_replace(vec![]);
        assert!(!router.accepts(&feed).await.unwrap());
    }
    server.abort();
}
