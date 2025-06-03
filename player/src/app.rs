use std::{path::PathBuf, time::Instant};
use std::fs::OpenOptions;
use std::io::Write;

use crate::commands;
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Color,
};
use core::{
    MediaInfo, MediaPlayer, YouTubeConfig, YouTubePlayer, YouTubeVideoInfo, FrameBuffer,
    create_media_player, detect_media_type, MediaSourceType, render::RenderConfig,
    render::RenderMethod,
};

// App state
pub struct App {
    /// Current media player instance
    pub player: Option<Box<dyn MediaPlayer>>,
    /// Media information for the currently playing media
    pub media_info: Option<MediaInfo>,
    /// Current application view
    pub view: AppView,
    /// File browser state
    pub file_browser: FileBrowser,
    /// YouTube search state
    pub youtube_search: YoutubeSearch,
    /// Input field for URLs/search
    pub input: String,
    /// Cursor position in the input field
    pub input_cursor: usize,
    /// Whether the player UI is shown (always true in this implementation)
    pub show_ui: bool,
    /// Last time the UI was shown
    pub last_ui_interaction: Instant,
    /// Whether YouTube URLs should be automatically detected
    pub auto_detect_youtube: bool,
    /// YouTube player configuration
    pub youtube_config: YouTubeConfig,
    /// Render configuration
    pub render_config: RenderConfig,
    /// Status message to display
    pub status_message: Option<(String, Instant, Color)>,
    /// Whether the app should exit
    pub should_quit: bool,
    /// Help dialog visibility
    pub show_help: bool,
    /// Settings dialog visibility
    pub show_settings: bool,
    /// Whether command mode is active
    pub command_mode: bool,
    /// Command buffer for command mode
    pub command_buffer: String,
    /// Buffer status (buffered frames, capacity, buffer position)
    pub buffer_status: Option<(usize, usize, f64)>,
}

/// Application views
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppView {
    /// Main menu
    MainMenu,
    /// Media player
    Player,
    /// File browser
    FileBrowser,
    /// YouTube search
    YoutubeSearch,
    /// Settings
    Settings,
}

/// File browser state
#[derive(Clone)]
pub struct FileBrowser {
    /// Current directory
    pub current_dir: PathBuf,
    /// Files in current directory
    pub files: Vec<FileEntry>,
    /// Selected file index
    pub selected: Option<usize>,
    /// Navigation history
    pub history: Vec<PathBuf>,
    /// Navigation future for back/forward
    pub future: Vec<PathBuf>,
    /// Filter for file list
    pub filter: String,
}

/// File entry in file browser
#[derive(Clone)]
pub struct FileEntry {
    /// File path
    pub path: PathBuf,
    /// File name
    pub name: String,
    /// Whether it's a directory
    pub is_dir: bool,
    /// File size
    pub size: u64,
    /// Last modified time
    pub modified: Option<std::time::SystemTime>,
}

/// YouTube search state
#[derive(Clone)]
pub struct YoutubeSearch {
    /// Search query
    pub query: String,
    /// Search results
    pub results: Vec<YoutubeResult>,
    /// Selected result index
    pub selected: Option<usize>,
    /// Whether a search is in progress
    pub searching: bool,
}

/// YouTube search result
#[derive(Clone)]
pub struct YoutubeResult {
    /// Video ID
    pub id: String,
    /// Video title
    pub title: String,
    /// Video duration
    pub duration: String,
    /// Video thumbnail URL
    pub thumbnail: Option<String>,
    /// Channel name
    pub channel: String,
}

impl Default for App {
    fn default() -> Self {
        // Get home directory for file browser
        let home_dir = dirs::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        Self {
            player: None,
            media_info: None,
            view: AppView::MainMenu,
            file_browser: FileBrowser {
                current_dir: home_dir.clone(),
                files: Vec::new(),
                selected: None,
                history: vec![home_dir],
                future: Vec::new(),
                filter: String::new(),
            },
            youtube_search: YoutubeSearch {
                query: String::new(),
                results: Vec::new(),
                selected: None,
                searching: false,
            },
            input: String::new(),
            input_cursor: 0,
            show_ui: true,
            last_ui_interaction: Instant::now(),
            auto_detect_youtube: true,
            youtube_config: YouTubeConfig::default(),
            render_config: RenderConfig {
                enable_gpu: false,
                method: RenderMethod::Auto,
                target_fps: 30,
                quality: 1.0,
            },
            status_message: None,
            should_quit: false,
            show_help: false,
            show_settings: false,
            command_mode: false,
            command_buffer: String::new(),
            buffer_status: None,
        }
    }
}

impl App {
    /// Create a new application
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a status message with a color
    pub fn set_status(&mut self, message: impl Into<String>, color: Color) {
        let message_string = message.into();
        log::debug!("Status message: {} ({})", message_string, color);
        self.status_message = Some((message_string, Instant::now(), color));
    }

    /// Open a media file or URL
    pub fn open_media(&mut self, path_or_url: &str) -> Result<()> {
        // First, detect what type of media this is
        let media_type = detect_media_type(path_or_url);

        match media_type {
            MediaSourceType::Unsupported => {
                // Check if this is a YouTube URL or ID that wasn't detected automatically
                if self.auto_detect_youtube && path_or_url.len() == 11 {
                    // This might be a YouTube video ID, try to load it
                    let youtube_url = format!("https://www.youtube.com/watch?v={}", path_or_url);
                    return self.open_media(&youtube_url);
                } else {
                    return Err(anyhow::anyhow!("Unsupported media type: {}", path_or_url));
                }
            }
            _ => {
                // Valid media type detected, stop any existing playback
                if let Some(player) = &mut self.player {
                    if let Err(e) = player.stop() {
                        log::warn!("Error stopping previous player: {}", e);
                    }
                }

                // Create a new player based on the media type
                self.last_ui_interaction = Instant::now();
                self.player = Some(create_media_player(path_or_url, Some(self.render_config.clone()))?);
                self.view = AppView::Player;

                // Try to get media info
                if let Some(player) = &self.player {
                    if let Some(info) = player.get_media_info() {
                        self.media_info = Some(info);
                    } else {
                        self.set_status("Warning: Could not get media information".to_string(), Color::Yellow);
                    }
                }

                self.set_status(format!("Playing: {}", path_or_url), Color::Green);
            }
        }

        Ok(())
    }

    /// Refresh file list in file browser
    pub fn refresh_file_list(&mut self) -> Result<()> {
        let path = &self.file_browser.current_dir;

        // Read directory
        let dir_entries = std::fs::read_dir(path)
            .with_context(|| format!("Failed to read directory: {}", path.to_string_lossy()))?;

        // Convert to FileEntry objects
        let mut entries = Vec::new();
        let filter = self.file_browser.filter.to_lowercase();

        for entry in dir_entries {
            let entry = entry?;
            let path = entry.path();
            let name = entry
                .file_name()
                .to_string_lossy()
                .to_string();

            // Apply filter if set
            if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                continue;
            }

            let is_dir = path.is_dir();
            let metadata = entry.metadata().ok();
            let size = metadata.as_ref().map_or(0, |m| m.len());
            let modified = metadata.and_then(|m| m.modified().ok());

            entries.push(FileEntry {
                path,
                name,
                is_dir,
                size,
                modified,
            });
        }

        // Sort entries: directories first, then alphabetically
        entries.sort_by(|a, b| {
            match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });

        // Update file browser state
        self.file_browser.files = entries;
        self.file_browser.selected = if self.file_browser.files.is_empty() {
            None
        } else {
            Some(0)
        };

        Ok(())
    }

    /// Navigate to a directory in file browser
    pub fn navigate_to(&mut self, path: &PathBuf) -> Result<()> {
        // Add current directory to history
        self.file_browser.history.push(self.file_browser.current_dir.clone());
        self.file_browser.future.clear();

        // Update current directory
        self.file_browser.current_dir = path.clone();
        self.file_browser.selected = None;

        // Refresh file list
        self.refresh_file_list()?;

        Ok(())
    }

    /// Navigate back in file browser history
    pub fn navigate_back(&mut self) -> Result<()> {
        if let Some(prev_dir) = self.file_browser.history.pop() {
            // Add current directory to future
            self.file_browser.future.push(self.file_browser.current_dir.clone());

            // Update current directory
            self.file_browser.current_dir = prev_dir;
            self.file_browser.selected = None;

            // Refresh file list
            self.refresh_file_list()?;
        }

        Ok(())
    }

    /// Handle key event
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        self.last_ui_interaction = Instant::now();
        self.show_ui = true;

        // Check if we're in command mode
        if self.is_command_mode() {
            match key.code {
                KeyCode::Char(c) => {
                    self.add_to_command_buffer(c);
                }
                KeyCode::Backspace => {
                    self.remove_from_command_buffer();
                }
                KeyCode::Esc => {
                    self.exit_command_mode();
                }
                _ => {}
            }
            return Ok(());
        }

        // Handle function keys globally
        match key.code {
            KeyCode::F(1) => {
                self.show_help = !self.show_help;
                return Ok(());
            }
            _ => {}
        }

        // Handle view-specific keys
        match self.view {
            AppView::MainMenu => self.handle_main_menu_key(key)?,
            AppView::Player => self.handle_player_key(key)?,
            AppView::FileBrowser => self.handle_file_browser_key(key)?,
            AppView::YoutubeSearch => self.handle_youtube_search_key(key)?,
            AppView::Settings => self.handle_settings_key(key)?,
        }

        Ok(())
    }

    // Mouse events have been removed for consistency across all app views

    /// Handle key events in player view
    fn handle_player_key(&mut self, key: KeyEvent) -> Result<()> {
        // First check if we have a valid player
        if self.player.is_none() {
            // No player active, return to main menu
            self.view = AppView::MainMenu;
            self.set_status("No active media playback".to_string(), Color::Yellow);
            return Ok(());
        }

        // Check buffer status to show debugging info
        if let Some((buffer_size, capacity, _)) = self.buffer_status {
            log::debug!("Buffer status: {}/{} frames", buffer_size, capacity);
        }

        // IMPORTANT: Add aggressive debug logging to track control flow
        log::warn!("PLAYER CONTROL: Key press detected: {:?}", key.code);

        // Define an enum for the actions we might take
        enum PlayerAction {
            Seek(f64, String),
            TogglePause(bool),
        }

        // First determine what action to take without calling set_status
        let action = match key.code {
            KeyCode::Char(' ') => {
                if let Some(player) = &mut self.player {
                    // DIRECT CONTROL: Immediately toggle pause state to improve responsiveness
                    let was_paused = player.is_paused();
                    player.toggle_pause();
                    let now_paused = player.is_paused();
                    
                    log::warn!("PLAYER CONTROL: Toggle pause - Was: {}, Now: {}", was_paused, now_paused);
                    
                    self.last_ui_interaction = Instant::now();
                    Some(PlayerAction::TogglePause(now_paused))
                } else {
                    None
                }
            }
            KeyCode::Left => {
                if let Some(player) = &mut self.player {
                    let old_pos = player.get_position();
                    let new_pos = (old_pos - 5.0).max(0.0);
                    
                    log::warn!("PLAYER CONTROL: Seeking from {:.2}s to {:.2}s", old_pos, new_pos);
                    
                    // More robust seek with error handling
                    match player.seek(new_pos) {
                        Ok(_) => {
                            log::warn!("PLAYER CONTROL: Seek successful");
                            self.last_ui_interaction = Instant::now();
                            Some(PlayerAction::Seek(
                                new_pos,
                                format!("Seek to {:.1}s", new_pos),
                            ))
                        },
                        Err(e) => {
                            log::warn!("PLAYER CONTROL: Seek failed: {}", e);
                            self.set_status(format!("Seek failed: {}", e), Color::Red);
                            None
                        }
                    }
                } else {
                    None
                }
            }
            KeyCode::Right => {
                if let (Some(player), Some(media_info)) = (&mut self.player, &self.media_info) {
                    let old_pos = player.get_position();
                    let new_pos = (old_pos + 5.0).min(media_info.duration);
                    
                    log::warn!("PLAYER CONTROL: Seeking from {:.2}s to {:.2}s", old_pos, new_pos);
                    
                    // More robust seek with error handling
                    match player.seek(new_pos) {
                        Ok(_) => {
                            log::warn!("PLAYER CONTROL: Seek successful");
                            self.last_ui_interaction = Instant::now();
                            Some(PlayerAction::Seek(
                                new_pos,
                                format!("Seek to {:.1}s", new_pos),
                            ))
                        },
                        Err(e) => {
                            log::warn!("PLAYER CONTROL: Seek failed: {}", e);
                            self.set_status(format!("Seek failed: {}", e), Color::Red);
                            None
                        }
                    }
                } else {
                    None
                }
            }
            KeyCode::Char('f') => {
                if let (Some(player), Some(media_info)) = (&mut self.player, &self.media_info) {
                    let old_pos = player.get_position();
                    let new_pos = (old_pos + 30.0).min(media_info.duration);
                    
                    log::warn!("PLAYER CONTROL: Seeking forward 30s from {:.2}s to {:.2}s", old_pos, new_pos);
                    
                    match player.seek(new_pos) {
                        Ok(_) => {
                            log::warn!("PLAYER CONTROL: Forward seek successful");
                            self.last_ui_interaction = Instant::now();
                            Some(PlayerAction::Seek(
                                new_pos,
                                format!("Forward 30s to {:.1}s", new_pos),
                            ))
                        },
                        Err(e) => {
                            log::warn!("PLAYER CONTROL: Forward seek failed: {}", e);
                            self.set_status(format!("Seek failed: {}", e), Color::Red);
                            None
                        }
                    }
                } else {
                    None
                }
            }
            KeyCode::Char('b') => {
                if let Some(player) = &mut self.player {
                    let old_pos = player.get_position();
                    let new_pos = (old_pos - 30.0).max(0.0);
                    
                    log::warn!("PLAYER CONTROL: Seeking back 30s from {:.2}s to {:.2}s", old_pos, new_pos);
                    
                    match player.seek(new_pos) {
                        Ok(_) => {
                            log::warn!("PLAYER CONTROL: Backward seek successful");
                            self.last_ui_interaction = Instant::now();
                            Some(PlayerAction::Seek(
                                new_pos,
                                format!("Back 30s to {:.1}s", new_pos),
                            ))
                        },
                        Err(e) => {
                            log::warn!("PLAYER CONTROL: Backward seek failed: {}", e);
                            self.set_status(format!("Seek failed: {}", e), Color::Red);
                            None
                        }
                    }
                } else {
                    None
                }
            }
            KeyCode::Esc => {
                if self.show_help || self.show_settings {
                    self.show_help = false;
                    self.show_settings = false;
                    None
                } else {
                    // Stop playback and return to main menu
                    if let Some(player) = &mut self.player {
                        let _ = player.stop();
                    }
                    self.player = None;
                    self.media_info = None;
                    self.view = AppView::MainMenu;
                    Some(PlayerAction::Seek(0.0, "Playback stopped".to_string()))
                }
            }
            KeyCode::Char('o') => {
                self.view = AppView::FileBrowser;
                self.refresh_file_list()?;
                Some(PlayerAction::Seek(0.0, "File Browser".to_string()))
            }
            KeyCode::Char('y') => {
                self.view = AppView::YoutubeSearch;
                self.input = String::new();
                self.input_cursor = 0;
                Some(PlayerAction::Seek(0.0, "YouTube Search".to_string()))
            }
            KeyCode::Char('s') => {
                // Toggle settings dialog
                self.show_settings = !self.show_settings;
                // Set view to Settings if dialog is shown
                if self.show_settings {
                    self.view = AppView::Settings;
                }
                Some(PlayerAction::Seek(
                    0.0,
                    if self.show_settings {
                        "Settings opened".to_string()
                    } else {
                        "Settings closed".to_string()
                    },
                ))
            }
            KeyCode::Char('h') => {
                // Toggle help dialog
                self.show_help = !self.show_help;
                Some(PlayerAction::Seek(
                    0.0,
                    if self.show_help {
                        "Help opened".to_string()
                    } else {
                        "Help closed".to_string()
                    },
                ))
            }
            _ => None,
        };

        // Now take action based on what we determined above
        if let Some(action) = action {
            match action {
                PlayerAction::Seek(_, message) => {
                    self.set_status(message, Color::Blue);
                }
                PlayerAction::TogglePause(is_paused) => {
                    self.set_status(
                        (if is_paused { "Paused" } else { "Playing" }).to_string(),
                        Color::Green,
                    );
                }
            }
        }

        Ok(())
    }

    /// Handle key events in main menu view
    fn handle_main_menu_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('1') | KeyCode::Char('f') => {
                self.view = AppView::FileBrowser;
                self.refresh_file_list()?;
                self.set_status("File Browser".to_string(), Color::Blue);
            }
            KeyCode::Char('2') | KeyCode::Char('y') => {
                self.view = AppView::YoutubeSearch;
                self.input = String::new();
                self.input_cursor = 0;
                self.set_status("YouTube Search".to_string(), Color::Blue);
            }
            KeyCode::Char('3') | KeyCode::Char('s') => {
                self.view = AppView::Settings;
                self.show_settings = true;
                self.set_status("Settings".to_string(), Color::Blue);
            }
            KeyCode::Char('4') | KeyCode::Char('h') => {
                self.show_help = true;
                self.set_status("Help".to_string(), Color::Blue);
            }
            KeyCode::Char('5') | KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Esc => {
                // Close help or settings if open
                if self.show_help {
                    self.show_help = false;
                }
                if self.show_settings {
                    self.show_settings = false;
                }
            }
            _ => {}
        }

        Ok(())
    }

    // Mouse events have been removed for consistency across all app views

    /// Handle key events in file browser view
    fn handle_file_browser_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up => {
                if let Some(selected) = self.file_browser.selected {
                    if selected > 0 {
                        self.file_browser.selected = Some(selected - 1);
                    }
                }
            }
            KeyCode::Down => {
                if let Some(selected) = self.file_browser.selected {
                    if selected < self.file_browser.files.len().saturating_sub(1) {
                        self.file_browser.selected = Some(selected + 1);
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(selected) = self.file_browser.selected {
                    if let Some(entry) = self.file_browser.files.get(selected) {
                        if entry.is_dir {
                            // Navigate to directory
                            self.navigate_to(&entry.path)?;
                        } else {
                            // Open file
                            self.open_media(entry.path.to_str().unwrap_or_default())?;
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                // Navigate up a directory
                let current_dir = &self.file_browser.current_dir;
                if let Some(parent) = current_dir.parent() {
                    self.navigate_to(&parent.to_path_buf())?;
                }
            }
            KeyCode::Char('~') => {
                // Navigate to home directory
                if let Some(home) = dirs::home_dir() {
                    self.navigate_to(&home)?;
                }
            }
            KeyCode::Char('/') => {
                // Filter files
                self.input = String::new();
                self.input_cursor = 0;
                // TODO: Enter filter mode
            }
            KeyCode::Esc => {
                self.view = AppView::MainMenu;
                self.set_status("Main Menu".to_string(), Color::Blue);
            }
            _ => {}
        }

        Ok(())
    }

    // Mouse events have been removed for consistency across all app views

    /// Handle key events in YouTube search view
    fn handle_youtube_search_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                if self.input.is_empty() {
                    // If the input is empty but a result is selected, play that result
                    if let Some(selected) = self.youtube_search.selected {
                        if let Some(result) = self.youtube_search.results.get(selected).cloned() {
                            self.open_media(&result.id)?;
                        }
                    }
                } else {
                    // Perform YouTube search with the input
                    let query = self.input.clone();
                    self.youtube_search.query = query.clone();
                    self.youtube_search.searching = true;
                    self.set_status(format!("Searching YouTube for '{}'...", query), Color::Yellow);
                    
                    // Create search results (would connect to API in production)
                    self.youtube_search.results.clear();
                    
                    // Add some mock results for testing
                    self.youtube_search.results.push(YoutubeResult {
                        id: "dQw4w9WgXcQ".to_string(),
                        title: format!("{} - Never Gonna Give You Up", query),
                        duration: "3:32".to_string(),
                        thumbnail: None,
                        channel: "Rick Astley".to_string(),
                    });
                    
                    self.youtube_search.results.push(YoutubeResult {
                        id: "xvFZjo5PgG0".to_string(),
                        title: format!("{} - Popular Video", query),
                        duration: "4:20".to_string(),
                        thumbnail: None,
                        channel: "YouTube Channel".to_string(),
                    });
                    
                    self.youtube_search.results.push(YoutubeResult {
                        id: "jNQXAC9IVRw".to_string(),
                        title: format!("{} - Me at the zoo", query),
                        duration: "0:19".to_string(),
                        thumbnail: None,
                        channel: "jawed".to_string(),
                    });
                    
                    // Set first result as selected and update status
                    self.youtube_search.selected = Some(0);
                    self.youtube_search.searching = false;
                    self.set_status(format!("Found {} results for '{}'", 
                        self.youtube_search.results.len(), query), Color::Green);
                    
                    self.input = String::new();
                    self.input_cursor = 0;
                }
            }
            KeyCode::Up => {
                if let Some(selected) = self.youtube_search.selected {
                    if selected > 0 {
                        self.youtube_search.selected = Some(selected - 1);
                    }
                }
            }
            KeyCode::Down => {
                if let Some(selected) = self.youtube_search.selected {
                    if selected < self.youtube_search.results.len().saturating_sub(1) {
                        self.youtube_search.selected = Some(selected + 1);
                    }
                }
            }
            KeyCode::Char(c) => {
                // Add to input
                self.input.insert(self.input_cursor, c);
                self.input_cursor += 1;
            }
            KeyCode::Backspace => {
                // Remove from input
                if self.input_cursor > 0 {
                    self.input.remove(self.input_cursor - 1);
                    self.input_cursor -= 1;
                }
            }
            KeyCode::Left => {
                // Move cursor left
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                }
            }
            KeyCode::Right => {
                // Move cursor right
                if self.input_cursor < self.input.len() {
                    self.input_cursor += 1;
                }
            }
            KeyCode::Esc => {
                // Return to main menu
                self.view = AppView::MainMenu;
                self.set_status("Main Menu".to_string(), Color::Blue);
            }
            _ => {}
        }

        Ok(())
    }

    // Mouse events have been removed for consistency across all app views

    /// Handle key events in settings view
    fn handle_settings_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.view = AppView::MainMenu;
                self.show_settings = false;
            }
            KeyCode::Char('1') => {
                // Toggle GPU acceleration
                self.render_config.enable_gpu = !self.render_config.enable_gpu;
            }
            KeyCode::Char('2') => {
                // Cycle through render methods
                self.render_config.method = match self.render_config.method {
                    RenderMethod::Auto => RenderMethod::Blocks,
                    RenderMethod::Blocks => RenderMethod::Sixel,
                    RenderMethod::Sixel => RenderMethod::Kitty,
                    RenderMethod::Kitty => RenderMethod::ITerm,
                    RenderMethod::ITerm => RenderMethod::Auto,
                };
            }
            _ => {}
        }

        Ok(())
    }

    /// Update application state
    pub fn update(&mut self) -> Result<()> {
        // Always keep UI visible - this prevents flickering issues
        self.show_ui = true;

        // Clear status message after timeout
        if let Some((_, time, _)) = &self.status_message {
            if time.elapsed() > std::time::Duration::from_secs(5) {
                self.status_message = None;
            }
        }

        // When Settings or Help dialogs are shown, make sure we're in the right view
        if self.show_settings && self.view != AppView::Settings {
            self.view = AppView::Settings;
        }

        // Make sure YouTube search view is properly set up
        if self.view