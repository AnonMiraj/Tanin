use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub global_volume: f32,
    pub sounds: HashMap<String, SoundState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundState {
    pub enabled: bool,
    pub volume: f32,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            global_volume: 0.5,
            sounds: HashMap::new(),
        }
    }
}

impl Session {
    pub fn load() -> Result<Self> {
        let path = get_session_path()?;
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            // If it fails to parse (e.g. empty or corrupted), return default instead of crashing
            // because session state is disposable.
            let session: Session = toml::from_str(&content).unwrap_or_default();
            Ok(session)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = get_session_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}

fn get_session_path() -> Result<PathBuf> {
    if let Some(proj_dirs) = ProjectDirs::from("com", "tanin", "tanin") {
        Ok(proj_dirs.cache_dir().join("session.toml"))
    } else {
        Ok(PathBuf::from("session.toml"))
    }
}
