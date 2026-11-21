use serde::Serialize;
use std::{fs, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager};
use xangi_events_server::display::{next_scale, valid_scale};

#[derive(Clone, Copy, Serialize)]
pub struct PetDisplay {
    pub scale: f64,
    pub revision: u64,
    pub persisted: bool,
}

static DISPLAY: Mutex<PetDisplay> = Mutex::new(PetDisplay {
    scale: 1.0,
    revision: 0,
    persisted: false,
});
static UPDATE: Mutex<()> = Mutex::new(());

pub fn initialize(app: &AppHandle) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    match fs::read(dir.join("pet-display.json")) {
        Ok(bytes) => {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(scale) = value["scale"].as_f64().filter(|scale| valid_scale(*scale)) {
                    *DISPLAY.lock().unwrap() = PetDisplay {
                        scale,
                        revision: 0,
                        persisted: true,
                    };
                }
            }
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            eprintln!("xangi-pets: display preference read failed: {error}")
        }
        _ => {}
    }
}

pub fn get() -> PetDisplay {
    *DISPLAY.lock().unwrap()
}

pub fn with_current<R>(operation: impl FnOnce(PetDisplay) -> R) -> R {
    let _update = UPDATE.lock().unwrap();
    operation(get())
}

pub fn update(app: &AppHandle, requested: Option<f64>) -> Result<PetDisplay, String> {
    let _update = UPDATE.lock().map_err(|e| e.to_string())?;
    let mut current = DISPLAY.lock().map_err(|e| e.to_string())?;
    let scale = requested.unwrap_or_else(|| next_scale(current.scale));
    if !valid_scale(scale) {
        return Err("invalid pet scale".into());
    }
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("pet-display.json");
    let temporary = dir.join(format!("pet-display-{}.tmp", std::process::id()));
    fs::write(&temporary, serde_json::json!({"scale":scale}).to_string())
        .map_err(|e| e.to_string())?;
    fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
    *current = PetDisplay {
        scale,
        revision: current.revision + 1,
        persisted: true,
    };
    let result = *current;
    drop(current);
    if let Some(state) = super::PULL_STATE.get() {
        state.session_windows.resize_all(scale);
    }
    app.emit("pet://scale-changed", result)
        .map_err(|e| e.to_string())?;
    Ok(result)
}
