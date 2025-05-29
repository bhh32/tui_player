use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Rect, Layout, Direction, Constraint},
    style::Color
};
use crate::commands;

use core::{
    create_media_player, detect_media_type, MediaInfo, MediaPlayer, MediaSourceType, YouTubeConfig,
    YouTubePlayer, render::RenderConfig,
};

/// Main application state
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
}

/// Application views
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppView {
    /// Main menu view (home screen)
    MainMenu,
    /// Player view (main video playback)
    Player,
    /// File browser view
    FileBrowser,
    /// YouTube search view
    YoutubeSearch,
    /// Settings view
    Settings,
}

/// File browser state
pub struct FileBrowser {
    /// Current directory
    pub current_dir: PathBuf,
    /// List of files in the current directory
    pub files: Vec<FileEntry>,
    /// Currently selected file index
    pub selected: Option<usize>,
    /// History of directories
    pub history: Vec<PathBuf>,
    /// Future directories (for forward navigation)
    pub future: Vec<PathBuf>,
    /// Filter string
    pub filter: String,
}

/// File entry in the file browser
#[derive(Clone)]
pub struct FileEntry {
    /// File path
    pub path: PathBuf,
    /// File name
    pub name: String,
    /// Whether this is a directory
    pub is_dir: bool,
    /// File size in bytes
    pub size: u64,
    /// Last modified time
    pub modified: Option<std::time::SystemTime>,
}

/// YouTube search state
pub struct YoutubeSearch {
    /// Search query
    pub query: String,
    /// Search results
    pub results: Vec<YoutubeResult>,
    /// Currently selected result
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
        // Set default render configuration to Auto for best terminal compatibility
        let mut render_config = RenderConfig::default();
        render_config.method = core::render::RenderMethod::Auto;
        
        // Set default YouTube configuration
        let youtube_config = YouTubeConfig {
            quality: 1, // 0 is best, higher is lower quality
            format: Some("mp4".to_string()),
            max_resolution: Some("720p".to_string()),
            ..Default::default()
        };
        
        // Add some placeholder YouTube results for demonstration
        let mut youtube_results = Vec::new();
        youtube_results.push(YoutubeResult {
            id: "dQw4w9WgXcQ".to_string(),
            title: "Sample YouTube Video".to_string(),
            duration: "3:32".to_string(),
            thumbnail: None,
            channel: "Sample Channel".to_string(),
        });
        
        // Get home directory for file browser
        let home_dir = directories::UserDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        
        Self {
            player: None,
            media_info: None,
            view: AppView::MainMenu,
            file_browser: FileBrowser {
                current_dir: home_dir.clone(),
                files: Vec::new(),
                selected: None,
                history: Vec::new(),
                future: Vec::new(),
                filter: String::new(),
            },
            youtube_search: YoutubeSearch {
                query: String::new(),
                results: youtube_results,
                selected: Some(0),
                searching: false,
            },
            input: String::new(),
            input_cursor: 0,
            show_ui: true,
            last_ui_interaction: Instant::now(),
            auto_detect_youtube: true,
            youtube_config,
            render_config,
            status_message: None,
            should_quit: false,
            show_help: false,
            show_settings: false,
            command_mode: false,
            command_buffer: String::new(),
        }
    }
}

impl App {
    /// Create a new application
    pub fn new() -> Self {
        let mut app = Self::default();
        // Initialize the file browser
        app.refresh_file_list().ok();
        app
    }

    /// Set a status message with a color
    pub fn set_status(&mut self, message: impl Into<String>, color: Color) {
        self.status_message = Some((message.into(), Instant::now(), color));
    }

    /// Open a media file or URL
    pub fn open_media(&mut self, path: &str) -> Result<()> {
        // Detect media type
        let media_type = detect_media_type(path);
        
        if media_type == MediaSourceType::Unsupported {
            self.set_status(format!("Unsupported media: {}", path), Color::Red);
            return Err(anyhow::anyhow!("Unsupported media type"));
        }
        
        // Configure rendering to leave space for UI
        let mut render_config = self.render_config.clone();
        render_config.y = 5; // Leave space at top for UI
        
        // Set status to show we're working on it
        match media_type {
            MediaSourceType::LocalFile => {
                self.set_status(format!("Opening file: {}", path), Color::Yellow);
            },
            MediaSourceType::YouTube => {
                self.set_status(format!("Opening YouTube: {} (this may take a moment...)", path), Color::Yellow);
            },
            _ => {}
        }
        
        // Create media player with timeout protection to prevent hanging
        let player_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match media_type {
                MediaSourceType::LocalFile => {
                    create_media_player(path, Some(render_config))
                },
                MediaSourceType::YouTube => {
                    // Create YouTube player with configuration
                    let player = YouTubePlayer::new(
                        path, 
                        Some(render_config),
                        Some(self.youtube_config.clone()),
                    )?;
                    
                    Ok(Box::new(player) as Box<dyn MediaPlayer>)
                },
                _ => Err(anyhow::anyhow!("Unsupported media type")),
            }
        }));
        
        // Handle player creation result
        let player = match player_result {
            Ok(result) => match result {
                Ok(player) => {
                    // Successfully created player
                    match media_type {
                        MediaSourceType::LocalFile => {
                            self.set_status(format!("Opened file: {}", path), Color::Green);
                        },
                        MediaSourceType::YouTube => {
                            self.set_status(format!("Opened YouTube video: {}", path), Color::Green);
                        },
                        _ => {}
                    }
                    player
                },
                Err(e) => {
                    // Error creating player
                    let error_msg = format!("Error opening media: {}", e);
                    self.set_status(error_msg, Color::Red);
                    return Err(e);
                }
            },
            Err(_) => {
                // Panic during player creation
                let error_msg = "Player initialization failed unexpectedly";
                self.set_status(error_msg.to_string(), Color::Red);
                return Err(anyhow::anyhow!(error_msg));
            }
        };
        
        // Store the player and get media info
        self.player = Some(player);
        if let Some(player) = &self.player {
            // Attempt to get media info, but handle errors gracefully
            self.media_info = player.get_media_info();
            if self.media_info.is_none() {
                self.set_status("Warning: Could not get media information", Color::Yellow);
            }
            self.view = AppView::Player;
            self.show_ui = true;
            self.last_ui_interaction = Instant::now();
        }
        
        Ok(())
    }
    
    /// Refresh the file list in the file browser
    pub fn refresh_file_list(&mut self) -> Result<()> {
        let dir = &self.file_browser.current_dir;
        
        // Read directory contents
        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("Failed to read directory: {}", dir.display()))?;
        
        // Create file entries
        let mut files = Vec::new();
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                let metadata = entry.metadata().ok();
                
                let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified = metadata.and_then(|m| m.modified().ok());
                
                let name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown")
                    .to_string();
                
                // Skip hidden files
                if !name.starts_with('.') {
                    files.push(FileEntry {
                        path,
                        name,
                        is_dir,
                        size,
                        modified,
                    });
                }
            }
        }
        
        // Sort: directories first, then alphabetically
        files.sort_by(|a, b| {
            match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });
        
        self.file_browser.files = files;
        self.file_browser.selected = if self.file_browser.files.is_empty() {
            None
        } else {
            Some(0)
        };
        
        Ok(())
    }
    
    /// Navigate to a directory in the file browser
    pub fn navigate_to(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        
        if path.is_dir() {
            // Add current directory to history
            self.file_browser.history.push(self.file_browser.current_dir.clone());
            self.file_browser.future.clear();
            
            // Set new directory
            self.file_browser.current_dir = path.to_path_buf();
            self.refresh_file_list()?;
            
            self.set_status(format!("Directory: {}", path.display()), Color::Blue);
        }
        
        Ok(())
    }
    
    /// Navigate back in file browser history
    pub fn navigate_back(&mut self) -> Result<()> {
        if let Some(prev_dir) = self.file_browser.history.pop() {
            // Add current directory to future for forward navigation
            self.file_browser.future.push(self.file_browser.current_dir.clone());
            
            // Set previous directory
            self.file_browser.current_dir = prev_dir;
            self.refresh_file_list()?;
        }
        
        Ok(())
    }
    
    /// Navigate forward in file browser history
    pub fn navigate_forward(&mut self) -> Result<()> {
        if let Some(next_dir) = self.file_browser.future.pop() {
            // Add current directory to history
            self.file_browser.history.push(self.file_browser.current_dir.clone());
            
            // Set next directory
            self.file_browser.current_dir = next_dir;
            self.refresh_file_list()?;
        }
        
        Ok(())
    }
    
    /// Handle key event
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        // Handle global keybindings first
        match key.code {
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                return Ok(());
            }
            KeyCode::Char(':') if !self.command_mode => {
                self.enter_command_mode();
                return Ok(());
            }
            KeyCode::F(1) => {
                self.show_help = !self.show_help;
                return Ok(());
            }
            _ => {}
        }
        
        // If in command mode, handle command input
        if self.command_mode {
            return self.handle_command_key(key);
        }
        
        // Otherwise delegate to view-specific handlers
        match self.view {
            AppView::MainMenu => self.handle_main_menu_key(key)?,
            AppView::Player => self.handle_player_key(key)?,
            AppView::FileBrowser => self.handle_file_browser_key(key)?,
            AppView::YoutubeSearch => self.handle_youtube_search_key(key)?,
            AppView::Settings => self.handle_settings_key(key)?,
        }
        
        Ok(())
    }
    
    /// Handle key events in command mode
    fn handle_command_key(&mut self, key: KeyEvent) -> Result<()> {
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
            KeyCode::Enter => {
                let cmd = self.command_buffer.clone();
                self.exit_command_mode();
                commands::handle_command(self, &cmd)?;
            }
            _ => {}
        }
        Ok(())
    }
    
    /// Handle mouse event
    pub fn handle_mouse_event(&mut self, mouse: MouseEvent, area: Rect) -> Result<()> {
        self.last_ui_interaction = Instant::now();
        self.show_ui = true;
        
        // Handle mouse events based on current view
        match self.view {
            AppView::MainMenu => {
                if let Err(e) = self.handle_main_menu_mouse(mouse, area) {
                    self.set_status(format!("Mouse error: {}", e), Color::Red);
                }
            },
            AppView::Player => {
                if let Err(e) = self.handle_player_mouse(mouse, area) {
                    self.set_status(format!("Mouse error: {}", e), Color::Red);
                }
            },
            AppView::FileBrowser => {
                if let Err(e) = self.handle_file_browser_mouse(mouse, area) {
                    self.set_status(format!("Mouse error: {}", e), Color::Red);
                }
            },
            AppView::YoutubeSearch => {
                if let Err(e) = self.handle_youtube_search_mouse(mouse, area) {
                    self.set_status(format!("Mouse error: {}", e), Color::Red);
                }
            },
            AppView::Settings => {
                if let Err(e) = self.handle_settings_mouse(mouse, area) {
                    self.set_status(format!("Mouse error: {}", e), Color::Red);
                }
            },
        }
        
        Ok(())
    }
    
    /// Handle key events in player view
    fn handle_player_key(&mut self, key: KeyEvent) -> Result<()> {
        // First check if we have a valid player
        if self.player.is_none() {
            // No player active, return to main menu
            self.view = AppView::MainMenu;
            self.set_status("No active media playback", Color::Yellow);
            return Ok(());
        }
        
        // Define an enum for the actions we might take
        enum PlayerAction {
            Seek(f64, String),
            TogglePause(bool),
        }
        
        // First determine what action to take without calling set_status
        let action = match key.code {
            KeyCode::Char(' ') => {
                if let Some(player) = &mut self.player {
                    player.toggle_pause();
                    self.last_ui_interaction = Instant::now();
                    let is_paused = player.is_paused();
                    Some(PlayerAction::TogglePause(is_paused))
                } else {
                    None
                }
            }
            KeyCode::Left => {
                if let Some(player) = &mut self.player {
                    let new_pos = (player.get_position() - 5.0).max(0.0);
                    let _ = player.seek(new_pos);
                    self.last_ui_interaction = Instant::now();
                    Some(PlayerAction::Seek(new_pos, format!("Seek to {:.1}s", new_pos)))
                } else {
                    None
                }
            }
            KeyCode::Right => {
                if let (Some(player), Some(media_info)) = (&mut self.player, &self.media_info) {
                    let new_pos = (player.get_position() + 5.0).min(media_info.duration);
                    let _ = player.seek(new_pos);
                    self.last_ui_interaction = Instant::now();
                    Some(PlayerAction::Seek(new_pos, format!("Seek to {:.1}s", new_pos)))
                } else {
                    None
                }
            }
            KeyCode::Char('f') => {
                if let (Some(player), Some(media_info)) = (&mut self.player, &self.media_info) {
                    let new_pos = (player.get_position() + 30.0).min(media_info.duration);
                    let _ = player.seek(new_pos);
                    self.last_ui_interaction = Instant::now();
                    Some(PlayerAction::Seek(new_pos, format!("Forward 30s to {:.1}s", new_pos)))
                } else {
                    None
                }
            }
            KeyCode::Char('b') => {
                if let Some(player) = &mut self.player {
                    let new_pos = (player.get_position() - 30.0).max(0.0);
                    let _ = player.seek(new_pos);
                    self.last_ui_interaction = Instant::now();
                    Some(PlayerAction::Seek(new_pos, format!("Back 30s to {:.1}s", new_pos)))
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
                Some(PlayerAction::Seek(0.0, 
                    if self.show_settings { "Settings opened" } else { "Settings closed" }.to_string()))
            }
            _ => None
        };
        
        // Now take action based on what we determined above
        if let Some(action) = action {
            match action {
                PlayerAction::Seek(_, message) => {
                    self.set_status(message, Color::Blue);
                },
                PlayerAction::TogglePause(is_paused) => {
                    self.set_status(
                        format!("{}", if is_paused { "Paused" } else { "Playing" }),
                        Color::Green
                    );
                }
            }
        }
        
        Ok(())
    }
    
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
            KeyCode::PageUp => {
                if let Some(selected) = self.file_browser.selected {
                    let new_selected = selected.saturating_sub(10);
                    self.file_browser.selected = Some(new_selected);
                }
            }
            KeyCode::PageDown => {
                if let Some(selected) = self.file_browser.selected {
                    let new_selected = (selected + 10).min(self.file_browser.files.len().saturating_sub(1));
                    self.file_browser.selected = Some(new_selected);
                }
            }
            KeyCode::Home => {
                if !self.file_browser.files.is_empty() {
                    self.file_browser.selected = Some(0);
                }
            }
            KeyCode::End => {
                if !self.file_browser.files.is_empty() {
                    self.file_browser.selected = Some(self.file_browser.files.len() - 1);
                }
            }
            KeyCode::Enter => {
                if let Some(selected) = self.file_browser.selected {
                    if let Some(entry) = self.file_browser.files.get(selected).cloned() {
                        if entry.is_dir {
                            self.navigate_to(&entry.path)?;
                        } else {
                            self.open_media(entry.path.to_str().unwrap_or_default())?;
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                self.navigate_back()?;
            }
            KeyCode::Char('y') => {
                self.view = AppView::YoutubeSearch;
                self.input = String::new();
                self.input_cursor = 0;
            }
            KeyCode::Char('/') => {
                self.file_browser.filter.clear();
                self.input = String::new();
                self.input_cursor = 0;
                // TODO: Enter filter mode
            }
            KeyCode::Char('~') => {
                if let Some(home) = directories::UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
                    self.navigate_to(home)?;
                }
            }
            KeyCode::Esc => {
                if self.show_help || self.show_settings {
                    self.show_help = false;
                    self.show_settings = false;
                } else {
                    self.file_browser.filter.clear();
                    self.view = AppView::MainMenu;
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
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
                    self.youtube_search.query = query;
                    self.youtube_search.searching = true;
                    // TODO: Implement actual search
                    self.set_status("Searching YouTube...", Color::Yellow);
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
                if self.input_cursor < self.input.len() {
                    self.input.insert(self.input_cursor, c);
                } else {
                    self.input.push(c);
                }
                self.input_cursor += 1;
            }
            KeyCode::Backspace => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    if self.input_cursor < self.input.len() {
                        self.input.remove(self.input_cursor);
                    }
                }
            }
            KeyCode::Esc => {
                if self.show_help || self.show_settings {
                    self.show_help = false;
                    self.show_settings = false;
                } else {
                    self.view = AppView::MainMenu;
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    /// Handle key events in settings view
    /// Handle settings menu
    fn handle_settings_key(&mut self, key: KeyEvent) -> Result<()> {
        
        match key.code {
            KeyCode::Esc => {
                self.show_settings = false;
                // Return to main menu if we were in Settings view
                if self.view == AppView::Settings {
                    self.view = AppView::MainMenu;
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    /// Handle key events in main menu view
    fn handle_main_menu_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                // Currently no selection mechanism, so we'll handle number keys instead
            }
            KeyCode::Char('1') | KeyCode::Char('f') => {
                // Open file browser
                self.view = AppView::FileBrowser;
                self.refresh_file_list()?;
                self.set_status("File Browser", Color::Blue);
            }
            KeyCode::Char('2') | KeyCode::Char('y') => {
                // Open YouTube search
                self.view = AppView::YoutubeSearch;
                self.input = String::new();
                self.input_cursor = 0;
                self.set_status("YouTube Search", Color::Blue);
            }
            KeyCode::Char('3') | KeyCode::Char('s') => {
                // Open settings
                self.view = AppView::Settings;
                self.show_settings = true;
                self.set_status("Settings", Color::Blue);
            }
            KeyCode::Char('4') | KeyCode::Char('h') | KeyCode::F(1) => {
                // Show help
                self.show_help = true;
                self.set_status("Help", Color::Blue);
            }
            KeyCode::Char('5') | KeyCode::Char('q') | KeyCode::Char('x') => {
                // Exit application
                self.should_quit = true;
            }
            _ => {}
        }
        
        Ok(())
    }
    
    /// Handle mouse events in main menu view
    fn handle_main_menu_mouse(&mut self, mouse: MouseEvent, area: Rect) -> Result<()> {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            // Calculate button areas
            let button_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),  // Title
                    Constraint::Length(5),  // Space + Local Files button
                    Constraint::Length(4),  // Space + YouTube button
                    Constraint::Length(4),  // Space + Settings button
                    Constraint::Length(4),  // Space + Help button
                    Constraint::Length(4),  // Space + Exit button
                    Constraint::Min(0),     // Remaining space
                ])
                .split(area);
            
            // Check if we clicked on any button (using row position)
            if mouse.row >= button_layout[1].y && mouse.row < button_layout[1].y + 3 {
                // File browser button clicked
                self.view = AppView::FileBrowser;
                self.refresh_file_list()?;
                self.set_status("File Browser", Color::Blue);
            } else if mouse.row >= button_layout[2].y && mouse.row < button_layout[2].y + 3 {
                // YouTube button clicked
                self.view = AppView::YoutubeSearch;
                self.input = String::new();
                self.input_cursor = 0;
                self.set_status("YouTube Search", Color::Blue);
            } else if mouse.row >= button_layout[3].y && mouse.row < button_layout[3].y + 3 {
                // Settings button clicked
                self.view = AppView::Settings;
                self.show_settings = true;
                self.set_status("Settings", Color::Blue);
            } else if mouse.row >= button_layout[4].y && mouse.row < button_layout[4].y + 3 {
                // Help button clicked
                self.show_help = true;
                self.set_status("Help", Color::Blue);
            } else if mouse.row >= button_layout[5].y && mouse.row < button_layout[5].y + 3 {
                // Exit button clicked
                self.should_quit = true;
            }
        }
        
        Ok(())
    }
    
    /// Handle mouse events in player view
    fn handle_player_mouse(&mut self, mouse: MouseEvent, area: Rect) -> Result<()> {
        // First check if we have a valid player
        if self.player.is_none() || self.media_info.is_none() {
            // No player active, return to main menu
            self.view = AppView::MainMenu;
            self.set_status("No active media playback", Color::Yellow);
            return Ok(());
        }
        
        // Define an enum for the actions we might take
        enum PlayerAction {
            Seek(f64, String),
            TogglePause(bool),
        }
        
        // First determine what action to take without calling set_status
        let action = if let (Some(player), Some(media_info)) = (&mut self.player, &self.media_info) {
            // Calculate UI control positions - at the bottom of the screen
            let controls_area_top = area.height.saturating_sub(5);
            
            // Check if we're clicking on the exit button in the controls
            let button_x = (area.width / 2) as u16;
            let button_y = controls_area_top + 2; // Two rows below progress bar
            let button_width = 10; // Width of play/pause button
            let exit_button_x = button_x + button_width + 2 + 25 + 7; // After info and some spacing
            let exit_button_width = 15;
        
            if mouse.row == button_y && 
               mouse.column >= exit_button_x - (exit_button_width / 2) && 
               mouse.column < exit_button_x + (exit_button_width / 2) {
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    // Stop playback and return to main menu
                    player.toggle_pause();
                    self.view = AppView::MainMenu;
                    return Ok(());
                }
            }
        
            // Check if we're clicking on the progress bar
            let progress_bar_y = controls_area_top + 1;
            let progress_bar_height = 1;
        
            if mouse.row >= progress_bar_y && mouse.row < progress_bar_y + progress_bar_height {
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    // Calculate position based on click position
                    let progress_percent = mouse.column as f64 / area.width as f64;
                    let new_pos = progress_percent * media_info.duration;
                    let _ = player.seek(new_pos);
                
                    Some(PlayerAction::Seek(new_pos, format!("Seek to {:.1}s", new_pos)))
                } else {
                    None
                }
            }
            // Check if we're clicking on the play/pause button
            else {
                let button_x = (area.width / 2) as u16;
                let button_y = controls_area_top + 2; // Two rows below progress bar
                let button_width = 10; // Wider to make it easier to click
                let button_height = 1;
                
                if mouse.row >= button_y && mouse.row < button_y + button_height
                    && mouse.column >= button_x - (button_width / 2) && mouse.column < button_x + (button_width / 2)
                {
                    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                        player.toggle_pause();
                        let is_paused = player.is_paused();
                        Some(PlayerAction::TogglePause(is_paused))
                    } else {
                        None
                    }
                }
                // Check if we're clicking on the back button
                else {
                    let back_button_x = button_x - button_width - 2;
                    if mouse.row == button_y && 
                       mouse.column >= back_button_x - 3 && mouse.column < back_button_x + 3 {
                        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                            let new_pos = (player.get_position() - 10.0).max(0.0);
                            let _ = player.seek(new_pos);
                            Some(PlayerAction::Seek(new_pos, format!("Back 10s to {:.1}s", new_pos)))
                        } else {
                            None
                        }
                    }
                    // Check if we're clicking on the forward button
                    else {
                        let forward_button_x = button_x + button_width + 2;
                        if mouse.row == button_y && 
                           mouse.column >= forward_button_x - 3 && mouse.column < forward_button_x + 3 {
                            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                                let new_pos = match (player.get_position() + 10.0, media_info.duration) {
                                    (pos, dur) if pos < dur => pos,
                                    (_, dur) => dur
                                };
                                let _ = player.seek(new_pos);
                                Some(PlayerAction::Seek(new_pos, format!("Forward 10s to {:.1}s", new_pos)))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                }
            }
        } else {
            None
        };
        
        // Now take action based on what we determined above
        if let Some(action) = action {
            match action {
                PlayerAction::Seek(_, message) => {
                    self.set_status(message, Color::Yellow);
                },
                PlayerAction::TogglePause(is_paused) => {
                    self.set_status(
                        format!("{}", if is_paused { "Paused" } else { "Playing" }),
                        Color::Green
                    );
                }
            }
        }
                
        
        // Handle top menu clicks (File, YouTube, Settings, Help)
        if mouse.row == 0 && mouse.column < area.width {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                // Check which menu item was clicked
                let menu_width = area.width / 4;
                
                if mouse.column < menu_width {
                    // File menu
                    self.view = AppView::FileBrowser;
                    self.refresh_file_list()?;
                    self.set_status("File Browser", Color::Blue);
                } else if mouse.column < menu_width * 2 {
                    // YouTube menu
                    self.view = AppView::YoutubeSearch;
                    self.input = String::new();
                    self.input_cursor = 0;
                    self.set_status("YouTube Search", Color::Blue);
                } else if mouse.column < menu_width * 3 {
                    // Settings menu
                    self.show_settings = !self.show_settings;
                    // Set view to Settings if dialog is shown
                    if self.show_settings {
                        self.view = AppView::Settings;
                    }
                    self.set_status(
                        if self.show_settings { "Settings opened" } else { "Settings closed" },
                        Color::Blue
                    );
                } else {
                    // Help menu
                    self.show_help = !self.show_help;
                    self.set_status(
                        if self.show_help { "Help opened" } else { "Help closed" },
                        Color::Blue
                    );
                }
            }
        }
        
        Ok(())
    }
    
    /// Handle mouse events in file browser view
    fn handle_file_browser_mouse(&mut self, mouse: MouseEvent, _area: Rect) -> Result<()> {
        // Check if we're clicking on a file entry
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            // Calculate which entry was clicked based on row
            // Assuming the file list starts at row 2 (0-indexed)
            if mouse.row >= 2 {
                let index = (mouse.row - 2) as usize;
                if index < self.file_browser.files.len() {
                    self.file_browser.selected = Some(index);
                    
                    // Store the selected entry for potential use
                    let selected_entry = self.file_browser.files.get(index).cloned();
                    
                    // Process the selected entry if available
                    if let Some(entry) = selected_entry {
                        if entry.is_dir {
                            self.navigate_to(&entry.path)?;
                        } else {
                            self.open_media(entry.path.to_str().unwrap_or_default())?;
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Handle mouse events in YouTube search view
    fn handle_youtube_search_mouse(&mut self, mouse: MouseEvent, _area: Rect) -> Result<()> {
        // Check if we're clicking on a search result
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            // Calculate which result was clicked based on row
            // Assuming the result list starts at row 3 (0-indexed)
            if mouse.row >= 3 {
                let index = (mouse.row - 3) as usize;
                if index < self.youtube_search.results.len() {
                    self.youtube_search.selected = Some(index);
                    
                    // Store the selected result for potential use
                    let selected_result = self.youtube_search.results.get(index).cloned();
                    
                    // Process the selected result if available
                    if let Some(result) = selected_result {
                        self.open_media(&result.id)?;
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Handle mouse events in settings view
    fn handle_settings_mouse(&mut self, _mouse: MouseEvent, _area: Rect) -> Result<()> {
        // Implement settings mouse handling
        Ok(())
    }
    
    /// Update application state
    pub fn update(&mut self) -> Result<()> {
        // Always keep UI visible - this prevents flickering issues
        self.show_ui = true;
        
        // Clear status message after timeout
        if let Some((_, time, _)) = &self.status_message {
            if time.elapsed() > Duration::from_secs(5) {
                self.status_message = None;
            }
        }
        
        // When Settings or Help dialogs are shown, make sure we're in the right view
        if self.show_settings && self.view != AppView::Settings {
            self.view = AppView::Settings;
        }
        
        // Make sure YouTube search view is properly set up
        if self.view == AppView::YoutubeSearch && self.youtube_search.results.is_empty() && !self.youtube_search.searching {
            // Initialize with some placeholder results if empty
            if self.youtube_search.results.is_empty() {
                // Add a placeholder message
                self.set_status("Enter search term or YouTube URL", Color::Yellow);
            }
        }
        
        // Check if we're in player view but don't have a player or media info
        // This can happen if something went wrong during playback initialization
        if self.view == AppView::Player && (self.player.is_none() || self.media_info.is_none()) {
            // If we've been in this state for a few seconds, likely something is wrong
            if self.last_ui_interaction.elapsed() > Duration::from_secs(3) {
                self.set_status("Media playback failed to initialize", Color::Red);
                // Return to main menu after a delay
                if self.last_ui_interaction.elapsed() > Duration::from_secs(5) {
                    self.view = AppView::MainMenu;
                    return Ok(());
                }
            }
        }
        
        Ok(())
    }
    
    /// Check if command mode is active
    pub fn is_command_mode(&self) -> bool {
        self.command_mode
    }
    
    /// Enter command mode
    pub fn enter_command_mode(&mut self) {
        self.command_mode = true;
        self.command_buffer.clear();
    }
    
    /// Exit command mode
    pub fn exit_command_mode(&mut self) {
        self.command_mode = false;
    }
    
    /// Get command buffer contents
    pub fn get_command_buffer(&self) -> &str {
        &self.command_buffer
    }
    
    /// Add character to command buffer
    pub fn add_to_command_buffer(&mut self, c: char) {
        self.command_buffer.push(c);
    }
    
    /// Remove last character from command buffer
    pub fn remove_from_command_buffer(&mut self) {
        self.command_buffer.pop();
    }
}