use anyhow::Result;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Tabs, Wrap,
    },
    Frame,
};

use std::{
    path::Path,
    time::{SystemTime},
};

use crate::app::{App, AppView};

/// Draw the main UI
pub fn draw_ui(f: &mut Frame, app: &mut App) -> Result<()> {
    // Get terminal size
    let size = f.area();
    
    // Clear any previous artifacts for player view only
    if app.view == AppView::Player {
        // Use a solid black background to prevent flickering
        let clear_block = Block::default().style(Style::default().bg(Color::Black));
        f.render_widget(clear_block, size);
    }
    
    // Draw different UI based on current view
    match app.view {
        AppView::MainMenu => draw_main_menu_view(f, app, size)?,
        AppView::Player => draw_player_view(f, app, size)?,
        AppView::FileBrowser => draw_file_browser_view(f, app, size)?,
        AppView::YoutubeSearch => draw_youtube_search_view(f, app, size)?,
        AppView::Settings => draw_settings_view(f, app, size)?,
    }
    
    // Draw status message if available
    if let Some((message, _, color)) = &app.status_message {
        draw_status_message(f, message, *color);
    }
    
    // Draw command mode if active
    if app.is_command_mode() {
        draw_command_prompt(f, app);
    }
    
    // Draw help dialog if shown
    if app.show_help {
        draw_help_dialog(f);
    }
    
    // Draw settings dialog if shown
    if app.show_settings {
        draw_settings_dialog(f, app);
    }
    
    Ok(())
}

/// Draw the main menu view
fn draw_main_menu_view(f: &mut Frame, _app: &mut App, area: Rect) -> Result<()> {
    // No imports needed, using direct text in paragraphs
    // Clear the area first
    f.render_widget(Block::default().style(Style::default().bg(Color::Black)), area);
    
    // Create layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Title
            Constraint::Min(2),      // Menu buttons
            Constraint::Length(1),   // Status bar
        ])
        .split(area);
    
    // Draw title with decorative border
    let title_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" TUI Video Player ")
        .title_alignment(Alignment::Center)
        .style(Style::default().bg(Color::Black));
    
    let title_text = Paragraph::new("Welcome to TUI Video Player")
        .block(title_block)
        .style(Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center);
    
    f.render_widget(title_text, chunks[0]);
    
    // Calculate button dimensions
    let button_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Local Files button
            Constraint::Length(1),  // Spacing
            Constraint::Length(3),  // YouTube button
            Constraint::Length(1),  // Spacing
            Constraint::Length(3),  // Settings button
            Constraint::Length(1),  // Spacing
            Constraint::Length(3),  // Help button
            Constraint::Length(1),  // Spacing
            Constraint::Length(3),  // Exit button
            Constraint::Min(0),     // Remaining space
        ])
        .margin(2)
        .split(chunks[1]);
    
    // Create centered horizontal layout for buttons
    let button_width = 30;
    let horizontal_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - button_width) / 2),
            Constraint::Percentage(button_width),
            Constraint::Percentage((100 - button_width) / 2),
        ]);
    
    // Local Files button
    let files_button_area = horizontal_layout.split(button_layout[0])[1];
    let files_button = Paragraph::new(" 1 - BROWSE LOCAL FILES ")
        .alignment(Alignment::Center)
        .style(Style::default()
            .fg(Color::White)
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightBlue))
            .style(Style::default().bg(Color::Blue)));
    
    f.render_widget(files_button, files_button_area);
    
    // YouTube button
    let youtube_button_area = horizontal_layout.split(button_layout[2])[1];
    let youtube_button = Paragraph::new(" 2 - SEARCH YOUTUBE ")
        .alignment(Alignment::Center)
        .style(Style::default()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightRed))
            .style(Style::default().bg(Color::Red)));
    
    f.render_widget(youtube_button, youtube_button_area);
    
    // Settings button
    let settings_button_area = horizontal_layout.split(button_layout[4])[1];
    let settings_button = Paragraph::new(" 3 - SETTINGS ")
        .alignment(Alignment::Center)
        .style(Style::default()
            .fg(Color::White)
            .bg(Color::Magenta)
            .add_modifier(Modifier::BOLD))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightMagenta))
            .style(Style::default().bg(Color::Magenta)));
    
    f.render_widget(settings_button, settings_button_area);
    
    // Help button
    let help_button_area = horizontal_layout.split(button_layout[6])[1];
    let help_button = Paragraph::new(" 4 - HELP ")
        .alignment(Alignment::Center)
        .style(Style::default()
            .fg(Color::White)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightCyan))
            .style(Style::default().bg(Color::Cyan)));
    
    f.render_widget(help_button, help_button_area);
    
    // Exit button
    let exit_button_area = horizontal_layout.split(button_layout[8])[1];
    let exit_button = Paragraph::new(" 5 - EXIT ")
        .alignment(Alignment::Center)
        .style(Style::default()
            .fg(Color::White)
            .bg(Color::Gray)
            .add_modifier(Modifier::BOLD))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White))
            .style(Style::default().bg(Color::Gray)));
    
    f.render_widget(exit_button, exit_button_area);
    
    // Draw status bar with help text
    let status_text = "F1: Help | Arrow Keys: Navigate | Enter: Select | Ctrl+Q: Quit";
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    
    f.render_widget(status, chunks[2]);
    
    Ok(())
}

/// Draw the player view with video and controls
fn draw_player_view(f: &mut Frame, app: &mut App, area: Rect) -> Result<()> {
    // Create layout - video is drawn by the renderer, we just need to draw controls on top
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),         // Top menu bar
            Constraint::Min(1),            // Video area (video rendering happens outside TUI)
            Constraint::Length(5),         // Controls area
        ])
        .split(area);
    
    // Draw top menu bar with title if available
    let title = if let Some(media_info) = &app.media_info {
        format!(" {} ({}x{})", 
            app.player.as_ref().and_then(|p| p.get_media_info()).map_or("Unknown".to_string(), |i| i.format_name.clone()),
            media_info.width, 
            media_info.height
        )
    } else {
        " Media Player".to_string()
    };
    
    // Create a visually distinct menu with high contrast
    let menu_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White).bg(Color::Black))
        .title(title)
        .title_alignment(Alignment::Center)
        .style(Style::default().bg(Color::Black)); // Ensure background is solid
        
    // First clear the area with black to prevent any transparency or artifacts
    f.render_widget(Block::default().style(Style::default().bg(Color::Black)), vertical[0]);
    f.render_widget(menu_block, vertical[0]);
    draw_menu_bar(f, app, vertical[0]);
    
    // Draw a strong background for controls to ensure visibility over video
    let controls_bg = Block::default()
        .style(Style::default().bg(Color::Black))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::White));
    f.render_widget(controls_bg, vertical[2]);
    
    // If we're still loading or don't have a valid player, show a loading message
    if app.player.is_none() || app.media_info.is_none() {
        // Get elapsed time for spinner animation
        let elapsed_ms = app.last_ui_interaction.elapsed().as_millis();
        let spinner = get_spinner_frame(elapsed_ms);
        
        // Show loading message in the video area with animated spinner
        let loading_message = Paragraph::new(format!("{} Loading media, please wait... {}", spinner, spinner))
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .block(Block::default()
                .borders(Borders::NONE)
                .style(Style::default().bg(Color::Black)));
        
        f.render_widget(loading_message, vertical[1]);
        
        // Add a progress indicator that moves over time to show the app hasn't frozen
        let progress_pct = (elapsed_ms % 3000) as f64 / 3000.0;
        let progress = Gauge::default()
            .block(Block::default().borders(Borders::NONE))
            .gauge_style(Style::default().fg(Color::Yellow).bg(Color::Black))
            .ratio(progress_pct)
            .label(format!("{} Loading... {}", spinner, spinner));
            
        // Split the video area to add the progress bar
        let video_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(45),
                Constraint::Length(1),
                Constraint::Percentage(45),
            ])
            .split(vertical[1]);
            
        f.render_widget(progress, video_split[1]);
        
        // Show placeholder controls
        let controls_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan).bg(Color::Black))
            .title(format!(" {} LOADING MEDIA... {} ", spinner, spinner))
            .title_alignment(Alignment::Center)
            .style(Style::default().bg(Color::Black));
            
        f.render_widget(controls_block, vertical[2]);
        
        // Draw a basic exit button
        let exit_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Percentage(20),
                Constraint::Percentage(40),
            ])
            .split(Block::default().inner(vertical[2]));
        
        let exit_button = Paragraph::new("CANCEL [ESC]")
            .alignment(Alignment::Center)
            .style(Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD))
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightRed))
                .style(Style::default().bg(Color::Red)));
                
        f.render_widget(exit_button, exit_layout[1]);
    } else if let Some(player) = &app.player {
        // Draw controls if we have a valid player
        if let Some(media_info) = &app.media_info {
            let controls_block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan).bg(Color::Black))
                .title(" PLAYBACK CONTROLS ")
                .title_alignment(Alignment::Center)
                .style(Style::default().bg(Color::Black)); // Solid background
                
            f.render_widget(controls_block, vertical[2]);
            draw_player_controls(f, app, player, media_info, vertical[2]);
        }
    }
    
    Ok(())
}

/// Draw the menu bar
fn draw_menu_bar(f: &mut Frame, app: &App, area: Rect) {
    // Get inner area inside block borders
    let inner_area = Block::default().inner(area);
    
    // Ensure the area has a solid background
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)), 
        inner_area
    );
    
    let menu_items = ["File", "YouTube", "Settings", "Help"];
    
    // Determine which tab to highlight based on current view
    let selected_tab = match app.view {
        AppView::FileBrowser => 0,
        AppView::YoutubeSearch => 1,
        AppView::Settings => 2,
        _ => 0, // Default to file browser
    };
    
    let tabs = Tabs::new(menu_items.iter().map(|t| Line::from(*t)).collect::<Vec<_>>())
        .block(Block::default().style(Style::default().bg(Color::Black)))
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .highlight_style(Style::default()
            .fg(Color::Yellow)
            .bg(Color::Black)
            .add_modifier(Modifier::BOLD))
        .select(selected_tab)
        .divider("|");
        
    f.render_widget(tabs, inner_area);
}

// Draw player controls
fn draw_player_controls(
    f: &mut Frame, 
    _app: &App, 
    player: &Box<dyn core::MediaPlayer>, 
    media_info: &core::MediaInfo, 
    area: Rect
) {
    // Import necessary items for this function
    use ratatui::text::Span;
    // Adjust area to account for borders
    let inner_area = Block::default().inner(area);
    
    // Add a solid background to the control area for better visibility
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)), 
        inner_area
    );
    
    // Create control layout
    let controls = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Progress bar
            Constraint::Length(1),  // Space
            Constraint::Length(1),  // Button row
            Constraint::Length(1),  // Status info
        ])
        .split(inner_area);
    
    // Calculate progress percentage
    let position = player.get_position();
    let duration = media_info.duration;
    let percent = if duration > 0.0 { (position / duration * 100.0) as u16 } else { 0 };
    
    // Format timestamps
    let position_str = format_time(position);
    let duration_str = format_time(duration);
    let time_text = format!("{} / {}", position_str, duration_str);
    
    // Create progress bar with enhanced visibility
    let gauge = Gauge::default()
        .block(Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(Color::Black)))
        .gauge_style(Style::default().fg(Color::LightGreen).bg(Color::DarkGray))
        .ratio(percent as f64 / 100.0)
        .label(Span::styled(
            time_text, 
            Style::default()
                .fg(Color::White)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD)
        ));
    
    f.render_widget(gauge, controls[0]);
    
    // Create button layout
    let button_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(5),   // Space
            Constraint::Length(10),      // Back button
            Constraint::Length(12),      // Play/Pause button
            Constraint::Length(10),      // Forward button
            Constraint::Percentage(5),   // Space
            Constraint::Length(25),      // Info
            Constraint::Length(15),      // Exit button
            Constraint::Percentage(5),   // Space
        ])
        .split(controls[2]);
    
    // Draw buttons with enhanced visibility
    let back_button = Paragraph::new("<<< BACK <<<")
        .alignment(Alignment::Center)
        .style(Style::default()
            .fg(Color::White)
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .title(" -10s ")
            .title_alignment(Alignment::Center)
            .style(Style::default().bg(Color::Blue)));
    
    let play_pause = if player.is_paused() {
        Paragraph::new("▶ PLAY ▶")
            .alignment(Alignment::Center)
            .style(Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD))
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
                .title(" Play ")
                .title_alignment(Alignment::Center)
                .style(Style::default().bg(Color::Green)))
    } else {
        Paragraph::new("⏸ PAUSE ⏸")
            .alignment(Alignment::Center)
            .style(Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD))
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
                .title(" Pause ")
                .title_alignment(Alignment::Center)
                .style(Style::default().bg(Color::Yellow)))
    };
    
    let forward_button = Paragraph::new(">>> FORWARD >>>")
        .alignment(Alignment::Center)
        .style(Style::default()
            .fg(Color::White)
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .title(" +10s ")
            .title_alignment(Alignment::Center)
            .style(Style::default().bg(Color::Blue)));
    
    // Add exit button
    let exit_button = Paragraph::new("EXIT PLAYER")
        .alignment(Alignment::Center)
        .style(Style::default()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
            .title(" ESC ")
            .title_alignment(Alignment::Center)
            .style(Style::default().bg(Color::Red)));
    
    f.render_widget(back_button, button_layout[1]);
    f.render_widget(play_pause, button_layout[2]);
    f.render_widget(forward_button, button_layout[3]);
    
    // Draw video info with enhanced visibility
    // Create info text with enhanced visibility
    let info_text = format!(
        "{}x{} | {} | {}",
        media_info.width,
        media_info.height,
        media_info.video_codec.to_uppercase(),
        if let Some(audio) = &media_info.audio_codec {
            format!("AUDIO: {}", audio.to_uppercase())
        } else {
            "NO AUDIO".to_string()
        }
    );
    
    let info = Paragraph::new(info_text)
        .alignment(Alignment::Center)
        .style(Style::default()
            .fg(Color::White)
            .bg(Color::Black))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Gray))
            .title(" INFO ")
            .title_alignment(Alignment::Center)
            .style(Style::default().bg(Color::Black)));
    
    f.render_widget(info, button_layout[5]);
    f.render_widget(exit_button, button_layout[6]);
    
    // Draw playback status with enhanced visibility
    let status = format!("STATUS: {} | {:.2}x SPEED", 
        if player.is_paused() { "PAUSED" } else { "PLAYING" },
        1.0 // Future: could support variable playback speed
    );
    
    let status_color = if player.is_paused() { Color::Yellow } else { Color::Green };
    let status_widget = Paragraph::new(status)
        .alignment(Alignment::Center)
        .style(Style::default()
            .fg(status_color)
            .bg(Color::Black)
            .add_modifier(Modifier::BOLD))
        .block(Block::default()
            .style(Style::default().bg(Color::Black)));
    
    f.render_widget(status_widget, controls[3]);
}

/// Draw the file browser view
fn draw_file_browser_view(f: &mut Frame, app: &mut App, area: Rect) -> Result<()> {
    // Clear the entire area first
    f.render_widget(Clear, area);
    
    // Create layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Menu bar
            Constraint::Length(3),   // Current directory
            Constraint::Min(2),      // File list
            Constraint::Length(1),   // Status bar
        ])
        .split(area);
    
    // Draw menu bar
    let menu_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White))
        .title(" File Browser ")
        .title_alignment(Alignment::Center);
    
    f.render_widget(menu_block, chunks[0]);
    draw_menu_bar(f, app, chunks[0]);
    
    // Draw current directory
    let current_dir = app.file_browser.current_dir.to_string_lossy();
    let dir_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue))
        .title(" Location ")
        .title_alignment(Alignment::Left);
    
    let dir_text = Paragraph::new(current_dir.to_string())
        .block(dir_block)
        .style(Style::default().fg(Color::Blue).bg(Color::Black));
    
    f.render_widget(dir_text, chunks[1]);
    
    // Create file list items
    let files: Vec<ListItem> = app.file_browser.files
        .iter()
        .map(|file| {
            let (icon, color) = if file.is_dir {
                ("📁 ", Color::Blue)
            } else if is_video_file(&file.path) {
                ("🎬 ", Color::Green)
            } else if is_audio_file(&file.path) {
                ("🎵 ", Color::Yellow)
            } else {
                ("📄 ", Color::White)
            };
            
            let size_str = if file.is_dir {
                String::from("<DIR>")
            } else {
                format_size(file.size)
            };
            
            let modified_str = if let Some(modified) = &file.modified {
                format_time_ago(*modified)
            } else {
                String::from("Unknown")
            };
            
            let name_len = file.name.len();
            let spaces = if name_len < 40 { 40 - name_len } else { 1 };
            
            let item_text = Line::from(vec![
                Span::styled(icon, Style::default().fg(color)),
                Span::styled(&file.name, Style::default().fg(color)),
                Span::raw(" ".repeat(spaces)),
                Span::styled(size_str, Style::default().fg(Color::Gray)),
                Span::raw("   "),
                Span::styled(modified_str, Style::default().fg(Color::Gray)),
            ]);
            
            ListItem::new(item_text)
        })
        .collect();
    
    // Create file list
    let mut list_state = ListState::default();
    list_state.select(app.file_browser.selected);
    
    let file_list = List::new(files)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    
    f.render_stateful_widget(file_list, chunks[2], &mut list_state);
    
    // Draw status bar with help text
    let status_text = "↑/↓: Navigate | Enter: Open | Backspace: Back | ESC: Main Menu | F1: Help | Ctrl+Q: Quit";
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    
    f.render_widget(status, chunks[3]);
    
    Ok(())
}

/// Draw the YouTube search view
fn draw_youtube_search_view(f: &mut Frame, app: &mut App, area: Rect) -> Result<()> {
    // Clear the area first
    f.render_widget(Block::default().style(Style::default().bg(Color::Black)), area);
    
    // Create layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Menu bar
            Constraint::Length(3),   // Search input
            Constraint::Min(2),      // Search results
            Constraint::Length(1),   // Status bar
        ])
        .split(area);
    
    // Draw title block with strong styling
    let title_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White))
        .title(" YouTube Search ")
        .title_alignment(Alignment::Center)
        .style(Style::default().bg(Color::Black));
    
    // Get inner area before rendering the block
    let inner_area = title_block.inner(chunks[0]);
    
    f.render_widget(title_block, chunks[0]);
    
    // Draw menu bar in the inner area we calculated
    draw_menu_bar(f, app, inner_area);
    
    // Draw search input with better styling
    let input_value = &app.input;
    let input_cursor_position = app.input_cursor;
    
    let input = Paragraph::new(input_value.clone())
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Search YouTube or enter video URL ")
            .style(Style::default().bg(Color::Black)));
    
    f.render_widget(input, chunks[1]);
    
    // Show cursor at the input position
    if app.view == AppView::YoutubeSearch {
        // Use set_cursor_position with a single position argument
        f.set_cursor_position(
            (chunks[1].x + input_cursor_position as u16 + 1, chunks[1].y + 1)
        );
    }
    
    // Create search result items
    let results: Vec<ListItem> = app.youtube_search.results
        .iter()
        .map(|result| {
            let item_text = Line::from(vec![
                Span::styled("▶ ", Style::default().fg(Color::Red)),
                Span::styled(&result.title, Style::default()),
                Span::raw(" "),
                Span::styled(&result.duration, Style::default().fg(Color::Gray)),
                Span::raw(" - "),
                Span::styled(&result.channel, Style::default().fg(Color::Blue)),
            ]);
            
            ListItem::new(item_text)
        })
        .collect();
    
    // Create result list with better styling
    let mut list_state = ListState::default();
    list_state.select(app.youtube_search.selected);
    
    let result_list = List::new(results)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Gray))
            .title(" Search Results ")
            .style(Style::default().bg(Color::Black)))
        .highlight_style(
            Style::default()
                .bg(Color::Red)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    
    f.render_stateful_widget(result_list, chunks[2], &mut list_state);
    
    // Show placeholder message if no results
    if app.youtube_search.results.is_empty() && !app.youtube_search.searching {
        let placeholder = Paragraph::new("Enter a search term or YouTube URL above and press Enter")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));
        
        f.render_widget(placeholder, chunks[2]);
    }
    
    // Draw status bar with keyboard shortcuts
    let status_text = "Enter: Search/Play | ↑/↓: Navigate | ESC: Main Menu | F1: Help | Ctrl+Q: Quit";
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    
    f.render_widget(status, chunks[3]);
    
    Ok(())
}

/// Draw the settings view
fn draw_settings_view(f: &mut Frame, app: &mut App, area: Rect) -> Result<()> {
    // Clear the area with a solid background
    f.render_widget(Block::default().style(Style::default().bg(Color::Black)), area);
    
    // Create layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Menu bar
            Constraint::Min(2),      // Settings content
            Constraint::Length(1),   // Status bar
        ])
        .split(area);
    
    // Draw title block with strong styling
    let title_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White))
        .title(" Settings ")
        .title_alignment(Alignment::Center)
        .style(Style::default().bg(Color::Black));
    
    // Get inner area before rendering the block
    let inner_area = title_block.inner(chunks[0]);
    
    f.render_widget(title_block, chunks[0]);
    
    // Draw menu bar in the inner area we calculated
    draw_menu_bar(f, app, inner_area);
    
    // Draw settings with better styling
    let settings_text = Text::from(vec![
        Line::from(vec![
            Span::styled("Video Settings:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("• GPU Acceleration: ", Style::default().fg(Color::Green)),
            Span::styled(
                if app.render_config.enable_gpu { "Enabled" } else { "Disabled" },
                Style::default().fg(if app.render_config.enable_gpu { Color::LightGreen } else { Color::LightRed })
            )
        ]),
        Line::from(vec![
            Span::styled("• Renderer: ", Style::default().fg(Color::Green)),
            Span::styled(format!("{:?}", app.render_config.method), Style::default().fg(Color::White)),
            Span::styled(" (Auto = default, best for your terminal)", Style::default().fg(Color::Gray))
        ]),
        Line::from(vec![
            Span::styled("• Target FPS: ", Style::default().fg(Color::Green)),
            Span::styled(format!("{}", app.render_config.target_fps), Style::default().fg(Color::White))
        ]),
        Line::from(vec![
            Span::styled("• Quality: ", Style::default().fg(Color::Green)),
            Span::styled(format!("{:.1}", app.render_config.quality), Style::default().fg(Color::White))
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("YouTube Settings:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("• Quality: ", Style::default().fg(Color::Green)),
            Span::styled(format!("{}", app.youtube_config.quality), Style::default().fg(Color::White))
        ]),
        Line::from(vec![
            Span::styled("• Format: ", Style::default().fg(Color::Green)),
            Span::styled(
                app.youtube_config.format.as_ref().unwrap_or(&"Auto".to_string()).to_string(),
                Style::default().fg(Color::White)
            )
        ]),
        Line::from(vec![
            Span::styled("• Max Resolution: ", Style::default().fg(Color::Green)),
            Span::styled(
                app.youtube_config.max_resolution.as_ref().unwrap_or(&"Auto".to_string()).to_string(),
                Style::default().fg(Color::White)
            )
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Press the number keys 1-7 to change settings", Style::default().fg(Color::DarkGray))
        ]),
    ]);
    
    let settings = Paragraph::new(settings_text)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Gray)))
        .wrap(Wrap { trim: true });
    
    f.render_widget(settings, chunks[1]);
    
    // Draw status bar with more informative text
    let status_text = "ESC: Back to Player | 1-7: Change Settings | F1: Help | Ctrl+Q: Quit";
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    
    f.render_widget(status, chunks[2]);
    
    Ok(())
}

// Draw status message
fn draw_status_message(f: &mut Frame, message: &str, color: Color) {
    let area = Rect::new(0, f.area().height - 1, f.area().width, 1);
    
    // Create a more visible status bar with enhanced contrast
    let status_block = Block::default()
        .style(Style::default().bg(Color::DarkGray))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Gray));
    
    f.render_widget(status_block, area);
    
    // Add a background to the text for better visibility
    // Make sure text stands out by adding stars for important messages
    let formatted_message = if color == Color::Red || color == Color::Yellow {
        format!(" !!! {} !!! ", message)
    } else {
        format!(" {} ", message)
    };
    
    let status = Paragraph::new(formatted_message)
        .style(Style::default()
            .fg(color)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center);
    
    f.render_widget(status, area);
}

/// Draw command prompt when in command mode
fn draw_command_prompt(f: &mut Frame, app: &App) {
    // Create an area at the bottom of the screen for the command prompt
    let area = Rect::new(0, f.area().height - 2, f.area().width, 1);
    
    // Create a solid black background first
    let bg_block = Block::default()
        .style(Style::default().bg(Color::Black));
    
    f.render_widget(bg_block, area);
    
    // Create a command prompt block with highly visible border
    let prompt_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));
    
    f.render_widget(&prompt_block, area);
    
    // Calculate inner area for text
    let inner_area = prompt_block.inner(area);
    
    // Create a string with a colon prefix followed by the command buffer
    let command_text = format!(":{}", app.get_command_buffer());
    
    // Create the command prompt paragraph with improved visibility
    let command = Paragraph::new(command_text)
        .style(Style::default()
            .fg(Color::Yellow)
            .bg(Color::Black)
            .add_modifier(Modifier::BOLD))
        .alignment(Alignment::Left);
    
    // Render the command prompt
    f.render_widget(command, inner_area);
    
    // Position the cursor at the end of the command text (accounting for borders)
    f.set_cursor_position(
        (inner_area.x + 1 + app.get_command_buffer().len() as u16, inner_area.y)
    );
}

/// Draw help dialog
fn draw_help_dialog(f: &mut Frame) {
    let area = centered_rect(60, 70, f.area());
    
    // Clear the area with a solid background for better visibility
    f.render_widget(Block::default().style(Style::default().bg(Color::Black)), area);
    f.render_widget(Clear, area);
    
    // Create the help content
    let help_text = Text::from(vec![
        Line::from(vec![
            Span::styled("▶ Keyboard Controls:", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Global:", Style::default().add_modifier(Modifier::BOLD).fg(Color::Green))
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("F1", Style::default().add_modifier(Modifier::BOLD).fg(Color::White)),
            Span::raw("          - Toggle help")
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Ctrl+Q", Style::default().add_modifier(Modifier::BOLD).fg(Color::White)),
            Span::raw("      - Quit application")
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Escape", Style::default().add_modifier(Modifier::BOLD).fg(Color::White)),
            Span::raw("      - Return to main menu (or close dialog)")
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(":menu", Style::default().add_modifier(Modifier::BOLD).fg(Color::White)),
            Span::raw("      - Return to main menu from anywhere")
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Main Menu:", Style::default().add_modifier(Modifier::BOLD).fg(Color::Green))
        ]),
        Line::from("  1 or f      - Browse local files"),
        Line::from("  2 or y      - Search YouTube"),
        Line::from("  3 or s      - Open settings"),
        Line::from("  4 or h      - Show help"),
        Line::from("  5 or q      - Quit application"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Player:", Style::default().add_modifier(Modifier::BOLD).fg(Color::Green))
        ]),
        Line::from("  Space       - Play/Pause"),
        Line::from("  Left/Right  - Seek backward/forward 5 seconds"),
        Line::from("  b/f         - Seek backward/forward 30 seconds"),
        Line::from("  o           - Open file browser"),
        Line::from("  y           - Open YouTube search"),
        Line::from("  s           - Open settings"),
        Line::from("  ESC         - Exit video and return to main menu"),
        Line::from(""),
        Line::from(vec![
            Span::styled("File Browser:", Style::default().add_modifier(Modifier::BOLD).fg(Color::Green))
        ]),
        Line::from("  Up/Down     - Navigate files"),
        Line::from("  Enter       - Open file/directory"),
        Line::from("  Backspace   - Go up a directory"),
        Line::from("  /           - Filter files"),
        Line::from("  ~           - Go to home directory"),
        Line::from("  ESC         - Return to main menu"),
        Line::from(""),
        Line::from(vec![
            Span::styled("YouTube:", Style::default().add_modifier(Modifier::BOLD).fg(Color::Green))
        ]),
        Line::from("  Enter       - Search or play selected video"),
        Line::from("  Up/Down     - Navigate search results"),
        Line::from("  ESC         - Return to main menu"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Mouse Controls:", Style::default().add_modifier(Modifier::BOLD).fg(Color::Green))
        ]),
        Line::from("  Click on progress bar - Seek to position"),
        Line::from("  Click on play/pause  - Toggle playback"),
        Line::from("  Click on back/forward - Seek backward/forward"),
        Line::from("  Click on EXIT button - Exit video and return to main menu"),
        Line::from("  Click on file/result - Select item"),
        Line::from("  Double-click        - Open file/play video"),
    ]);
    
    // Create a block with strong borders and background for visibility
    let help_block = Block::default()
        .borders(Borders::ALL)
        .title(" HELP ")
        .title_alignment(Alignment::Center)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
        
    f.render_widget(Clear, area);
    f.render_widget(&help_block, area);
    
    // Render text in the inner area
    let inner_area = help_block.inner(area);
    let help = Paragraph::new(help_text)
        .style(Style::default().bg(Color::Black))
        .wrap(Wrap { trim: true });
    
    f.render_widget(help, inner_area);
}

/// Draw the settings dialog
fn draw_settings_dialog(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 70, f.area());
    
    // Clear the area
    f.render_widget(Clear, area);
    
    // Create the settings content
    let settings_text = Text::from(vec![
        Line::from(vec![
            Span::styled("Video Settings:", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("1. GPU Acceleration: ", Style::default().fg(Color::Green)),
            Span::styled(
                if app.render_config.enable_gpu { "Enabled" } else { "Disabled" },
                Style::default().fg(if app.render_config.enable_gpu { Color::LightGreen } else { Color::Red })
            )
        ]),
        Line::from(vec![
            Span::styled("2. Renderer: ", Style::default().fg(Color::Green)),
            Span::styled(format!("{:?}", app.render_config.method), Style::default().fg(Color::White)),
            Span::styled(" (Auto = default)", Style::default().fg(Color::Gray))
        ]),
        Line::from(vec![
            Span::styled("3. Target FPS: ", Style::default().fg(Color::Green)),
            Span::styled(format!("{}", app.render_config.target_fps), Style::default().fg(Color::White))
        ]),
        Line::from(vec![
            Span::styled("4. Quality: ", Style::default().fg(Color::Green)),
            Span::styled(format!("{:.1}", app.render_config.quality), Style::default().fg(Color::White))
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("YouTube Settings:", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("5. Quality: ", Style::default().fg(Color::Green)),
            Span::styled(format!("{}", app.youtube_config.quality), Style::default().fg(Color::White))
        ]),
        Line::from(vec![
            Span::styled("6. Format: ", Style::default().fg(Color::Green)),
            Span::styled(
                app.youtube_config.format.as_ref().unwrap_or(&"Auto".to_string()).to_string(), 
                Style::default().fg(Color::White)
            )
        ]),
        Line::from(vec![
            Span::styled("7. Max Resolution: ", Style::default().fg(Color::Green)),
            Span::styled(
                app.youtube_config.max_resolution.as_ref().unwrap_or(&"Auto".to_string()).to_string(), 
                Style::default().fg(Color::White)
            )
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Interface Settings:", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("8. Auto-detect YouTube: ", Style::default().fg(Color::Green)),
            Span::styled(
                if app.auto_detect_youtube { "Enabled" } else { "Disabled" },
                Style::default().fg(if app.auto_detect_youtube { Color::LightGreen } else { Color::Red })
            )
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Press the number key to change a setting, or Escape to close", 
                        Style::default().fg(Color::DarkGray))
        ]),
    ]);
    
    // Create a block with strong borders and background for visibility
    let settings_block = Block::default()
        .borders(Borders::ALL)
        .title(" SETTINGS ")
        .title_alignment(Alignment::Center)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
        
    f.render_widget(Clear, area);
    f.render_widget(&settings_block, area);
    
    // Render text in the inner area
    let inner_area = settings_block.inner(area);
    let settings = Paragraph::new(settings_text)
        .style(Style::default().bg(Color::Black))
        .wrap(Wrap { trim: true });
    
    f.render_widget(settings, inner_area);
}

// Helper function to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Format seconds into a time string (HH:MM:SS)
fn format_time(seconds: f64) -> String {
    let hours = (seconds / 3600.0) as u32;
    let minutes = ((seconds % 3600.0) / 60.0) as u32;
    let secs = (seconds % 60.0) as u32;
    
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{:02}:{:02}", minutes, secs)
    }
}

/// Format file size into human-readable string
fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    
    if size >= GB {
        format!("{:.1} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

/// Format system time into a relative time string
fn format_time_ago(time: SystemTime) -> String {
    let now = SystemTime::now();
    
    if let Ok(duration) = now.duration_since(time) {
        let seconds = duration.as_secs();
        
        if seconds < 60 {
            format!("{} seconds ago", seconds)
        } else if seconds < 3600 {
            format!("{} minutes ago", seconds / 60)
        } else if seconds < 86400 {
            format!("{} hours ago", seconds / 3600)
        } else if seconds < 604800 {
            format!("{} days ago", seconds / 86400)
        } else if seconds < 2592000 {
            format!("{} weeks ago", seconds / 604800)
        } else if seconds < 31536000 {
            format!("{} months ago", seconds / 2592000)
        } else {
            format!("{} years ago", seconds / 31536000)
        }
    } else {
        String::from("Future time?")
    }
}

/// Create a loading spinner animation based on time
fn get_spinner_frame(duration_ms: u128) -> &'static str {
    // Use braille pattern characters for a smooth animation
    const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠚", "⠞", "⠖", "⠦", "⠴", "⠲", "⠳", "⠓"];
    let frame_idx = (duration_ms / 80) % SPINNER_FRAMES.len() as u128;
    SPINNER_FRAMES[frame_idx as usize]
}

/// Check if a file is a video file based on extension
fn is_video_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        matches!(ext.as_str(), "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "3gp" | "m4v")
    } else {
        false
    }
}

/// Check if a file is an audio file based on extension
fn is_audio_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        matches!(ext.as_str(), "mp3" | "ogg" | "wav" | "flac" | "aac" | "wma" | "m4a")
    } else {
        false
    }
}