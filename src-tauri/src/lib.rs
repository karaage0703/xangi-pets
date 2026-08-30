use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItem, MenuItemBuilder,
    PredefinedMenuItem, SubmenuBuilder,
};
use tauri::tray::TrayIconBuilder;
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Wry};
#[cfg(target_os = "macos")]
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use xangi_events_server::{
    create_pet_session, make_state_with_extra_pet_dirs, post_pet_message,
    spawn_pull_client_with_callbacks, AppState, PetInboxError, PullClientHandle,
    PullConnectionState,
};

const DEFAULT_PORT: u16 = 7895;
const PORT_AUTOSHIFT_TRIES: u16 = 10;

/// Set once the embedded server has bound to a TCP port. The frontend asks
/// for this via the `get_server_url` command on startup; it can't hard-code
/// 7895 because we may have auto-shifted to 7896/7897/... when running
/// multiple xangi-pets instances side-by-side.
static SERVER_URL: OnceLock<String> = OnceLock::new();

/// State that the Tauri commands need to reach: the bus-bearing AppState
/// (shared with the embedded HTTP server) and the currently-running pull
/// client (so we can stop+restart it when the user enters a new xangi URL).
struct PullState {
    app: AppState,
    app_handle: AppHandle,
    handle: Mutex<Option<PullClientHandle>>,
    current_url: Mutex<Option<String>>,
    pet_session_id: Mutex<Option<String>>,
    connection: Mutex<AppConnectionState>,
    notifications_enabled: AtomicBool,
    normal_responses_enabled: AtomicBool,
    completion_display_enabled: AtomicBool,
    notification_turns: Mutex<HashSet<String>>,
    generation: AtomicU64,
}

static PULL_STATE: OnceLock<Arc<PullState>> = OnceLock::new();

struct AppMenuState {
    status: MenuItem<Wry>,
    port: MenuItem<Wry>,
    notifications: CheckMenuItem<Wry>,
    normal_responses: CheckMenuItem<Wry>,
    completion_display: CheckMenuItem<Wry>,
}

static APP_MENU_STATE: OnceLock<AppMenuState> = OnceLock::new();

// Pet sprite size in *logical* (CSS) pixels — the unit used by the frontend
// canvas. The hit-test polling loop multiplies these by the current scale
// factor before comparing against cursor position. Updated at runtime via
// `set_pet_size` whenever the user changes pet-scale (frontend reads
// `xangi-pets:scale` from localStorage and pushes it down on init).
//
// Default mirrors the frontend default: SCALE=0.5, CELL=192x208 → 96x104.
static PET_W: AtomicI32 = AtomicI32::new(96);
static PET_H: AtomicI32 = AtomicI32::new(104);

// When a bubble is visible the user must be able to click anywhere on it,
// not just on the pet sprite. The frontend toggles this via the
// `set_bubble_active` Tauri command whenever the bubble stack is non-empty.
static BUBBLE_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppConnectionState {
    NotConfigured,
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
}

impl AppConnectionState {
    fn label(self) -> &'static str {
        match self {
            Self::NotConfigured => "未設定",
            Self::Connecting => "接続中",
            Self::Connected => "接続済み",
            Self::Reconnecting => "再接続中",
            Self::Disconnected => "切断",
        }
    }

    fn event_value(self) -> &'static str {
        match self {
            Self::NotConfigured => "not-configured",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
            Self::Disconnected => "disconnected",
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The embedded Web Chat is a remote page and intentionally has no Tauri IPC
    // capability. Disable the opener plugin's injected click handler so it does
    // not prevent `_blank` links before `on_new_window` below can handle them.
    let builder = tauri::Builder::default().plugin(
        tauri_plugin_opener::Builder::new()
            .open_js_links_on_click(false)
            .build(),
    );
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_plugin_notification::init());

    builder
        .setup(|app| {
            // Build a minimal macOS menu bar: App menu (About / Preferences /
            // Quit) + Help menu (Show Help). On Linux/Windows it shows up as
            // a regular menu bar; the pet window itself is decoration-less so
            // it never renders an in-window menu strip — the menu only lives
            // in the system menu bar / nowhere visible. Menu actions are
            // forwarded to the webview as Tauri events (`menu:about` etc.) so
            // the JS side can render the help overlay / open the URL prompt.
            if let Err(err) = install_app_menu(app) {
                eprintln!("xangi-pets: failed to install app menu: {err}");
            }

            if let Some(window) = app.get_webview_window("pet") {
                #[cfg(target_os = "macos")]
                {
                    use tauri::TitleBarStyle;
                    let _ = window.set_title_bar_style(TitleBarStyle::Transparent);
                }

                // Start fully click-through. The polling task below flips this
                // back to "receive clicks" only when the cursor is over the
                // pet sprite or a bubble is showing.
                let _ = window.set_ignore_cursor_events(true);
                spawn_hit_polling(window);
            }

            // Spawn the embedded event-bus HTTP server (replaces the old Node sidecar).
            // Bind to 0.0.0.0 so a remote xangi (e.g. on DGX) can POST events
            // over Tailscale to the Mac running the pet.
            let port: u16 = std::env::var("XANGI_PET_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_PORT);
            let bind: std::net::IpAddr = std::env::var("XANGI_PET_BIND")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| "0.0.0.0".parse().unwrap());
            let addr = SocketAddr::new(bind, port);

            // Bundled fallback pet dir. We ship `xangi/{pet.json,spritesheet.webp}`
            // under `resources/default-pets/` so a fresh install has at least one
            // pet without the user having to run `hatch-pet` first. Listed at
            // *lowest* priority — anything in `~/.xangi/pets` or `~/.codex/pets`
            // still wins, so users can override the bundled xangi just by placing
            // their own `xangi/` folder there.
            let bundled_pet_dir = match app.path().resource_dir() {
                Ok(rd) => Some(rd.join("resources").join("default-pets")),
                Err(err) => {
                    eprintln!("xangi-pets: resource_dir lookup failed: {err}");
                    None
                }
            };
            let extra_pet_dirs: Vec<std::path::PathBuf> = bundled_pet_dir.into_iter().collect();
            let state = make_state_with_extra_pet_dirs(extra_pet_dirs);
            let pet_dirs = state.pet_dirs.clone();

            // Stash the bus-bearing state so the Tauri commands below can
            // reach it (set_xangi_url restarts the pull client by handing
            // this state to spawn_pull_client again).
            let pull_state = Arc::new(PullState {
                app: state.clone(),
                app_handle: app.handle().clone(),
                handle: Mutex::new(None),
                current_url: Mutex::new(None),
                pet_session_id: Mutex::new(None),
                connection: Mutex::new(AppConnectionState::NotConfigured),
                notifications_enabled: AtomicBool::new(false),
                normal_responses_enabled: AtomicBool::new(true),
                completion_display_enabled: AtomicBool::new(true),
                notification_turns: Mutex::new(HashSet::new()),
                generation: AtomicU64::new(0),
            });
            let _ = PULL_STATE.set(pull_state.clone());
            install_tray(app.handle())?;

            let server_state = state.clone();
            let tray_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let listener = match xangi_events_server::bind_with_autoshift(
                    addr,
                    PORT_AUTOSHIFT_TRIES,
                )
                .await
                {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("xangi-events: failed to bind anywhere from {addr}: {e}");
                        return;
                    }
                };
                let bound = listener.local_addr().unwrap_or(addr);
                let url = format!("http://127.0.0.1:{}", bound.port());
                let _ = SERVER_URL.set(url.clone());
                refresh_tray(&tray_app);

                println!("xangi-events listening on http://{bound}");
                if bound.port() != addr.port() {
                    println!(
                        "  (requested :{} was in use, auto-shifted to :{})",
                        addr.port(),
                        bound.port()
                    );
                }
                println!("  pet dirs (search order):");
                for d in pet_dirs.iter() {
                    println!("    - {}", d.display());
                }
                if let Err(err) = xangi_events_server::serve_listener(listener, server_state).await
                {
                    eprintln!("xangi-events server error: {err}");
                }
            });

            // Optionally bootstrap the pull client from $XANGI_URL so that
            // CI / dev runs can start receiving without going through the
            // webview onboarding UI. Frontend can override later via
            // `set_xangi_url`.
            if let Ok(url) = std::env::var("XANGI_URL") {
                match normalize_xangi_url(&url) {
                    Ok(url) => {
                        let pull_state = pull_state.clone();
                        tauri::async_runtime::spawn(async move {
                            start_pull_client(&pull_state, url);
                        });
                    }
                    Err(err) => {
                        eprintln!("xangi-pets: ignored unsafe XANGI_URL: {err}");
                    }
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_bubble_active,
            get_server_url,
            set_pet_size,
            set_xangi_url,
            get_xangi_url,
            clear_xangi_url,
            send_pet_message,
            set_notifications_enabled,
            get_notifications_enabled,
            set_normal_responses_enabled,
            set_completion_display_enabled,
            get_connection_status,
            open_web_chat
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Returns the URL the embedded HTTP server actually bound to. The
/// frontend invokes this on startup so it can connect to the right port
/// even when port auto-shift has moved the server off the default 7895.
/// Returns `None` while the server is still binding (frontend should retry).
#[tauri::command]
fn get_server_url() -> Option<String> {
    SERVER_URL.get().cloned()
}

/// Frontend tells us whether at least one bubble is currently displayed.
/// While true, the entire window is click-receiving so the user can click
/// the bubble (which sits above the pet sprite). Otherwise the pet hit-test
/// is the only interactive area.
#[tauri::command]
fn set_bubble_active(active: bool) {
    BUBBLE_ACTIVE.store(active, Ordering::Relaxed);
}

/// Frontend pushes the current pet sprite size (logical/CSS px) down so the
/// hit-test polling rectangle stays aligned with what's actually rendered on
/// the canvas. Called on init and whenever `xangi-pets:scale` changes.
#[tauri::command]
fn set_pet_size(w: i32, h: i32) {
    if w > 0 {
        PET_W.store(w, Ordering::Relaxed);
    }
    if h > 0 {
        PET_H.store(h, Ordering::Relaxed);
    }
}

/// Frontend tells us which xangi instance to subscribe to. Stops the
/// previous pull client (if any) and starts a fresh one. Returns the URL
/// we ended up using (mainly so the frontend can confirm it was applied).
///
/// `url` should be the **base URL** of the xangi web-chat server, e.g.
/// `http://localhost:18888`. The pet appends `/api/events/stream` itself.
///
/// **Must be async**: `start_pull_client` calls `tokio::spawn` deep inside
/// `events-server::spawn_pull_client`, which panics ("there is no reactor
/// running") when invoked from a sync command — Tauri 2 runs sync commands
/// on a worker thread without a Tokio runtime context. async commands run
/// inside `tauri::async_runtime` (which IS Tokio), so the inner spawn works.
#[tauri::command]
async fn set_xangi_url(url: String) -> Result<String, String> {
    let trimmed = normalize_xangi_url(&url)?;
    let pull_state = PULL_STATE.get().ok_or("pull state not initialised")?;
    start_pull_client(pull_state, trimmed.clone());
    Ok(trimmed)
}

/// Returns the xangi URL the pull client is currently subscribed to, or
/// `None` when no URL has been configured yet (frontend should show the
/// onboarding screen in that case).
#[tauri::command]
fn get_xangi_url() -> Option<String> {
    PULL_STATE
        .get()
        .and_then(|s| s.current_url.lock().ok().and_then(|g| g.clone()))
}

/// Send a single text line to the upstream xangi's `POST /api/pet/inbox`.
///
/// Uses the same base URL the pull client is subscribed to (so the pet UI
/// only ever talks to "its" xangi — inter-instance routing is out of scope).
/// The token, if any, comes from the `XANGI_PET_INBOX_TOKEN` env var. xangi
/// itself only requires a token when accessed from a public (non-LAN /
/// non-Tailscale) IP, so the common "self-hosted xangi on Tailscale +
/// pet on Mac" setup works without any token at all.
///
/// Returns the thread_id from the 202 response so the JS side can correlate
/// the agent reply that will arrive shortly on the SSE pull stream.
#[tauri::command]
async fn send_pet_message(text: String) -> Result<String, String> {
    let pull_state = PULL_STATE.get().ok_or("pull state not initialised")?;
    let base_url = pull_state
        .current_url
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .ok_or("xangi URL not set — open the URL prompt with `x` first")?;
    let token = std::env::var("XANGI_PET_INBOX_TOKEN").ok();
    let token_ref = token.as_deref().filter(|s| !s.is_empty());
    let session_id = pull_state
        .pet_session_id
        .lock()
        .ok()
        .and_then(|session_id| session_id.clone());
    let session_id = match session_id {
        Some(session_id) => session_id,
        None => {
            let session_id = create_pet_session(&base_url, token_ref)
                .await
                .map_err(|e| e.to_string())?;
            if let Ok(mut current) = pull_state.pet_session_id.lock() {
                *current = Some(session_id.clone());
            }
            session_id
        }
    };
    let resp = match post_pet_message(&base_url, &text, token_ref, &session_id).await {
        Ok(resp) => resp,
        Err(PetInboxError::Rejected { status: 404, .. }) => {
            // xangi may have restarted and forgotten an in-memory session.
            // Create one replacement and retry this message exactly once.
            let replacement = create_pet_session(&base_url, token_ref)
                .await
                .map_err(|e| e.to_string())?;
            if let Ok(mut current) = pull_state.pet_session_id.lock() {
                *current = Some(replacement.clone());
            }
            post_pet_message(&base_url, &text, token_ref, &replacement)
                .await
                .map_err(|e| e.to_string())?
        }
        Err(err) => return Err(err.to_string()),
    };
    Ok(resp.thread_id.unwrap_or_default())
}

/// Stop the pull client and forget the configured URL. Used when the
/// frontend wants to show the onboarding screen again.
#[tauri::command]
fn clear_xangi_url() {
    if let Some(pull_state) = PULL_STATE.get() {
        pull_state.generation.fetch_add(1, Ordering::Relaxed);
        set_connection_state(pull_state, AppConnectionState::NotConfigured);
        // Cancel the running task without blocking the command. The async
        // shutdown will run on the tokio pool.
        let pull_state = pull_state.clone();
        tauri::async_runtime::spawn(async move {
            stop_pull_client(&pull_state).await;
            if let Ok(mut url) = pull_state.current_url.lock() {
                *url = None;
            }
            if let Ok(mut session_id) = pull_state.pet_session_id.lock() {
                *session_id = None;
            }
            refresh_tray(&pull_state.app_handle);
        });
    }
}

#[tauri::command]
fn set_notifications_enabled(enabled: bool) {
    if let Some(pull_state) = PULL_STATE.get() {
        pull_state
            .notifications_enabled
            .store(enabled, Ordering::Relaxed);
        if let Ok(mut turns) = pull_state.notification_turns.lock() {
            turns.clear();
        }
        refresh_tray(&pull_state.app_handle);
        let _ = pull_state
            .app_handle
            .emit("pet://notifications-changed", enabled);
    }
}

#[tauri::command]
fn get_notifications_enabled() -> bool {
    PULL_STATE
        .get()
        .map(|state| state.notifications_enabled.load(Ordering::Relaxed))
        .unwrap_or(false)
}

#[tauri::command]
fn set_normal_responses_enabled(enabled: bool) {
    if let Some(state) = PULL_STATE.get() {
        state
            .normal_responses_enabled
            .store(enabled, Ordering::Relaxed);
        refresh_tray(&state.app_handle);
        let _ = state
            .app_handle
            .emit("pet://normal-responses-changed", enabled);
    }
}

#[tauri::command]
fn set_completion_display_enabled(enabled: bool) {
    if let Some(state) = PULL_STATE.get() {
        state
            .completion_display_enabled
            .store(enabled, Ordering::Relaxed);
        refresh_tray(&state.app_handle);
        let _ = state
            .app_handle
            .emit("pet://completion-display-changed", enabled);
    }
}

#[tauri::command]
fn get_connection_status() -> String {
    PULL_STATE
        .get()
        .and_then(|state| {
            state
                .connection
                .lock()
                .ok()
                .map(|value| value.event_value().into())
        })
        .unwrap_or_else(|| AppConnectionState::NotConfigured.event_value().into())
}

#[tauri::command]
fn open_web_chat(app: AppHandle) -> Result<(), String> {
    open_configured_web_chat(&app)
}

/// Append `/api/events/stream` to a base URL and start (or restart) the
/// pull client. Synchronous: the actual stream-reading runs on a tokio
/// task, but we replace the handle synchronously so subsequent calls see
/// the new handle.
fn start_pull_client(pull_state: &Arc<PullState>, base_url: String) {
    let stream_url = format!("{base_url}/api/events/stream");
    println!("xangi-events pull: subscribing to {stream_url}");
    let generation = pull_state.generation.fetch_add(1, Ordering::Relaxed) + 1;
    set_connection_state(pull_state, AppConnectionState::Connecting);
    if let Ok(mut session_id) = pull_state.pet_session_id.lock() {
        *session_id = None;
    }

    // Stop the previous client (if any) before starting a new one.
    let prev_handle = pull_state.handle.lock().ok().and_then(|mut g| g.take());
    if let Some(prev) = prev_handle {
        let pull_state = pull_state.clone();
        tauri::async_runtime::spawn(async move {
            prev.shutdown().await;
            // Drop only happens after shutdown returns.
            drop(pull_state);
        });
    }

    let state_for_connection = pull_state.clone();
    let on_connection = Arc::new(move |connection: PullConnectionState| {
        if state_for_connection.generation.load(Ordering::Relaxed) != generation {
            return;
        }
        let mapped = match connection {
            PullConnectionState::Connecting => AppConnectionState::Connecting,
            PullConnectionState::Connected => AppConnectionState::Connected,
            PullConnectionState::Reconnecting => AppConnectionState::Reconnecting,
            PullConnectionState::Disconnected => AppConnectionState::Disconnected,
        };
        set_connection_state(&state_for_connection, mapped);
    });
    let state_for_events = pull_state.clone();
    let on_event = Arc::new(move |event: &serde_json::Value| {
        if state_for_events.generation.load(Ordering::Relaxed) == generation {
            handle_notification_event(&state_for_events, event);
        }
    });
    let new_handle = spawn_pull_client_with_callbacks(
        pull_state.app.clone(),
        stream_url,
        on_connection,
        on_event,
    );
    if let Ok(mut g) = pull_state.handle.lock() {
        *g = Some(new_handle);
    }
    if let Ok(mut g) = pull_state.current_url.lock() {
        *g = Some(base_url);
    }
    refresh_tray(&pull_state.app_handle);
}

async fn stop_pull_client(pull_state: &Arc<PullState>) {
    let prev = pull_state.handle.lock().ok().and_then(|mut g| g.take());
    if let Some(h) = prev {
        h.shutdown().await;
    }
}

fn normalize_xangi_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("xangi URL is empty".into());
    }
    let mut parsed = tauri::Url::parse(trimmed).map_err(|_| "xangi URL is invalid")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("xangi URL must use http:// or https://".into());
    }
    if parsed.host_str().is_none() {
        return Err("xangi URL must include a host".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("xangi URL must not include user information".into());
    }
    parsed.set_query(None);
    parsed.set_fragment(None);
    Ok(parsed.to_string().trim_end_matches('/').to_string())
}

fn set_connection_state(pull_state: &PullState, connection: AppConnectionState) {
    if connection != AppConnectionState::Connected {
        if let Ok(mut turns) = pull_state.notification_turns.lock() {
            turns.clear();
        }
    }
    if let Ok(mut current) = pull_state.connection.lock() {
        *current = connection;
    }
    let _ = pull_state
        .app_handle
        .emit("pet://connection-status", connection.event_value());
    refresh_tray(&pull_state.app_handle);
}

fn handle_notification_event(pull_state: &PullState, event: &serde_json::Value) {
    if !pull_state.notifications_enabled.load(Ordering::Relaxed) {
        return;
    }
    let should_notify = pull_state
        .notification_turns
        .lock()
        .map(|mut turns| notification_transition(&mut turns, event))
        .unwrap_or(false);
    if !should_notify {
        return;
    }
    let Some(event_type) = event.get("type").and_then(|value| value.as_str()) else {
        return;
    };
    let (title, body) = if event_type == "agent.error" {
        (
            "xangiでエラー",
            event
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("処理中にエラーが発生しました"),
        )
    } else {
        (
            "xangiの応答が完了",
            event
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or("新しい応答があります"),
        )
    };
    let body = truncate_notification(body, 120);
    show_system_notification(&pull_state.app_handle, title, &body);
}

fn notification_transition(turns: &mut HashSet<String>, event: &serde_json::Value) -> bool {
    let Some(event_type) = event.get("type").and_then(|value| value.as_str()) else {
        return false;
    };
    let Some(thread_id) = event.get("thread_id").and_then(|value| value.as_str()) else {
        return false;
    };
    let Some(turn_id) = event.get("turn_id").and_then(|value| value.as_str()) else {
        return false;
    };
    let key = format!("{thread_id}\u{0}{turn_id}");
    match event_type {
        "turn.started" => {
            turns.insert(key);
            false
        }
        "turn.complete" | "agent.error" => turns.remove(&key),
        "turn.aborted" => {
            turns.remove(&key);
            false
        }
        _ => false,
    }
}

#[cfg(target_os = "macos")]
fn show_system_notification(app_handle: &AppHandle, title: &str, body: &str) {
    if let Err(err) = app_handle
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
    {
        eprintln!("xangi-pets: notification failed: {err}");
    }
}

#[cfg(not(target_os = "macos"))]
fn show_system_notification(_app_handle: &AppHandle, _title: &str, _body: &str) {}

fn truncate_notification(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn install_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_tray_menu(app)?;
    let mut builder = TrayIconBuilder::with_id("xangi-pets-tray")
        .tooltip("xangi-pets")
        .menu(&menu);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn build_tray_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let (connection, notifications_enabled, normal_responses_enabled, completion_display_enabled) =
        PULL_STATE
            .get()
            .map(|state| {
                (
                    state
                        .connection
                        .lock()
                        .map(|value| *value)
                        .unwrap_or(AppConnectionState::Disconnected),
                    state.notifications_enabled.load(Ordering::Relaxed),
                    state.normal_responses_enabled.load(Ordering::Relaxed),
                    state.completion_display_enabled.load(Ordering::Relaxed),
                )
            })
            .unwrap_or((AppConnectionState::NotConfigured, false, true, true));
    let status = MenuItemBuilder::with_id("tray_status", format!("xangi: {}", connection.label()))
        .enabled(false)
        .build(app)?;
    let port = MenuItemBuilder::with_id(
        "tray_port",
        format!(
            "pet server: {}",
            SERVER_URL
                .get()
                .and_then(|url| url.rsplit(':').next())
                .map(str::to_owned)
                .unwrap_or_else(|| "起動中".into())
        ),
    )
    .enabled(false)
    .build(app)?;
    let show = MenuItemBuilder::with_id("tray_show", "ペットを表示").build(app)?;
    let hide = MenuItemBuilder::with_id("tray_hide", "ペットを隠す").build(app)?;
    let talk = MenuItemBuilder::with_id("tray_talk", "xangiに話しかける…").build(app)?;
    let open_chat =
        MenuItemBuilder::with_id("tray_open_chat", "Web Chatをアプリで開く").build(app)?;
    let open_chat_browser =
        MenuItemBuilder::with_id("tray_open_chat_browser", "Web Chatをブラウザで開く")
            .build(app)?;
    let preferences =
        MenuItemBuilder::with_id("tray_preferences", "xangi URLを設定…").build(app)?;
    let normal_responses = CheckMenuItemBuilder::with_id("tray_normal_responses", "通常応答を表示")
        .checked(normal_responses_enabled)
        .build(app)?;
    let completion_display =
        CheckMenuItemBuilder::with_id("tray_completion_display", "完了通知を表示")
            .checked(completion_display_enabled)
            .build(app)?;
    let notifications = CheckMenuItemBuilder::with_id("tray_notifications", "システム通知")
        .checked(notifications_enabled)
        .build(app)?;
    let help = MenuItemBuilder::with_id("tray_help", "ヘルプ").build(app)?;
    let quit = MenuItemBuilder::with_id("tray_quit", "終了").build(app)?;
    MenuBuilder::new(app)
        .items(&[
            &status,
            &port,
            &PredefinedMenuItem::separator(app)?,
            &show,
            &hide,
            &talk,
            &open_chat,
            &open_chat_browser,
            &preferences,
            &normal_responses,
            &completion_display,
            &notifications,
            &PredefinedMenuItem::separator(app)?,
            &help,
            &quit,
        ])
        .build()
}

fn refresh_tray(app: &AppHandle) {
    refresh_app_menu(app);
    let Some(tray) = app.tray_by_id("xangi-pets-tray") else {
        return;
    };
    match build_tray_menu(app) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(err) => eprintln!("xangi-pets: failed to refresh tray menu: {err}"),
    }
    if let Some(state) = PULL_STATE.get() {
        let connection = state
            .connection
            .lock()
            .map(|value| *value)
            .unwrap_or(AppConnectionState::Disconnected);
        let port = SERVER_URL
            .get()
            .and_then(|url| url.rsplit(':').next())
            .unwrap_or("…");
        let _ = tray.set_tooltip(Some(format!(
            "xangi-pets · {} · port {port}",
            connection.label()
        )));
    }
}

fn handle_control_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "tray_show" | "menu_show" => show_pet_window(app),
        "tray_hide" | "menu_hide" => {
            if let Some(window) = app.get_webview_window("pet") {
                let _ = window.hide();
            }
        }
        "tray_talk" | "menu_talk" => {
            show_pet_window(app);
            let _ = app.emit("pet://talk", ());
        }
        "tray_open_chat" | "menu_open_chat" => {
            if let Err(err) = open_configured_web_chat_window(app) {
                eprintln!("xangi-pets: open in-app Web Chat failed: {err}");
            }
        }
        "tray_open_chat_browser" | "menu_open_chat_browser" => {
            if let Err(err) = open_configured_web_chat(app) {
                eprintln!("xangi-pets: open Web Chat in browser failed: {err}");
            }
        }
        "tray_preferences" | "menu_prefs" => {
            show_pet_window(app);
            let _ = app.emit("pet://set-xangi-url", ());
        }
        "tray_notifications" | "menu_notifications" => {
            let enabled = PULL_STATE
                .get()
                .map(|state| !state.notifications_enabled.load(Ordering::Relaxed))
                .unwrap_or(false);
            set_notifications_enabled(enabled);
        }
        "tray_normal_responses" | "menu_normal_responses" => {
            let enabled = PULL_STATE
                .get()
                .map(|state| !state.normal_responses_enabled.load(Ordering::Relaxed))
                .unwrap_or(true);
            set_normal_responses_enabled(enabled);
        }
        "tray_completion_display" | "menu_completion_display" => {
            let enabled = PULL_STATE
                .get()
                .map(|state| !state.completion_display_enabled.load(Ordering::Relaxed))
                .unwrap_or(true);
            set_completion_display_enabled(enabled);
        }
        "tray_help" | "menu_help" | "menu_about" => {
            show_pet_window(app);
            let _ = app.emit("pet://show-help", ());
        }
        "tray_quit" => app.exit(0),
        _ => {}
    }
}

fn show_pet_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("pet") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn open_configured_web_chat(app: &AppHandle) -> Result<(), String> {
    let url = configured_xangi_url()?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|err| err.to_string())
}

fn configured_xangi_url() -> Result<String, String> {
    PULL_STATE
        .get()
        .and_then(|state| {
            state
                .current_url
                .lock()
                .ok()
                .and_then(|value| value.clone())
        })
        .ok_or_else(|| "xangi URL is not configured".into())
}

fn open_web_chat_new_window<F, E>(url: tauri::Url, open_url: F) -> NewWindowResponse<Wry>
where
    F: FnOnce(&str) -> Result<(), E>,
    E: std::fmt::Display,
{
    if matches!(url.scheme(), "http" | "https") {
        if let Err(err) = open_url(url.as_str()) {
            eprintln!("xangi-pets: open Web Chat link in browser failed: {err}");
        }
    } else {
        eprintln!(
            "xangi-pets: refused Web Chat new-window URL with unsupported scheme: {}",
            url.scheme()
        );
    }

    NewWindowResponse::Deny
}

fn open_configured_web_chat_window(app: &AppHandle) -> Result<(), String> {
    let url = tauri::Url::parse(&configured_xangi_url()?)
        .map_err(|_| "configured xangi URL is invalid")?;

    if let Some(window) = app.get_webview_window("web-chat") {
        let should_navigate = window.url().map(|current| current != url).unwrap_or(true);
        if should_navigate {
            window.navigate(url).map_err(|err| err.to_string())?;
        }
        let _ = window.unminimize();
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }

    let opener = app.clone();
    WebviewWindowBuilder::new(app, "web-chat", WebviewUrl::External(url))
        .title("xangi Web Chat")
        .inner_size(1100.0, 760.0)
        .min_inner_size(720.0, 480.0)
        .resizable(true)
        .center()
        .on_new_window(move |url, _features| {
            open_web_chat_new_window(url, |url| {
                opener
                    .opener()
                    .open_url(url, None::<&str>)
                    .map_err(|err| err.to_string())
            })
        })
        .build()
        .map(|_| ())
        .map_err(|err| err.to_string())
}

/// Background task: 50ms tick that toggles set_ignore_cursor_events based on
/// whether the cursor is currently inside the pet sprite (or a bubble is up).
fn spawn_hit_polling(window: tauri::WebviewWindow) {
    tauri::async_runtime::spawn(async move {
        let mut last_ignore: Option<bool> = None;
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;

            let want_ignore = if BUBBLE_ACTIVE.load(Ordering::Relaxed) {
                // bubble visible → accept all window-region clicks (the bubble
                // sits inside the window so the click reaches the DOM)
                false
            } else {
                cursor_outside_pet(&window).unwrap_or(true)
            };

            // Only poke the platform if the value actually changed; on macOS
            // changing this constantly seems to cost a tiny bit of latency.
            if last_ignore != Some(want_ignore) {
                if window.set_ignore_cursor_events(want_ignore).is_ok() {
                    last_ignore = Some(want_ignore);
                }
            }
        }
    });
}

/// Build the application menu and wire up the same controls that are exposed
/// by the tray. On macOS the first submenu becomes the application menu
/// automatically. The normal Edit menu remains present because WKWebView
/// clipboard shortcuts depend on those predefined items.
fn install_app_menu(app: &tauri::App) -> tauri::Result<()> {
    let about = MenuItemBuilder::with_id("menu_about", "About xangi-pets").build(app)?;
    let status = MenuItemBuilder::with_id("menu_status", "xangi: 未設定")
        .enabled(false)
        .build(app)?;
    let port = MenuItemBuilder::with_id("menu_port", "pet server: 起動中")
        .enabled(false)
        .build(app)?;
    let show = MenuItemBuilder::with_id("menu_show", "ペットを表示").build(app)?;
    let hide = MenuItemBuilder::with_id("menu_hide", "ペットを隠す").build(app)?;
    let talk = MenuItemBuilder::with_id("menu_talk", "xangiに話しかける…")
        .accelerator("CmdOrCtrl+T")
        .build(app)?;
    let open_chat =
        MenuItemBuilder::with_id("menu_open_chat", "Web Chatをアプリで開く").build(app)?;
    let open_chat_browser =
        MenuItemBuilder::with_id("menu_open_chat_browser", "Web Chatをブラウザで開く")
            .build(app)?;
    let prefs = MenuItemBuilder::with_id("menu_prefs", "Preferences (xangi URL)…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let normal_responses = CheckMenuItemBuilder::with_id("menu_normal_responses", "通常応答を表示")
        .checked(true)
        .build(app)?;
    let completion_display =
        CheckMenuItemBuilder::with_id("menu_completion_display", "完了通知を表示")
            .checked(true)
            .build(app)?;
    let notifications = CheckMenuItemBuilder::with_id("menu_notifications", "システム通知")
        .checked(false)
        .build(app)?;
    let help = MenuItemBuilder::with_id("menu_help", "Show Help")
        .accelerator("CmdOrCtrl+/")
        .build(app)?;

    // App menu — title is shown on macOS only (the OS forces the bundle name
    // there anyway, so the literal we pass is mostly for non-mac platforms).
    let app_submenu = SubmenuBuilder::new(app, "xangi-pets")
        .item(&about)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&status)
        .item(&port)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&show)
        .item(&hide)
        .item(&talk)
        .item(&open_chat)
        .item(&open_chat_browser)
        .item(&prefs)
        .item(&normal_responses)
        .item(&completion_display)
        .item(&notifications)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&PredefinedMenuItem::quit(app, None)?)
        .build()?;
    // Edit menu: macOS WKWebView only enables Cmd+C / Cmd+V / Cmd+X / Cmd+A
    // when the corresponding PredefinedMenuItem is present somewhere in the
    // menu bar. Without this, the xangi URL modal accepts no clipboard
    // shortcuts (which the user noticed first).
    let edit_submenu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let help_submenu = SubmenuBuilder::new(app, "Help").item(&help).build()?;

    let menu = MenuBuilder::new(app)
        .items(&[&app_submenu, &edit_submenu, &help_submenu])
        .build()?;
    app.set_menu(menu)?;
    let _ = APP_MENU_STATE.set(AppMenuState {
        status,
        port,
        notifications,
        normal_responses,
        completion_display,
    });

    app.on_menu_event(|app, event| {
        handle_control_menu_event(app, event);
    });

    Ok(())
}

fn refresh_app_menu(_app: &AppHandle) {
    let Some(menu) = APP_MENU_STATE.get() else {
        return;
    };
    let (connection, notifications_enabled, normal_responses_enabled, completion_display_enabled) =
        PULL_STATE
            .get()
            .map(|state| {
                (
                    state
                        .connection
                        .lock()
                        .map(|value| *value)
                        .unwrap_or(AppConnectionState::Disconnected),
                    state.notifications_enabled.load(Ordering::Relaxed),
                    state.normal_responses_enabled.load(Ordering::Relaxed),
                    state.completion_display_enabled.load(Ordering::Relaxed),
                )
            })
            .unwrap_or((AppConnectionState::NotConfigured, false, true, true));
    let port = SERVER_URL
        .get()
        .and_then(|url| url.rsplit(':').next())
        .unwrap_or("起動中");
    if let Err(err) = menu
        .status
        .set_text(format!("xangi: {}", connection.label()))
    {
        eprintln!("xangi-pets: failed to refresh app menu status: {err}");
    }
    if let Err(err) = menu.port.set_text(format!("pet server: {port}")) {
        eprintln!("xangi-pets: failed to refresh app menu port: {err}");
    }
    if let Err(err) = menu.notifications.set_checked(notifications_enabled) {
        eprintln!("xangi-pets: failed to refresh app menu notifications: {err}");
    }
    if let Err(err) = menu.normal_responses.set_checked(normal_responses_enabled) {
        eprintln!("xangi-pets: failed to refresh normal response setting: {err}");
    }
    if let Err(err) = menu
        .completion_display
        .set_checked(completion_display_enabled)
    {
        eprintln!("xangi-pets: failed to refresh completion display setting: {err}");
    }
}

/// Returns true when the cursor is *outside* the pet sprite rectangle
/// (i.e. we should keep the window click-through). Returns Err when the
/// cursor position can't be determined — caller treats that as "outside".
fn cursor_outside_pet(window: &tauri::WebviewWindow) -> Result<bool, ()> {
    let cursor = window.cursor_position().map_err(|_| ())?;
    let win_pos = window.outer_position().map_err(|_| ())?;
    let win_size = window.outer_size().map_err(|_| ())?;
    // outer_size / outer_position / cursor_position are all in *physical*
    // pixels, but PET_W/PET_H are CSS (logical) pixels. On a Retina display
    // logical 1px == physical 2px, so we need to scale up the pet rect or
    // half of it gets reported as outside the hit area (= click-through).
    let sf = window.scale_factor().unwrap_or(1.0);
    let pet_w = PET_W.load(Ordering::Relaxed);
    let pet_h = PET_H.load(Ordering::Relaxed);
    let pet_w_px = (pet_w as f64 * sf) as i32;
    let pet_h_px = (pet_h as f64 * sf) as i32;

    // Pet is centered horizontally at the bottom of the window
    // (matches the CSS: body { flex-direction: column; align-items: center;
    //  justify-content: flex-end }).
    let cx_window = win_pos.x + (win_size.width as i32) / 2;
    let bottom = win_pos.y + win_size.height as i32;
    let x_min = cx_window - pet_w_px / 2;
    let x_max = x_min + pet_w_px;
    let y_max = bottom;
    let y_min = bottom - pet_h_px;

    let cx = cursor.x as i32;
    let cy = cursor.y as i32;
    let inside = cx >= x_min && cx <= x_max && cy >= y_min && cy <= y_max;
    Ok(!inside)
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_xangi_url, notification_transition, open_web_chat_new_window,
        truncate_notification,
    };
    use serde_json::json;
    use std::collections::HashSet;
    use tauri::webview::NewWindowResponse;

    #[test]
    fn xangi_url_accepts_http_and_strips_query_and_fragment() {
        assert_eq!(
            normalize_xangi_url(" https://pet.example.test:8443/chat/?token=secret#part ").unwrap(),
            "https://pet.example.test:8443/chat"
        );
        assert_eq!(
            normalize_xangi_url("http://127.0.0.1:18888/").unwrap(),
            "http://127.0.0.1:18888"
        );
    }

    #[test]
    fn xangi_url_rejects_unsafe_schemes_and_userinfo() {
        assert!(normalize_xangi_url("file:///tmp/xangi").is_err());
        assert!(normalize_xangi_url("https://user:pass@example.test").is_err());
        assert!(normalize_xangi_url("not-a-url").is_err());
    }

    #[test]
    fn notifications_only_fire_once_for_observed_turns() {
        let mut turns = HashSet::new();
        let complete = json!({
            "type": "turn.complete",
            "thread_id": "discord:1",
            "turn_id": "turn-1",
            "text": "done"
        });
        assert!(!notification_transition(&mut turns, &complete));
        assert!(!notification_transition(
            &mut turns,
            &json!({
                "type": "turn.started",
                "thread_id": "discord:1",
                "turn_id": "turn-1"
            })
        ));
        assert!(notification_transition(&mut turns, &complete));
        assert!(!notification_transition(&mut turns, &complete));
    }

    #[test]
    fn aborted_turns_never_notify_and_error_turns_notify_once() {
        let mut turns = HashSet::new();
        let start = json!({
            "type": "turn.started",
            "thread_id": "web:1",
            "turn_id": "turn-2"
        });
        assert!(!notification_transition(&mut turns, &start));
        assert!(!notification_transition(
            &mut turns,
            &json!({
                "type": "turn.aborted",
                "thread_id": "web:1",
                "turn_id": "turn-2"
            })
        ));
        assert!(!notification_transition(&mut turns, &start));
        assert!(notification_transition(
            &mut turns,
            &json!({
                "type": "agent.error",
                "thread_id": "web:1",
                "turn_id": "turn-2",
                "message": "boom"
            })
        ));
    }

    #[test]
    fn notification_body_is_unicode_safe() {
        assert_eq!(truncate_notification("あいう", 2), "あい…");
        assert_eq!(truncate_notification("あいう", 3), "あいう");
    }

    #[test]
    fn web_chat_new_window_opens_http_url_externally_and_denies_webview_window() {
        let url = tauri::Url::parse("https://example.test/docs?q=xangi").unwrap();
        let mut opened = None;

        let response = open_web_chat_new_window(url, |url| {
            opened = Some(url.to_owned());
            Ok::<(), &str>(())
        });

        assert_eq!(opened.as_deref(), Some("https://example.test/docs?q=xangi"));
        assert!(matches!(response, NewWindowResponse::Deny));
    }

    #[test]
    fn web_chat_new_window_rejects_unsafe_scheme() {
        let url = tauri::Url::parse("javascript:alert('xangi')").unwrap();
        let mut called = false;

        let response = open_web_chat_new_window(url, |_| {
            called = true;
            Ok::<(), &str>(())
        });

        assert!(!called);
        assert!(matches!(response, NewWindowResponse::Deny));
    }
}
