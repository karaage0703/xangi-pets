use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionSource {
    Chat,
    Web,
    Scheduler,
}

pub fn defaults() -> Vec<SessionSource> {
    vec![SessionSource::Chat, SessionSource::Web]
}

pub fn load(path: &Path) -> Vec<SessionSource> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            eprintln!("xangi-pets: invalid session filters: {error}");
            defaults()
        }),
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                eprintln!("xangi-pets: session filter read failed: {error}");
            }
            defaults()
        }
    }
}

pub fn save(path: &Path, filters: &[SessionSource]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, serde_json::to_vec(filters)?)?;
    fs::rename(&temporary, path)
}

/// Use execution origin before platform: Feed jobs use Web conversations.
pub fn source(row: &serde_json::Value) -> SessionSource {
    if row["scope"] == "scheduler" || row["executionOrigin"] == "automation" {
        SessionSource::Scheduler
    } else if row["platform"] == "web" {
        SessionSource::Web
    } else {
        SessionSource::Chat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_survive_restart_including_all_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings/session-filters.json");
        assert_eq!(load(&path), defaults());
        for filters in [vec![SessionSource::Scheduler], vec![], defaults()] {
            save(&path, &filters).unwrap();
            assert_eq!(load(&path), filters);
        }
        fs::write(&path, r#"["unknown"]"#).unwrap();
        assert_eq!(load(&path), defaults());
    }

    #[test]
    fn failed_save_preserves_saved_selection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-filters.json");
        save(&path, &defaults()).unwrap();
        fs::create_dir(path.with_extension(format!("{}.tmp", std::process::id()))).unwrap();
        assert!(save(&path, &[]).is_err());
        assert_eq!(load(&path), defaults());
    }
}
