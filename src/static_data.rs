use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const REPO_URL_BASE: &str = "https://raw.githubusercontent.com/AnonMiraj/Tanin/main/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sound {
    pub id: String,
    pub name: String,
    pub category: String,
    pub file_path: String,
    #[serde(default = "default_volume")]
    pub volume_linear: f32,
    #[serde(default = "default_icon")]
    pub icon: String,
    pub url: Option<String>,
    #[serde(skip)]
    pub error_state: bool,
}

fn default_volume() -> f32 {
    0.5
}

fn default_icon() -> String {
    "🎵".to_string()
}

#[derive(Debug, Deserialize)]
struct SoundEntry {
    name: Option<String>,
    file: Option<String>,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default = "default_icon")]
    pub icon: String,
    pub url: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum AssetStatus {
    Present,
    Missing,
}

pub fn check_assets() -> AssetStatus {
    if get_active_assets_path().is_some() {
        AssetStatus::Present
    } else {
        AssetStatus::Missing
    }
}

pub fn get_active_assets_path() -> Option<PathBuf> {
    // 1. Check local (dev/portable)
    let local = PathBuf::from("assets/sounds.toml");
    if local.exists() {
        return Some(local);
    }

    // 2. Check user data (downloaded)
    if let Some(proj_dirs) = ProjectDirs::from("com", "tanin", "tanin") {
        let user = proj_dirs.data_dir().join("assets").join("sounds.toml");
        if user.exists() {
            return Some(user);
        }
    }

    // 3. Check system (AUR/Global)
    let system = PathBuf::from("/usr/share/tanin/assets/sounds.toml");
    if system.exists() {
        return Some(system);
    }

    None
}

pub fn get_bundled_sounds() -> Vec<Sound> {
    if let Some(path) = get_active_assets_path() {
        match load_sounds_from_file(&path) {
            Ok(sounds) => sounds,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to load bundled sounds from {:?}: {}. Loading empty list.",
                    path, e
                );
                Vec::new()
            }
        }
    } else {
        // No assets found
        Vec::new()
    }
}

pub fn load_custom_sounds() -> Vec<Sound> {
    let path = if let Some(proj_dirs) = ProjectDirs::from("com", "tanin", "tanin") {
        proj_dirs.config_dir().join("sounds.toml")
    } else {
        PathBuf::from("custom_sounds.toml")
    };

    if !path.exists() {
        return Vec::new();
    }

    match load_sounds_from_file(&path) {
        Ok(sounds) => sounds,
        Err(e) => {
            eprintln!(
                "Warning: Failed to load custom sounds from {:?}: {}",
                path, e
            );
            Vec::new()
        }
    }
}

pub fn load_sounds_from_file<P: AsRef<Path>>(path: P) -> Result<Vec<Sound>> {
    let path = path.as_ref();
    let content = fs::read_to_string(path).context("Could not read sounds configuration file")?;
    let root: toml::Table =
        toml::from_str(&content).context("Could not parse sounds configuration file")?;

    let config_dir = path.parent().unwrap_or(Path::new("."));

    // We assume sounds are in a "sounds" subdirectory relative to the toml file
    // This unifies logic for local, system, and user-downloaded assets.
    // We ignore the 'base_path' in the TOML unless it's absolute.

    let base_path_param = root
        .get("base_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches('/').to_string());

    let mut sounds = Vec::new();

    for (category_name, category_value) in &root {
        if category_name == "base_path" {
            continue;
        }

        if let Some(sound_map) = category_value.as_table() {
            for (sound_id, sound_data) in sound_map {
                let entry: SoundEntry = sound_data
                    .clone()
                    .try_into()
                    .map_err(|e| anyhow::anyhow!("Failed to parse sound '{}': {}", sound_id, e))?;

                let name = entry
                    .name
                    .clone()
                    .unwrap_or_else(|| sound_id.replace("_", " "));

                let filename = entry.file.clone().unwrap_or_else(|| {
                    let slug = name.to_lowercase().replace(" ", "_");
                    format!("{}.ogg", slug)
                });

                let file_path = if Path::new(&filename).is_absolute() {
                    filename
                } else if let Some(base) = &base_path_param {
                    if Path::new(base).is_absolute() {
                        Path::new(base)
                            .join(&filename)
                            .to_string_lossy()
                            .to_string()
                    } else {
                        // Default behavior: expect 'sounds' dir sibling to toml
                        config_dir
                            .join("sounds")
                            .join(&filename)
                            .to_string_lossy()
                            .to_string()
                    }
                } else {
                    config_dir
                        .join("sounds")
                        .join(&filename)
                        .to_string_lossy()
                        .to_string()
                };

                sounds.push(Sound {
                    id: sound_id.clone(),
                    name,
                    category: category_name.clone(),
                    file_path,
                    volume_linear: entry.volume,
                    icon: entry.icon,
                    url: entry.url,
                    error_state: false,
                });
            }
        }
    }

    // Sort for consistent order
    sounds.sort_by(|a, b| {
        let cat_cmp = a.category.cmp(&b.category);
        if cat_cmp == std::cmp::Ordering::Equal {
            a.id.cmp(&b.id)
        } else {
            cat_cmp
        }
    });

    Ok(sounds)
}

pub fn add_custom_sound(
    name: &str,
    category: &str,
    file_path: &str,
    icon: &str,
    url: Option<&str>,
) -> Result<()> {
    let toml_path = if let Some(proj_dirs) = ProjectDirs::from("com", "tanin", "tanin") {
        let config_dir = proj_dirs.config_dir();
        if !config_dir.exists() {
            fs::create_dir_all(config_dir)?;
        }
        config_dir.join("sounds.toml")
    } else {
        PathBuf::from("custom_sounds.toml")
    };

    let mut root: toml::Table = if toml_path.exists() {
        let content = fs::read_to_string(&toml_path)?;
        toml::from_str(&content).unwrap_or_else(|_| toml::Table::new())
    } else {
        toml::Table::new()
    };

    let category_entry = root
        .entry(category)
        .or_insert(toml::Value::Table(toml::Table::new()));

    if let toml::Value::Table(cat_table) = category_entry {
        let id = name.to_lowercase().replace(" ", "_");

        let mut sound_entry = toml::Table::new();
        sound_entry.insert(
            "file".to_string(),
            toml::Value::String(file_path.to_string()),
        );
        sound_entry.insert("icon".to_string(), toml::Value::String(icon.to_string()));
        sound_entry.insert("volume".to_string(), toml::Value::Float(0.5));

        if let Some(u) = url {
            sound_entry.insert("url".to_string(), toml::Value::String(u.to_string()));
        }

        cat_table.insert(id, toml::Value::Table(sound_entry));
    }

    let output = toml::to_string_pretty(&root)?;
    fs::write(toml_path, output)?;

    Ok(())
}

pub fn download_config() -> Result<Vec<Sound>> {
    let proj_dirs =
        ProjectDirs::from("com", "tanin", "tanin").context("No home directory found")?;
    let assets_dir = proj_dirs.data_dir().join("assets");
    let sounds_dir = assets_dir.join("sounds");

    fs::create_dir_all(&sounds_dir)?;

    // Download sounds.toml
    let toml_url = format!("{}assets/sounds.toml", REPO_URL_BASE);
    let toml_resp = ureq::get(&toml_url).call()?;

    let mut reader = toml_resp.into_reader();
    let toml_path = assets_dir.join("sounds.toml");
    let mut file = fs::File::create(&toml_path)?;
    std::io::copy(&mut reader, &mut file)?;

    // Load and return sounds
    load_sounds_from_file(&toml_path)
}
