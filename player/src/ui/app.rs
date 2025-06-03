use crate::app::{App, AppView};
use crate::ui::components::{*, VolumeIndicator};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, List, ListItem, ListState, Wrap, Clear, Gauge},
    Frame,
};
use std::path::Path;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

/// Draw the main menu view
pub fn draw_main_menu_view(f: &mut Frame, _app: &App, area: Rect) {
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
    
    let title_text = Paragraph::new(Text::from("Welcome to TUI Video Player"))
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
    let files_button = Paragraph::new(Text::from(" 1 - BROWSE LOCAL FILES "))
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
    let youtube_button = Paragraph::new(Text::from(" 2 - SEARCH YOUTUBE "))
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
    let settings_button = Paragraph::new(Text::from(" 3 - SETTINGS "))
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
    let help_button = Paragraph::new(Text::from(" 4 - HELP "))
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
    let exit_button = Paragraph::new(Text::from(" 5 - EXIT "))
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
    let status_text = "F1: Help | Arrow Keys: Navigate | Enter: Select | Esc: Back | Ctrl+Q: Quit";
    let status = Paragraph::new(Text::from(status_text))
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    
    f.render_widget(status, chunks[2]);
}

/// Draw the player view with improved controls
pub fn draw_player_view(f: &mut Frame, app: &App, area: Rect) {
    // Create layout - video is drawn by the renderer, we just need to draw controls on top
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),         // Top menu bar
            Constraint::Min(1),            // Video area (video rendering happens outside TUI)
            Constraint::Length(7),         // Controls area (increased for better controls)
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
        let loading_message = Paragraph::new(Text::from(format!("{} Loading media, please wait... {}", spinner, spinner)))
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
        
        let exit_button = Paragraph::new(Text::from("CANCEL [ESC]"))
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
    } else if let Some(_player) = &app.player {
        // Draw controls if we have a valid player
        if let Some(_media_info) = &app.media_info {
            draw_player_controls(f, app, vertical[2]);
        }
    }
}

/// Draw the menu bar
pub fn draw_menu_bar(f: &mut Frame, app: &App, area: Rect) {
    // Get inner area inside block borders
    let inner_area = Block::default().inner(area);
    
    // Ensure the area has a solid background
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)), 
        inner_area
    );
    
    // Create menu tabs
    let menu_tabs = Paragraph::new(Line::from(vec![
        Span::styled("[F]ile", Style::default().fg(if app.view == AppView::FileBrowser { Color::Yellow } else { Color::White })),
        Span::raw(" | "),
        Span::styled("[Y]ouTube", Style::default().fg(if app.view == AppView::YoutubeSearch { Color::Yellow } else { Color::White })),
        Span::raw(" | "),
        Span::styled("[S]ettings", Style::default().fg(if app.view == AppView::Settings { Color::Yellow } else { Color::White })),
        Span::raw(" | "),
        Span::styled("[H]elp", Style::default().fg(if app.show_help { Color::Yellow } else { Color::White })),
    ]))
    .alignment(Alignment::Center);
    
    f.render_widget(menu_tabs, inner_area);
}

/// Draw enhanced player controls
pub fn draw_player_controls(f: &mut Frame, app: &App, area: Rect) {
    // Get player state
    let player = match &app.player {
        Some(p) => p,
        None => return,
    };
    
    let media_info = match &app.media_info {
        Some(m) => m,
        None => return,
    };
    
    // Get current playback state
    let position = player.get_position();
    let duration = media_info.duration;
    let is_paused = player.is_paused();
    
    // Get buffer information if available
    let buffered_position = app.buffer_status.map(|(_, _, pos)| pos);
    
    // Create control layout
    let controls = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // Progress bar
            Constraint::Length(1),  // Space
            Constraint::Length(3),  // Playback controls
            Constraint::Length(1),  // Status info
        ])
        .split(area);
    
    // Create enhanced progress bar with buffer info
    let progress_bar = ProgressBar::new(position, duration)
        .paused(is_paused)
        .buffered_to(buffered_position)
        .title(Some(&media_info.format_name));
    
    f.render_widget(progress_bar, controls[0]);
    
    // Create playback controls
    let at_start = position < 5.0;
    let at_end = position > duration - 5.0;
    
    // Use the player_control_selected field to determine which button is selected
    let playback_controls = PlaybackControls::new(is_paused)
        .can_rewind(!at_start)
        .can_fast_forward(!at_end)
        .selected(app.player_control_selected)
        .show_keyboard_hints(true);
    
    f.render_widget(playback_controls, controls[2]);
    
    // Add volume indicator with actual volume data from player
    let volume = app.player.as_ref().map(|p| p.get_volume()).unwrap_or(50) as u8;
    let muted = app.player.as_ref().map(|p| p.is_muted()).unwrap_or(false);
    let volume_indicator = VolumeIndicator::new(volume, muted);
    
    // Render in top-right corner
    let volume_area = Rect::new(
        area.right() - 10, 
        area.top() + 1,
        8,
        1
    );
    f.render_widget(volume_indicator, volume_area);
    
    // Draw video info at the bottom
    let info_text = format!(
        "{}x{} | {} | {}",
        media_info.width,
        media_info.height,
        media_info.video_codec,
        media_info.audio_codec.as_deref().unwrap_or("No Audio")
    );
    
    let info = Paragraph::new(Text::from(info_text))
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    
    f.render_widget(info, controls[3]);
}

/// Draw the file browser view
pub fn draw_file_browser_view(f: &mut Frame, app: &App, area: Rect) {
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
    
    let dir_text = Paragraph::new(Text::from(current_dir.to_string()))
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
            
            let name_len = file.name.len();
            let spaces = if name_len < 40 { 40 - name_len } else { 1 };
            
            ListItem::new(Line::from(vec![
                Span::styled(icon, Style::default().fg(color)),
                Span::styled(&file.name, Style::default().fg(color)),
                Span::raw(" ".repeat(spaces)),
            ]))
        })
        .collect();
    
    // Create file list
    let mut list_state = ListState::default();
    list_state.select(app.file_browser.selected);
    
    let file_list = List::new(files)
        .block(Block::default().borders(Borders::ALL).title(" Files "))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    
    f.render_stateful_widget(file_list, chunks[2], &mut list_state);
    
    // Draw status bar with help text
    let status_text = "↑/↓: Navigate | Enter: Open | Backspace: Back | ESC: Main Menu | F1: Help | Ctrl+Q: Quit";
    let status = Paragraph::new(Text::from(status_text))
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    
    f.render_widget(status, chunks[3]);
}

/// Draw the YouTube search view
pub fn draw_youtube_search_view(f: &mut Frame, app: &App, area: Rect) {
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
    
    f.render_widget(title_block, chunks[0]);
    draw_menu_bar(f, app, chunks[0]);
    
    // Create search input with better styling and status indication
    let input_value = &app.input;
    let input_cursor_position = app.input_cursor;
    
    // Change border color based on search state
    let border_color = if app.youtube_search.searching {
        Color::LightYellow  // Bright yellow during search
    } else {
        Color::Yellow
    };
    
    let title = if app.youtube_search.searching {
        " Searching... " 
    } else {
        " Search YouTube or enter video URL "
    };
    
    let input = Paragraph::new(Text::from(input_value.clone()))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(title)
            .style(Style::default().bg(Color::Black)));
    
    f.render_widget(input, chunks[1]);
    
    // Show cursor at the input position
    if app.view == AppView::YoutubeSearch {
        f.set_cursor_position((chunks[1].x + input_cursor_position as u16 + 1, chunks[1].y + 1));
    }
    
    // Create search result items with thumbnails
    let results: Vec<ListItem> = app.youtube_search.results
        .iter()
        .map(|result| {
            // Use real thumbnail representation based on download status
            let thumbnail_lines = if result.has_downloaded_thumbnail() {
                // Actually downloaded thumbnail
                vec![
                    "┌─────────┐",
                    "│ ▓▓▓▓▓▓▓ │",
                    "│ ▓▓HD▓▓▓ │", // Show HD to indicate a real downloaded thumbnail
                    "│ ▓▓▓▓▓▓▓ │",
                    "└─────────┘",
                ]
            } else if result.thumbnail.is_some() {
                // Thumbnail URL exists but not downloaded yet
                vec![
                    "┌─────────┐",
                    "│ ▒▒▒▒▒▒▒ │",
                    "│ ▒⟳LOAD▒ │", // Loading indicator
                    "│ ▒▒▒▒▒▒▒ │",
                    "└─────────┘",
                ]
            } else {
                // No thumbnail available
                vec![
                    "┌─────────┐",
                    "│ No      │",
                    "│ Thumb-  │",
                    "│ nail    │",
                    "└─────────┘",
                ]
            };
            
            // Convert thumbnail art to text lines with appropriate colors
            let thumbnail_text_lines: Vec<Line> = thumbnail_lines.iter()
                .map(|line| {
                    let color = if result.has_downloaded_thumbnail() {
                        Color::LightGreen  // Use green for downloaded thumbnails
                    } else if result.thumbnail.is_some() {
                        Color::Yellow      // Use yellow for pending downloads
                    } else {
                        Color::Gray        // Use gray for missing thumbnails
                    };
                    Line::from(vec![Span::styled(*line, Style::default().fg(color))])
                })
                .collect();
            
            // Create a multi-line item with title, duration, and channel
            let content_lines = vec![
                Line::from(vec![
                    Span::styled("▶ ", Style::default().fg(Color::Red)),
                    Span::styled(&result.title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled("Duration: ", Style::default().fg(Color::Gray)),
                    Span::styled(&result.duration, Style::default().fg(Color::Yellow)),
                ]),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled("Channel: ", Style::default().fg(Color::Gray)),
                    Span::styled(&result.channel, Style::default().fg(Color::Blue)),
                ]),
            ];
            
            // Create a combined item with thumbnail and content
            let mut lines = Vec::new();
            
            // Add the thumbnail lines on the left side
            for (i, thumbnail_line) in thumbnail_text_lines.into_iter().enumerate() {
                if i < content_lines.len() {
                    // Combine with content line
                    lines.push(Line::from(vec![
                        thumbnail_line.spans[0].clone(),
                        Span::raw("  "),
                        content_lines[i].spans[0].clone(),
                    ]));
                } else {
                    // Just the thumbnail line
                    lines.push(thumbnail_line);
                }
            }
            
            // Add spacing between items
            lines.push(Line::from(""));
            
            ListItem::new(lines)
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
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("➤➤ ");
    
    f.render_stateful_widget(result_list, chunks[2], &mut list_state);
    
    // Show appropriate message based on search status
    if app.youtube_search.results.is_empty() || app.youtube_search.searching {
        let message = if app.youtube_search.searching {
            "Searching YouTube... Please wait"
        } else {
            "Enter a search term or YouTube URL above and press Enter"
        };
        
        let placeholder = Paragraph::new(Text::from(message))
            .alignment(Alignment::Center)
            .style(Style::default().fg(if app.youtube_search.searching { Color::Yellow } else { Color::DarkGray }));
        
        f.render_widget(placeholder, chunks[2]);
    }
    
    // Draw status bar with keyboard shortcuts
    let status_text = "Enter: Search/Play | ↑/↓: Navigate | Tab: Focus | ESC: Main Menu | F1: Help | Ctrl+Q: Quit";
    let status = Paragraph::new(Text::from(status_text))
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    
    f.render_widget(status, chunks[3]);
}

// The handler for player control selection is now implemented in the App struct

/// Draw the settings view
pub fn draw_settings_view(f: &mut Frame, app: &App, area: Rect) {
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
    
    f.render_widget(title_block, chunks[0]);
    draw_menu_bar(f, app, chunks[0]);
    
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
    let status_text = "ESC: Back to Player | 1-7: Change Settings | ↑/↓: Navigate | F1: Help | Ctrl+Q: Quit";
    let status = Paragraph::new(Text::from(status_text))
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    
    f.render_widget(status, chunks[2]);
}

/// Draw status message with fade effect
pub fn draw_status_message(f: &mut Frame, message: &str, color: Color, age: Duration) {
    let max_age = Duration::from_secs(3);
    let status_message = StatusMessage::new(message, color, age)
        .max_age(max_age);
    
    // Create a centered floating box for the message
    let area = f.area();
    let message_width = message.width() as u16 + 4; // Add space for borders
    let message_height = 3; // 1 line of text + 2 for borders
    
    let message_area = Rect {
        x: area.x + (area.width.saturating_sub(message_width)) / 2,
        y: area.y + area.height.saturating_sub(10), // Show near the bottom
        width: message_width.min(area.width),
        height: message_height,
    };
    
    f.render_widget(status_message, message_area);
}

/// Draw command prompt
pub fn draw_command_prompt(f: &mut Frame, command: &str) {
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
    let command_text = format!(":{}", command);
    
    // Create the command prompt paragraph with improved visibility
    let command_para = Paragraph::new(Text::from(command_text))
        .style(Style::default()
            .fg(Color::Yellow)
            .bg(Color::Black)
            .add_modifier(Modifier::BOLD))
        .alignment(Alignment::Left);
    
    // Render the command prompt
    f.render_widget(command_para, inner_area);
    
    // Position the cursor at the end of the command text (accounting for borders)
    f.set_cursor_position((inner_area.x + 1 + command.len() as u16, inner_area.y));
}

/// Draw help dialog
pub fn draw_help_dialog(f: &mut Frame) {
    let area = centered_rect(60, 70, f.area());
    
    // Clear the area
    f.render_widget(Clear, area);
    
    // Create help overlay
    let help = HelpOverlay::new(true);
    
    f.render_widget(help, area);
}

/// Draw settings dialog
pub fn draw_settings_dialog(f: &mut Frame, app: &App) {
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

/// Helper function to create a centered rect
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

/// Check if a file is a video file based on extension
pub fn is_video_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        matches!(ext.as_str(), "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "3gp" | "m4v")
    } else {
        false
    }
}

/// Check if a file is an audio file based on extension
pub fn is_audio_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        matches!(ext.as_str(), "mp3" | "ogg" | "wav" | "flac" | "aac" | "wma" | "m4a")
    } else {
        false
    }
}