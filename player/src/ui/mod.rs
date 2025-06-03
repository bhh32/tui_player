pub mod app;
pub mod components;

// Re-export components for easier access
pub use components::*;

use anyhow::Result;
use crate::app::{App, AppView};
use ratatui::Frame;

/// Draw the main UI
pub fn draw_ui(f: &mut Frame, app: &mut App) -> Result<()> {
    // Get terminal size
    let size = f.area();
    
    // Draw the appropriate view
    match app.view {
        AppView::MainMenu => app::draw_main_menu_view(f, app, size),
        AppView::Player => app::draw_player_view(f, app, size),
        AppView::FileBrowser => app::draw_file_browser_view(f, app, size),
        AppView::YoutubeSearch => app::draw_youtube_search_view(f, app, size),
        AppView::Settings => app::draw_settings_view(f, app, size),
    }
    
    // Draw status message if needed
    if let Some((msg, time, color)) = &app.status_message {
        let age = time.elapsed();
        app::draw_status_message(f, msg, *color, age);
    }
    
    // Draw command prompt if in command mode
    if app.is_command_mode() {
        app::draw_command_prompt(f, app.get_command_buffer());
    }
    
    // Draw help dialog if needed
    if app.show_help {
        app::draw_help_dialog(f);
    }
    
    // Draw settings dialog if needed
    if app.show_settings {
        app::draw_settings_dialog(f, app);
    }
    
    Ok(())
}