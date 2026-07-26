use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{Emitter, Manager};
use xangi_events_server::{
    make_state_with_extra_pet_dirs, post_pet_message, spawn_pull_client, AppState,
    PullClientHandle,
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
    handle: Mutex<Option<PullClientHandle>>,
    current_url: Mutex<Option<String>>,
}

static PULL_STATE: OnceLock<Arc<PullState>> = OnceLock::new();

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
            let extra_pet_dirs: Vec<std::path::PathBuf> =
                bundled_pet_dir.into_iter().collect();
            let state = make_state_with_extra_pet_dirs(extra_pet_dirs);
            let pet_dirs = state.pet_dirs.clone();

            // Stash the bus-bearing state so the Tauri commands below can
            // reach it (set_xangi_url restarts the pull client by handing
            // this state to spawn_pull_client again).
            let pull_state = Arc::new(PullState {
                app: state.clone(),
                handle: Mutex::new(None),
                current_url: Mutex::new(None),
            });
            let _ = PULL_STATE.set(pull_state.clone());

            let server_state = state.clone();
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
                if let Err(err) =
                    xangi_events_server::serve_listener(listener, server_state).await
                {
                    eprintln!("xangi-events server error: {err}");
                }
            });

            // Optionally bootstrap the pull client from $XANGI_URL so that
            // CI / dev runs can start receiving without going through the
            // webview onboarding UI. Frontend can override later via
            // `set_xangi_url`.
            if let Ok(url) = std::env::var("XANGI_URL") {
                let url = url.trim().to_string();
                if !url.is_empty() {
                    start_pull_client(&pull_state, url);
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
            send_pet_message
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
    let trimmed = url.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err("xangi URL is empty".into());
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(format!("xangi URL must start with http:// or https:// (got {trimmed})"));
    }
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
    let resp = post_pet_message(&base_url, &text, token_ref)
        .await
        .map_err(|e| e.to_string())?;
    Ok(resp.thread_id.unwrap_or_default())
}

/// Stop the pull client and forget the configured URL. Used when the
/// frontend wants to show the onboarding screen again.
#[tauri::command]
fn clear_xangi_url() {
    if let Some(pull_state) = PULL_STATE.get() {
        // Cancel the running task without blocking the command. The async
        // shutdown will run on the tokio pool.
        let pull_state = pull_state.clone();
        tauri::async_runtime::spawn(async move {
            stop_pull_client(&pull_state).await;
            if let Ok(mut url) = pull_state.current_url.lock() {
                *url = None;
            }
        });
    }
}

/// Append `/api/events/stream` to a base URL and start (or restart) the
/// pull client. Synchronous: the actual stream-reading runs on a tokio
/// task, but we replace the handle synchronously so subsequent calls see
/// the new handle.
fn start_pull_client(pull_state: &Arc<PullState>, base_url: String) {
    let stream_url = format!("{base_url}/api/events/stream");
    println!("xangi-events pull: subscribing to {stream_url}");

    // Stop the previous client (if any) before starting a new one.
    let prev_handle = pull_state
        .handle
        .lock()
        .ok()
        .and_then(|mut g| g.take());
    if let Some(prev) = prev_handle {
        let pull_state = pull_state.clone();
        tauri::async_runtime::spawn(async move {
            prev.shutdown().await;
            // Drop only happens after shutdown returns.
            drop(pull_state);
        });
    }

    let new_handle = spawn_pull_client(pull_state.app.clone(), stream_url);
    if let Ok(mut g) = pull_state.handle.lock() {
        *g = Some(new_handle);
    }
    if let Ok(mut g) = pull_state.current_url.lock() {
        *g = Some(base_url);
    }
}

async fn stop_pull_client(pull_state: &Arc<PullState>) {
    let prev = pull_state
        .handle
        .lock()
        .ok()
        .and_then(|mut g| g.take());
    if let Some(h) = prev {
        h.shutdown().await;
    }
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

/// Build the macOS menu bar and wire up menu events. On macOS the first
/// submenu becomes the application menu automatically; we put About /
/// Preferences / Quit there. A separate "Help" submenu hosts the
/// keybindings overlay trigger so users who don't know the `h` key can find
/// it from the menu bar.
fn install_app_menu(app: &tauri::App) -> tauri::Result<()> {
    let about = MenuItemBuilder::with_id("menu_about", "About xangi-pets").build(app)?;
    let prefs = MenuItemBuilder::with_id("menu_prefs", "Preferences (xangi URL)…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let help = MenuItemBuilder::with_id("menu_help", "Show Help")
        .accelerator("CmdOrCtrl+/")
        .build(app)?;

    // App menu — title is shown on macOS only (the OS forces the bundle name
    // there anyway, so the literal we pass is mostly for non-mac platforms).
    let app_submenu = SubmenuBuilder::new(app, "xangi-pets")
        .item(&about)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&prefs)
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

    app.on_menu_event(|app, event| {
        // Frontend listens for `pet://show-help` and `pet://set-xangi-url`
        // (see src/main.js — same handlers used for the `h` and `x` keys).
        // About reuses the help overlay because that's where the version /
        // current state / GitHub link live; building a separate About panel
        // would just duplicate that.
        let id = event.id().as_ref();
        let event_name = match id {
            "menu_about" => "pet://show-help",
            "menu_prefs" => "pet://set-xangi-url",
            "menu_help" => "pet://show-help",
            _ => return,
        };
        if let Err(err) = app.emit(event_name, ()) {
            eprintln!("xangi-pets: emit {event_name} failed: {err}");
        }
    });

    Ok(())
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
