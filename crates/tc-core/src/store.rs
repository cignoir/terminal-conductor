//! JSON persistence with atomic writes, plus the per-pane Claude session map.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn load_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Writes to a temp file next to `path`, then renames over it.
pub fn save_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,
    /// Unix millis of the last hook event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<u64>,
}

pub struct SessionStore {
    path: PathBuf,
    pub map: HashMap<String, SessionInfo>,
    last_saved_ms: u64,
}

impl SessionStore {
    pub fn open(path: PathBuf) -> Self {
        let map = load_json(&path).unwrap_or_default();
        Self {
            path,
            map,
            last_saved_ms: 0,
        }
    }

    pub fn save(&mut self) {
        let _ = save_json(&self.path, &self.map);
        self.last_saved_ms = now_ms();
    }

    /// Saves if `important`, or at most every 30 seconds otherwise.
    pub fn save_throttled(&mut self, important: bool) {
        if important || now_ms().saturating_sub(self.last_saved_ms) > 30_000 {
            self.save();
        }
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
