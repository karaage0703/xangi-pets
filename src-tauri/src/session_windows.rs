//! Temporary, display-only windows. The permanent `pet` window is never in
//! this registry and cannot be closed by session reconciliation.
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use xangi_events_server::{
    agents::{self, AgentPet},
    session_monitor::SessionPet,
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionContext {
    pub pet_display: super::display_settings::PetDisplay,
    pub session: SessionPet,
    pub agent_name: String,
    pub instance_name: String,
    pub sprite_directory: String,
    pub server_url: String,
    pub revision: u64,
    #[serde(skip)]
    base_url: String,
}

struct Entry {
    label: String,
    context: SessionContext,
    slot: usize,
    ready: bool,
}

pub struct SessionWindows {
    app: AppHandle,
    entries: Mutex<BTreeMap<String, Entry>>,
    sequence: AtomicU64,
    visible: AtomicBool,
}

impl SessionWindows {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            entries: Mutex::new(BTreeMap::new()),
            sequence: AtomicU64::new(1),
            visible: AtomicBool::new(true),
        }
    }

    pub fn context(&self, label: &str) -> Result<SessionContext, String> {
        self.entries
            .lock()
            .map_err(|e| e.to_string())?
            .values()
            .find(|entry| entry.label == label)
            .map(|entry| {
                let mut context = entry.context.clone();
                context.pet_display = super::display_settings::get();
                context
            })
            .ok_or_else(|| "セッションのペットは終了しました".into())
    }

    pub async fn sprite(&self, label: &str) -> Result<AgentPet, String> {
        let context = self.context(label)?;
        agents::load_agent_pet(
            &context.base_url,
            &context.session.agent_id,
            &context.sprite_directory,
        )
        .await
    }

    pub fn ready(&self, window: &WebviewWindow) -> Result<(), String> {
        self.context(window.label())?;
        super::display_settings::with_current(|display| {
            resize_window(window, display.scale);
            if let Some(entry) = self
                .entries
                .lock()
                .unwrap()
                .values_mut()
                .find(|entry| entry.label == window.label())
            {
                entry.ready = true;
            }
            if self.visible.load(Ordering::Relaxed) {
                window.show().map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }

    pub fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::Relaxed);
        let labels: Vec<_> = self
            .entries
            .lock()
            .unwrap()
            .values()
            .filter(|entry| entry.ready)
            .map(|entry| entry.label.clone())
            .collect();
        for label in labels {
            if let Some(window) = self.app.get_webview_window(&label) {
                if visible {
                    let _ = window.show();
                } else {
                    let _ = window.hide();
                }
            }
        }
    }

    pub fn clear_source(&self, source: &str) {
        let retired = {
            let mut entries = self.entries.lock().unwrap();
            let retired: Vec<_> = entries
                .values()
                .filter(|entry| entry.context.base_url == source)
                .map(|entry| entry.label.clone())
                .collect();
            entries.retain(|_, entry| entry.context.base_url != source);
            retired
        };
        for label in retired {
            self.close(&label);
        }
    }

    fn close(&self, label: &str) {
        if let Some(window) = self.app.get_webview_window(label) {
            if let Err(error) = window.destroy() {
                eprintln!("xangi-pets: close session window failed: {error}");
            }
        }
    }

    pub fn reconcile(
        &self,
        base_url: &str,
        directories: &BTreeMap<String, String>,
        instance_name: &str,
        pets: Vec<SessionPet>,
    ) {
        let desired: HashSet<_> = pets
            .iter()
            .map(|pet| {
                xangi_events_server::instances::pet_key(base_url, &pet.session_id, pet.persistent)
            })
            .collect();
        let retired = {
            let mut entries = self.entries.lock().unwrap();
            let retired: Vec<_> = entries
                .iter()
                .filter(|(id, entry)| entry.context.base_url == base_url && !desired.contains(*id))
                .map(|(_, entry)| entry.label.clone())
                .collect();
            entries.retain(|id, entry| entry.context.base_url != base_url || desired.contains(id));
            retired
        };
        for label in retired {
            self.close(&label);
        }

        for pet in pets {
            let key =
                xangi_events_server::instances::pet_key(base_url, &pet.session_id, pet.persistent);
            let (label, context, needs_window, slot) = {
                let mut entries = self.entries.lock().unwrap();
                if let Some(entry) = entries.get(&key) {
                    if entry.context.session == pet
                        && self.app.get_webview_window(&entry.label).is_some()
                    {
                        continue;
                    }
                }
                let revision = self.sequence.fetch_add(1, Ordering::Relaxed);
                let label = entries
                    .get(&key)
                    .map(|entry| entry.label.clone())
                    .unwrap_or_else(|| format!("session-pet-{revision}"));
                let context = SessionContext {
                    pet_display: super::display_settings::get(),
                    sprite_directory: directories
                        .get(&pet.agent_id)
                        .cloned()
                        .unwrap_or_else(|| "assets/xangi-pets".into()),
                    agent_name: pet.agent_name.clone(),
                    instance_name: instance_name.into(),
                    session: pet,
                    server_url: super::SERVER_URL.get().cloned().unwrap_or_default(),
                    base_url: base_url.into(),
                    revision,
                };
                let slot = entries
                    .get(&key)
                    .map(|entry| entry.slot)
                    .unwrap_or_else(|| {
                        (0..)
                            .find(|slot| entries.values().all(|entry| entry.slot != *slot))
                            .unwrap()
                    });
                let ready = entries.get(&key).is_some_and(|entry| entry.ready)
                    && self.app.get_webview_window(&label).is_some();
                entries.insert(
                    key.clone(),
                    Entry {
                        label: label.clone(),
                        context: context.clone(),
                        slot,
                        ready,
                    },
                );
                let needs_window = self.app.get_webview_window(&label).is_none();
                (label, context, needs_window, slot)
            };
            if needs_window {
                if let Err(error) = self.open(&label, slot) {
                    self.entries.lock().unwrap().remove(&key);
                    eprintln!("xangi-pets: open session window failed: {error}");
                    let _ = self.app.emit_to("pet", "pet://session-window-error", error);
                }
            } else {
                let _ = self.app.emit_to(&label, "pet://session-context", context);
            }
        }
    }

    fn open(&self, label: &str, slot: usize) -> Result<(), String> {
        let (x, y) = self.position(slot);
        let (_, _, width, height) =
            xangi_events_server::display::session_geometry(super::display_settings::get().scale);
        let window = WebviewWindowBuilder::new(
            &self.app,
            label,
            WebviewUrl::App("index.html?sessionPet=1".into()),
        )
        .title("xangi-pets · session")
        .inner_size(width as f64, height as f64)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .resizable(false)
        .skip_taskbar(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
        let _ = window.set_ignore_cursor_events(true);
        spawn_hit_polling(window);
        Ok(())
    }

    fn position(&self, slot: usize) -> (f64, f64) {
        let (_, _, width, height) =
            xangi_events_server::display::session_geometry(super::display_settings::get().scale);
        let width = width as f64;
        let height = height as f64;
        let Some(main) = self.app.get_webview_window("pet") else {
            return (40.0, 40.0);
        };
        let Ok(Some(monitor)) = main.current_monitor() else {
            return (40.0 + (slot % 5) as f64 * width, 40.0);
        };
        let scale = monitor.scale_factor();
        let origin = monitor.position();
        let size = monitor.size();
        let columns = ((size.width as f64 / scale - 24.0) / width)
            .floor()
            .max(1.0) as usize;
        let rows = ((size.height as f64 / scale - 80.0) / height)
            .floor()
            .max(1.0) as usize;
        let index = slot % (columns * rows);
        (
            origin.x as f64 / scale + 12.0 + (index % columns) as f64 * width,
            origin.y as f64 / scale + 40.0 + (index / columns) as f64 * height,
        )
    }

    pub fn resize_all(&self, scale: f64) {
        let labels: Vec<_> = self
            .entries
            .lock()
            .unwrap()
            .values()
            .map(|entry| entry.label.clone())
            .collect();
        for label in labels {
            if let Some(window) = self.app.get_webview_window(&label) {
                resize_window(&window, scale);
            }
        }
    }
}

fn resize_window(window: &WebviewWindow, scale: f64) {
    let (_, _, w, h) = xangi_events_server::display::session_geometry(scale);
    let Ok(old) = window.outer_size() else {
        return;
    };
    let Ok(position) = window.outer_position() else {
        return;
    };
    let factor = window.scale_factor().unwrap_or(1.0);
    let nw = (w as f64 * factor).round() as i32;
    let nh = (h as f64 * factor).round() as i32;
    let mut x = position.x + (old.width as i32 - nw) / 2;
    let mut y = position.y + old.height as i32 - nh;
    if let Ok(Some(monitor)) = window.current_monitor() {
        let origin = monitor.position();
        let size = monitor.size();
        x = x.clamp(origin.x, (origin.x + size.width as i32 - nw).max(origin.x));
        y = y.clamp(origin.y, (origin.y + size.height as i32 - nh).max(origin.y));
    }
    let _ = window.set_size(tauri::LogicalSize::new(w as f64, h as f64));
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

fn spawn_hit_polling(window: WebviewWindow) {
    tauri::async_runtime::spawn(async move {
        let mut last = None;
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let Ok(position) = window.outer_position() else {
                break;
            };
            let Ok(size) = window.outer_size() else {
                break;
            };
            let ignore = match window.cursor_position() {
                Ok(cursor) => {
                    let scale = window.scale_factor().unwrap_or(1.0);
                    let (pet_w, pet_h, _, _) = xangi_events_server::display::session_geometry(
                        super::display_settings::get().scale,
                    );
                    let hit_w = (pet_w as f64).max(180.0);
                    let hit_h = pet_h as f64 + 60.0;
                    let x = cursor.x - position.x as f64;
                    let y = cursor.y - position.y as f64;
                    x < (size.width as f64 - hit_w * scale) / 2.0
                        || x > (size.width as f64 + hit_w * scale) / 2.0
                        || y < size.height as f64 - hit_h * scale
                        || y > size.height as f64
                }
                Err(_) => true,
            };
            if last != Some(ignore) {
                if window.set_ignore_cursor_events(ignore).is_err() {
                    break;
                }
                last = Some(ignore);
            }
        }
    });
}
