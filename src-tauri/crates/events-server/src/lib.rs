// xangi-events-server
//
// Embedded HTTP event-bus that the Tauri pet app spawns at startup.
// Kept as a separate crate so it builds and tests cleanly on Linux without
// pulling in Tauri's webview system dependencies.

pub mod events;
pub mod http_server;
pub mod pet_inbox;
pub mod sse_client;
pub mod thread_state;

pub use http_server::{
    bind_with_autoshift, make_state, make_state_with_extra_pet_dirs, process_event, serve,
    serve_listener, AppState,
};
pub use pet_inbox::{create_pet_session, post_pet_message, PetInboxError, PetInboxResponse};
pub use sse_client::{
    spawn_pull_client, spawn_pull_client_with_callbacks, ConnectionCallback, EventCallback,
    PullClientHandle, PullConnectionState,
};
