use axum::{
    extract::State,
    response::sse::{Event, Sse},
    routing::get,
    Json, Router,
};
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use xangi_events_server::session_monitor::{spawn_session_monitor, SessionSet};

#[tokio::test]
async fn independent_instances_with_the_same_session_id_do_not_share_lifetimes() {
    let mut servers = Vec::new();
    let observed = Arc::new(Mutex::new(std::collections::BTreeMap::new()));
    let mut monitors = Vec::new();
    let (filters, _) =
        tokio::sync::watch::channel(xangi_events_server::session_filters::defaults());
    for name in ["A", "B"] {
        let app = Router::new()
            .route("/api/agents", get(|| async { Json(json!({"agents":[]})) }))
            .route(
                "/api/sessions/stream",
                get(|| async {
                    Sse::new(
                        stream::iter(vec![Ok::<_, Infallible>(
                            Event::default()
                                .event("sessions")
                                .data(json!({"sessions":[row("same",true)]}).to_string()),
                        )])
                        .chain(stream::pending()),
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        servers.push(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let capture = observed.clone();
        monitors.push(spawn_session_monitor(
            &url,
            filters.subscribe(),
            Arc::new(move |pets| {
                capture.lock().unwrap().insert(name, pets);
            }),
        ));
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        while observed.lock().unwrap().len() != 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    monitors.remove(0).shutdown().await;
    observed.lock().unwrap().remove("A");
    assert_eq!(observed.lock().unwrap()["B"][0].session_id, "same");
    assert!(!servers[1].is_finished(), "turning off A must not stop B");
    monitors.remove(0).shutdown().await;
    for server in servers {
        server.abort();
    }
}

fn row(id: &str, active: bool) -> Value {
    json!({"id":id,"platform":"web","selectedAgentId":"a","isActive":active,"lifecycle":"open"})
}

async fn stream_handler(
    State(count): State<Arc<AtomicUsize>>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let index = count.fetch_add(1, Ordering::Relaxed);
    let frames = if index == 0 {
        vec![
            Event::default()
                .event("sessions")
                .data(json!({"sessions":[row("one",true),row("two",true)]}).to_string()),
            Event::default()
                .event("session_changes")
                .data(json!({"upsert":[row("one",false)],"removed":[]}).to_string()),
        ]
    } else {
        vec![Event::default()
            .event("sessions")
            .data(json!({"sessions":[]}).to_string())]
    };
    // Let the first response close, simulating an upstream disconnect.
    let end: std::pin::Pin<Box<dyn futures::Stream<Item = Result<Event, Infallible>> + Send>> =
        if index == 0 {
            Box::pin(stream::empty())
        } else {
            Box::pin(stream::pending())
        };
    Sse::new(stream::iter(frames.into_iter().map(Ok)).chain(end))
}

#[tokio::test]
async fn monitor_reconnect_reconciles_completed_sessions_without_duplicate_windows() {
    let count = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/api/agents",
            get(|| async { Json(json!({"agents":[{"id":"a","name":"Agent A"}]})) }),
        )
        .route("/api/sessions/stream", get(stream_handler))
        .with_state(count.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let observed = Arc::new(Mutex::new(Vec::new()));
    let capture = observed.clone();
    let (filters, receiver) =
        tokio::sync::watch::channel(xangi_events_server::session_filters::defaults());
    let monitor = spawn_session_monitor(
        &base,
        receiver,
        Arc::new(move |pets| {
            if let Some(pet) = pets.first() {
                assert_eq!(pet.agent_name, "Agent A");
            }
            capture.lock().unwrap().push(pets.len());
        }),
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if observed.lock().unwrap().last() == Some(&0) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    monitor.shutdown().await;
    drop(filters);
    assert_eq!(*observed.lock().unwrap(), vec![2, 1, 0]);
    assert!(count.load(Ordering::Relaxed) >= 2);
    server.abort();
}

#[test]
fn initial_snapshot_handles_more_than_one_page_of_sessions_and_defaults() {
    let mut sessions = SessionSet::default();
    let mut rows: Vec<_> = (0..125).map(|i| row(&format!("s{i}"), true)).collect();
    rows.push(json!({"id":"default","isActive":true}));
    rows.push(json!({"id":"closed","isActive":true,"lifecycle":"closed"}));
    rows.push(json!({"id":"history","isActive":false}));
    assert!(sessions.apply("sessions", json!({"sessions":rows})));
    assert_eq!(sessions.active().len(), 126);
    assert_eq!(
        sessions
            .active()
            .iter()
            .find(|pet| pet.session_id == "default")
            .unwrap()
            .agent_id,
        "xangi:default"
    );
    assert!(!sessions.apply("sessions", json!({"bad":true})));
    assert_eq!(
        sessions.active().len(),
        126,
        "malformed snapshots do not close existing pets"
    );
}
