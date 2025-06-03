use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Widget, Wrap, Clear, Gauge},
    buffer::Buffer,
};
use unicode_width::UnicodeWidthStr;
use std::time::Duration;

/// Format duration as HH:MM:SS
pub fn format_duration(duration: f64) -> String {
    let total_seconds = duration.round() as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

/// Enhanced progress bar with playback indicators
pub struct ProgressBar<'a> {
    position: f64,
    duration: f64,
    is_paused: bool,
    buffered_to: Option<f64>,
    title: Option<&'a str>,
}

impl<'a> ProgressBar<'a> {
    pub fn new(position: f64, duration: f64) -> Self {
        Self {
            position,
            duration,
            is_paused: false,
            buffered_to: None,
            title: None,
        }
    }
    
    pub fn paused(mut self, is_paused: bool) -> Self {
        self.is_paused = is_paused;
        self
    }
    
    pub fn buffered_to(mut self, position: Option<f64>) -> Self {
        self.buffered_to = position;
        self
    }
    
    pub fn title(mut self, title: Option<&'a str>) -> Self {
        self.title = title;
        self
    }
}

impl<'a> Widget for ProgressBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // First render the basic gauge
        let percent = if self.duration > 0.0 {
            (self.position / self.duration).clamp(0.0, 1.0)
        } else {
            0.0
        };
        
        // Create the label with position/duration
        let pos_text = format_duration(self.position);
        let dur_text = format_duration(self.duration);
        let label = format!("{} / {}", pos_text, dur_text);
        
        // Add pause/play indicator to title
        let display_title = match (self.is_paused, self.title) {
            (true, Some(title)) => format!("⏸  {} ", title),
            (false, Some(title)) => format!("▶  {} ", title),
            (true, None) => "⏸  Paused ".to_string(),
            (false, None) => "▶  Playing ".to_string(),
        };
        
        // Create the gauge
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(display_title))
            .gauge_style(
                Style::default()
                    .fg(Color::Blue)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .percent((percent * 100.0) as u16)
            .label(label);
        
        // Render the gauge
        gauge.render(area, buf);
        
        // If we have buffer information, show it as a lighter background color
        if let Some(buffered) = self.buffered_to {
            if buffered > self.position && self.duration > 0.0 {
                let buffer_percent = (buffered / self.duration).clamp(0.0, 1.0);
                let progress_percent = (self.position / self.duration).clamp(0.0, 1.0);
                
                // Calculate the buffer bar area (between playback position and buffered position)
                let inner_width = area.width.saturating_sub(2) as f64;
                let start_x = area.x + 1 + (inner_width * progress_percent) as u16;
                let end_x = area.x + 1 + (inner_width * buffer_percent) as u16;
                
                // Don't render if there's no visible difference
                if end_x > start_x {
                    for x in start_x..end_x {
                        // Only draw inside the gauge
                        if x >= area.x + 1 && x < area.x + area.width - 1 {
                            for y in area.y + 1..area.y + area.height - 1 {
                                buf[(x, y)].set_bg(Color::DarkGray);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Enhanced playback control button row
pub struct PlaybackControls {
    is_playing: bool,
    can_rewind: bool,
    can_fast_forward: bool,
    selected: Option<usize>,
    show_keyboard_hints: bool,
}

impl PlaybackControls {
    pub fn new(is_playing: bool) -> Self {
        Self {
            is_playing,
            can_rewind: true,
            can_fast_forward: true,
            selected: None,
            show_keyboard_hints: true,
        }
    }
    
    pub fn can_rewind(mut self, can_rewind: bool) -> Self {
        self.can_rewind = can_rewind;
        self
    }
    
    pub fn can_fast_forward(mut self, can_fast_forward: bool) -> Self {
        self.can_fast_forward = can_fast_forward;
        self
    }
    
    pub fn selected(mut self, index: Option<usize>) -> Self {
        self.selected = index;
        self
    }
    
    pub fn show_keyboard_hints(mut self, show: bool) -> Self {
        self.show_keyboard_hints = show;
        self
    }
}

impl Widget for PlaybackControls {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Define the buttons to display
        let buttons = vec![
            ("⏪", "Back 30s", self.can_rewind, 'b'),
            ("◀◀", "Back 5s", self.can_rewind, '◀'),
            (if self.is_playing { "⏸" } else { "▶" }, if self.is_playing { "Pause" } else { "Play" }, true, ' '),
            ("▶▶", "Forward 5s", self.can_fast_forward, '▶'),
            ("⏩", "Forward 30s", self.can_fast_forward, 'f'),
        ];
        
        // Create constraints for the buttons (equal width)
        let constraints = vec![Constraint::Percentage(20); buttons.len()];
        
        // Create layout for the buttons
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);
        
        // Render each button
        for (i, ((symbol, tooltip, enabled, key), chunk)) in buttons.into_iter().zip(chunks.iter()).enumerate() {
            let selected = self.selected.map(|idx| idx == i).unwrap_or(false);
            
            // Set styles based on state
            let style = if !enabled {
                Style::default().fg(Color::DarkGray)
            } else if selected {
                Style::default().fg(Color::White).bg(Color::Blue).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            
            // Create block for the button
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(if selected { style } else { Style::default() });
            
            // Create the button content
            let key_hint = if self.show_keyboard_hints {
                format!(" ({})", key)
            } else {
                "".to_string()
            };
            
            let content = Paragraph::new(Line::from(vec![
                Span::styled(symbol, style.add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {}{}", tooltip, key_hint), style),
            ]))
            .alignment(Alignment::Center)
            .block(block);
            
            content.render(*chunk, buf);
        }
    }
}

/// Display a status message with fade effect
pub struct StatusMessage<'a> {
    message: &'a str,
    color: Color,
    age: Duration,
    max_age: Duration,
}

impl<'a> StatusMessage<'a> {
    pub fn new(message: &'a str, color: Color, age: Duration) -> Self {
        Self {
            message,
            color,
            age,
            max_age: Duration::from_secs(3),  // Default fade after 3 seconds
        }
    }
    
    pub fn max_age(mut self, duration: Duration) -> Self {
        self.max_age = duration;
        self
    }
}

impl<'a> Widget for StatusMessage<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Calculate opacity based on age
        let fade_factor = if self.age > self.max_age {
            0.0  // Fully invisible
        } else {
            1.0 - (self.age.as_secs_f32() / self.max_age.as_secs_f32())
        };
        
        // Don't render if fully faded
        if fade_factor <= 0.0 {
            return;
        }
        
        // Choose color based on fade level
        let color = match (self.color, fade_factor) {
            (Color::Red, _) => Color::Red,  // Errors always stay red
            (_, f) if f > 0.7 => self.color,  // Regular color for newer messages
            (_, f) if f > 0.3 => Color::DarkGray,  // Fade to dark gray
            _ => Color::DarkGray,  // Final fade stage
        };
        
        // Create the message box
        let text = Paragraph::new(Text::from(self.message))
            .style(Style::default().fg(color))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(color))
                    .style(Style::default().bg(Color::Black)),
            );
        
        // Render the message in a floating box
        let message_width = self.message.width() as u16 + 4;  // Add space for borders
        let message_height = 3;  // 1 line of text + 2 for borders
        
        let message_area = Rect {
            x: area.x + (area.width.saturating_sub(message_width)) / 2,
            y: area.y + area.height.saturating_sub(10),  // Show near the bottom
            width: message_width.min(area.width),
            height: message_height,
        };
        
        // Draw a transparent background
        Clear.render(message_area, buf);
        
        // Render the message
        text.render(message_area, buf);
    }
}

/// Volume indicator
pub struct VolumeIndicator {
    volume: u8,  // 0-100
    muted: bool,
}

impl VolumeIndicator {
    pub fn new(volume: u8, muted: bool) -> Self {
        Self {
            volume: volume.min(100),
            muted,
        }
    }
}

impl Widget for VolumeIndicator {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Choose the appropriate volume icon
        let (icon, color) = if self.muted {
            ("🔇", Color::DarkGray)
        } else if self.volume == 0 {
            ("🔇", Color::White)
        } else if self.volume < 30 {
            ("🔈", Color::White)
        } else if self.volume < 70 {
            ("🔉", Color::White)
        } else {
            ("🔊", Color::White)
        };
        
        // Create the volume display
        let vol_text = if self.muted {
            format!("{} Muted", icon)
        } else {
            format!("{} {}%", icon, self.volume)
        };
        
        let volume = Paragraph::new(Text::from(vol_text))
            .style(Style::default().fg(color))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Volume")
            );
        
        volume.render(area, buf);
    }
}

/// Simple help overlay widget
pub struct HelpOverlay {
    show_advanced: bool,
}

impl HelpOverlay {
    pub fn new(show_advanced: bool) -> Self {
        Self { show_advanced }
    }
}

impl Widget for HelpOverlay {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Create the help text
        let help_text = vec![
            Line::from(vec![
                Span::styled("Keyboard Controls", Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED))
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Space", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" - Play/Pause"),
            ]),
            Line::from(vec![
                Span::styled("←/→", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" - Seek 5 seconds"),
            ]),
            Line::from(vec![
                Span::styled("b/f", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" - Seek 30 seconds back/forward"),
            ]),
            Line::from(vec![
                Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" - Stop playback/Return to menu"),
            ]),
        ];
        
        // Add advanced help if requested
        let full_help = if self.show_advanced {
            let mut advanced = help_text;
            advanced.push(Line::from(""));
            advanced.push(Line::from(vec![
                Span::styled("Advanced Controls", Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED))
            ]));
            advanced.push(Line::from(vec![
                Span::styled("s", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" - Settings"),
            ]));
            advanced.push(Line::from(vec![
                Span::styled("o", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" - Open file browser"),
            ]));
            advanced.push(Line::from(vec![
                Span::styled("y", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" - YouTube search"),
            ]));
            advanced.push(Line::from(vec![
                Span::styled("h", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" - Toggle help"),
            ]));
            advanced
        } else {
            help_text
        };
        
        // Create the help overlay
        let help = Paragraph::new(Text::from(full_help))
            .block(Block::default().title("Help").borders(Borders::ALL))
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });
        
        // Clear the background
        Clear.render(area, buf);
        
        // Render the help text
        help.render(area, buf);
    }
}

/// Get a spinner frame for loading animations
pub fn get_spinner_frame(duration_ms: u128) -> &'static str {
    // Use braille pattern characters for a smooth animation
    const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠚", "⠞", "⠖", "⠦", "⠴", "⠲", "⠳", "⠓"];
    let frame_idx = (duration_ms / 80) % SPINNER_FRAMES.len() as u128;
    SPINNER_FRAMES[frame_idx as usize]
}