use axum::{
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
    routing::get,
    Router,
};
use futures_util::stream;
use serde_json::json;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};
use xangi_events_server::{
    session_filters::{self, SessionSource},
    session_monitor::spawn_session_monitor,
};

#[tokio::test]
async fn filters_update_without_server_events_and_while_disconnected() {
    let (disconnect, disconnected) = watch::channel(false);
    let app = Router::new()
        .route("/api/agents", get(|| async { axum::Json(json!({"agents":[]})) }))
        .route("/api/sessions/stream", get(move || {
            let disconnected = disconnected.clone();
            async move {
                if *disconnected.borrow() {
                    return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                Sse::new(stream::unfold((false, disconnected), |(sent, mut closed)| async move {
                    if sent { let _ = closed.changed().await; return None; }
                    let data = json!({"sessions":[
                        {"id":"chat","platform":"discord","isActive":true},
                        {"id":"web","platform":"web","isActive":true},
                        {"id":"auto","platform":"web","executionOrigin":"automation","isActive":true}
                    ], "monitorFilters":["scheduler"]});
                    Some((Ok::<_, Infallible>(Event::default().event("sessions").data(data.to_string())), (true, closed)))
                })).into_response()
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let (filters, receiver) = watch::channel(session_filters::defaults());
    let (send, mut received) = mpsc::unbounded_channel();
    let monitor = spawn_session_monitor(
        &url,
        receiver,
        Arc::new(move |pets| {
            let _ = send.send(pets);
        }),
    );
    let initial = tokio::time::timeout(Duration::from_secs(3), received.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        initial
            .iter()
            .map(|p| p.session_id.as_str())
            .collect::<Vec<_>>(),
        ["chat", "web"]
    );
    for (selection, expected) in [
        (vec![], vec![]),
        (vec![SessionSource::Scheduler], vec!["auto"]),
        (
            vec![
                SessionSource::Chat,
                SessionSource::Web,
                SessionSource::Scheduler,
            ],
            vec!["auto", "chat", "web"],
        ),
    ] {
        filters.send_replace(selection);
        let pets = tokio::time::timeout(Duration::from_millis(500), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            pets.iter()
                .map(|p| p.session_id.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
    let (other_send, mut other_received) = mpsc::unbounded_channel();
    let other_monitor = spawn_session_monitor(
        &url,
        filters.subscribe(),
        Arc::new(move |pets| {
            let _ = other_send.send(pets);
        }),
    );
    let later = tokio::time::timeout(Duration::from_secs(3), other_received.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(later.len(), 3, "new monitors inherit the current selection");
    disconnect.send_replace(true);
    tokio::time::sleep(Duration::from_millis(50)).await;
    filters.send_replace(vec![SessionSource::Web]);
    let offline = tokio::time::timeout(Duration::from_millis(500), received.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(offline[0].session_id, "web");
    assert_eq!(offline.len(), 1);
    let other_offline = tokio::time::timeout(Duration::from_millis(500), other_received.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(other_offline.len(), 1);
    assert_eq!(other_offline[0].session_id, "web");
    other_monitor.shutdown().await;
    monitor.shutdown().await;
    server.abort();
}
