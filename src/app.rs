pub mod audio;
pub mod download;
pub mod input;
pub mod navigation;
pub mod presets;

use crate::audio::AudioEngine;
use crate::config::Config;
use crate::presets::PresetsConfig;
use crate::session::{Session, SoundState};
use crate::static_data::{check_assets, get_bundled_sounds, AssetStatus, Sound};
use anyhow::Result;
pub use download::{DownloadEvent, DownloadStatus, DownloadTask};
use std::sync::mpsc::Receiver;

pub enum AssetDownloadEvent {
    ConfigDownloaded(Vec<Sound>),
    Error(String),
}

#[derive(PartialEq)]
pub enum CurrentView {
    Main,
    Presets,
    Help,
    Downloads,
    AssetMissing,
    DownloadingAssets,
}

pub struct App {
    pub sounds: Vec<Sound>,
    pub cursor_pos: usize,
    pub view: CurrentView,
    pub audio_engine: Option<AudioEngine>,
    pub config: Config,
    pub session: Session,
    pub presets_config: PresetsConfig,
    pub quitting: bool,
    pub grid_cols: u16,
    pub width: u16,
    pub height: u16,
    pub muted: bool,
    pub previous_volume: f32,
    pub grid_scroll: u16,

    // Preset view state
    pub preset_cursor_pos: usize,
    pub preset_input_mode: bool,
    pub preset_input_buffer: String,
    pub preset_rename_target: Option<usize>,
    pub active_preset: Option<String>,
    pub animation_offset: f32,

    // Add Sound view state
    pub add_sound_name: String,
    pub add_sound_category: String,
    pub add_sound_icon: String,
    pub add_sound_url: String,
    pub add_sound_focus_index: usize, // 0: Name, 1: Category, 2: Icon, 3: URL
    pub add_sound_status: String,
    pub add_sound_suggestion: Option<String>,

    // Search state
    pub search_query: String,
    pub search_mode: bool,

    // Download Queue
    pub yt_dlp_available: bool,
    pub download_queue: Vec<DownloadTask>,
    pub active_download_index: Option<usize>,
    pub download_rx: Option<Receiver<DownloadEvent>>,

    // Asset Download
    pub asset_download_rx: Option<Receiver<AssetDownloadEvent>>,
    pub asset_download_error: Option<String>,
}

impl App {
    pub fn new() -> Result<Self> {
        let config = Config::load()?;
        let session = Session::load()?;
        let presets_config = PresetsConfig::load().unwrap_or_default();

        let audio_engine = AudioEngine::new().ok();

        // Check yt-dlp availability
        let yt_dlp_available = std::process::Command::new("yt-dlp")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let mut app = Self {
            sounds: Vec::new(),
            cursor_pos: 0,
            view: CurrentView::Main,
            audio_engine,
            config: config.clone(),
            session: session.clone(),
            presets_config,
            quitting: false,
            grid_cols: 1,
            width: 80,
            height: 24,
            muted: false,
            previous_volume: session.global_volume,
            grid_scroll: 0,
            preset_cursor_pos: 0,
            preset_input_mode: false,
            preset_input_buffer: String::new(),
            preset_rename_target: None,
            active_preset: None,
            animation_offset: 0.0,
            add_sound_name: String::new(),
            add_sound_category: String::new(),
            add_sound_icon: "🎵".to_string(),
            add_sound_url: String::new(),
            add_sound_focus_index: 0,
            add_sound_status: String::new(),
            add_sound_suggestion: None,

            search_query: String::new(),
            search_mode: false,

            yt_dlp_available,
            download_queue: Vec::new(),
            active_download_index: None,
            download_rx: None,

            asset_download_rx: None,
            asset_download_error: None,
        };

        if check_assets() == AssetStatus::Missing {
            app.view = CurrentView::AssetMissing;
        } else if config.general.enable_bundled_sounds {
            app.sounds.extend(get_bundled_sounds());
        }

        app.sounds.extend(crate::static_data::load_custom_sounds());

        // Sort all sounds to ensure categories are grouped correctly (merging bundled + custom)
        app.sort_sounds();

        app.check_and_download_missing_files();

        // Apply config
        if let Some(engine) = &mut app.audio_engine {
            engine.set_master_volume(session.global_volume);

            for sound in &mut app.sounds {
                if let Some(sc) = session.sounds.get(&sound.id) {
                    sound.volume_linear = sc.volume;
                    if sc.enabled {
                        if let Err(e) =
                            engine.play(&sound.id, &sound.file_path, sound.volume_linear)
                        {
                            log::error!("Failed to auto-play sound '{}': {}", sound.id, e);
                            sound.error_state = true;
                        }
                    }
                }
            }
        }

        Ok(app)
    }

    pub fn start_asset_download(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.asset_download_rx = Some(rx);
        self.view = CurrentView::DownloadingAssets;
        self.asset_download_error = None;

        std::thread::spawn(move || match crate::static_data::download_config() {
            Ok(sounds) => {
                let _ = tx.send(AssetDownloadEvent::ConfigDownloaded(sounds));
            }
            Err(e) => {
                let _ = tx.send(AssetDownloadEvent::Error(e.to_string()));
            }
        });
    }

    pub fn update(&mut self, dt: std::time::Duration) {
        if let Some(rx) = &self.asset_download_rx {
            loop {
                match rx.try_recv() {
                    Ok(AssetDownloadEvent::ConfigDownloaded(sounds)) => {
                        self.asset_download_rx = None;

                        // Populate queue
                        for sound in sounds {
                            if !std::path::Path::new(&sound.file_path).exists() {
                                if let Some(url) = &sound.url {
                                    self.download_queue.push(DownloadTask {
                                        name: sound.name.clone(),
                                        category: sound.category.clone(),
                                        url: url.clone(),
                                        status: DownloadStatus::Pending,
                                        icon: sound.icon.clone(),
                                        target_filename: std::path::Path::new(&sound.file_path)
                                            .file_name()
                                            .map(|s| s.to_string_lossy().to_string()),
                                    });
                                }
                            }
                        }

                        // Reload sounds to pick up the new config
                        if self.config.general.enable_bundled_sounds {
                            self.sounds = get_bundled_sounds();
                        }
                        self.sounds.extend(crate::static_data::load_custom_sounds());
                        self.sort_sounds();

                        // Switch to Downloads view
                        self.view = CurrentView::Downloads;
                        // Force yt_dlp available check just in case, though we checked at start
                        // If it's false, the user will see an empty download list or we should warn them.
                        // But we assume they have it if they chose to download.

                        break;
                    }
                    Ok(AssetDownloadEvent::Error(e)) => {
                        self.asset_download_rx = None;
                        self.asset_download_error = Some(e);
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.asset_download_rx = None;
                        self.asset_download_error = Some("Thread disconnected".to_string());
                        break;
                    }
                }
            }
        }

        if let Some(engine) = &mut self.audio_engine {
            engine.update(dt);
        }
        self.animation_offset += dt.as_secs_f32() * 3.0;

        // Queue Management
        if self.active_download_index.is_none() {
            // Find next pending task
            if let Some(idx) = self
                .download_queue
                .iter()
                .position(|t| matches!(t.status, DownloadStatus::Pending))
            {
                self.spawn_download_task(idx);
            }
        }

        // Poll download events
        if let Some(rx) = &self.download_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => match event {
                        DownloadEvent::Progress(p) => {
                            if let Some(idx) = self.active_download_index {
                                if let Some(task) = self.download_queue.get_mut(idx) {
                                    task.status = DownloadStatus::Downloading(p);
                                }
                            }
                        }
                        DownloadEvent::Success(name, cat, path, icon, url) => {
                            if let Some(idx) = self.active_download_index {
                                if let Some(task) = self.download_queue.get_mut(idx) {
                                    task.status = DownloadStatus::Done;
                                }
                            }
                            self.active_download_index = None;
                            self.download_rx = None;

                            // Keep URL in config
                            if let Err(e) = crate::static_data::add_custom_sound(
                                &name,
                                &cat,
                                &path,
                                &icon,
                                Some(&url),
                            ) {
                                log::error!("Failed to save config after download: {}", e);
                            } else {
                                log::info!("Successfully added sound '{}' with URL", name);
                                let id = name.to_lowercase().replace(" ", "_");
                                let new_sound = crate::static_data::Sound {
                                    id: id.clone(),
                                    name,
                                    category: cat,
                                    file_path: path,
                                    volume_linear: 0.5,
                                    icon,
                                    url: Some(url.clone()),
                                    error_state: false,
                                };
                                // Check if sound already exists (update case)
                                if let Some(existing) = self.sounds.iter_mut().find(|s| s.id == id)
                                {
                                    existing.file_path = new_sound.file_path;
                                    existing.url = Some(url);
                                    existing.error_state = false;
                                } else {
                                    let mut s = new_sound;
                                    s.url = Some(url);
                                    self.sounds.push(s);
                                }

                                self.sort_sounds();
                            }
                            break;
                        }
                        DownloadEvent::Error(e) => {
                            if let Some(idx) = self.active_download_index {
                                if let Some(task) = self.download_queue.get_mut(idx) {
                                    task.status = DownloadStatus::Error(e.clone());
                                }
                            }
                            self.active_download_index = None;
                            self.download_rx = None;
                            log::error!("Download error: {}", e);
                            break;
                        }
                    },
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        if let Some(idx) = self.active_download_index {
                            if let Some(task) = self.download_queue.get_mut(idx) {
                                task.status =
                                    DownloadStatus::Error("Thread disconnected".to_string());
                            }
                        }
                        self.active_download_index = None;
                        self.download_rx = None;
                        break;
                    }
                }
            }
        }
    }

    pub fn get_filtered_sounds(&self) -> Vec<(usize, &Sound)> {
        let hidden = &self.config.general.hidden_categories;
        if self.search_query.is_empty() {
            self.sounds
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    // Check hidden categories
                    if hidden.contains(&s.category) {
                        return false;
                    }
                    // Check hidden per-sound config
                    if let Some(sc) = self.config.sounds.get(&s.id) {
                        if sc.hidden {
                            return false;
                        }
                    }
                    true
                })
                .collect()
        } else {
            let query = self.search_query.to_lowercase();
            self.sounds
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    // Check hidden categories first
                    if hidden.contains(&s.category) {
                        return false;
                    }
                    // Check hidden per-sound config
                    if let Some(sc) = self.config.sounds.get(&s.id) {
                        if sc.hidden {
                            return false;
                        }
                    }

                    s.name.to_lowercase().contains(&query)
                        || s.category.to_lowercase().contains(&query)
                })
                .collect()
        }
    }

    pub fn sort_sounds(&mut self) {
        let order = &self.config.general.category_order;
        self.sounds.sort_by(|a, b| {
            let pos_a = order.iter().position(|c| c == &a.category);
            let pos_b = order.iter().position(|c| c == &b.category);

            match (pos_a, pos_b) {
                (Some(ia), Some(ib)) => {
                    let cmp = ia.cmp(&ib);
                    if cmp == std::cmp::Ordering::Equal {
                        a.id.cmp(&b.id)
                    } else {
                        cmp
                    }
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => {
                    let cmp = a.category.cmp(&b.category);
                    if cmp == std::cmp::Ordering::Equal {
                        a.id.cmp(&b.id)
                    } else {
                        cmp
                    }
                }
            }
        });
    }

    pub fn save_session(&mut self) {
        for sound in &self.sounds {
            let enabled = if let Some(engine) = &self.audio_engine {
                engine.is_playing(&sound.id)
            } else {
                false
            };

            self.session.sounds.insert(
                sound.id.clone(),
                SoundState {
                    enabled,
                    volume: sound.volume_linear,
                },
            );
        }
        let _ = self.session.save();
        let _ = self.presets_config.save();
    }
}
