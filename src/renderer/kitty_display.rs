//! Kitty protocol display handler

use crate::renderer::types::*;
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use crossterm::{
    event::{Event, EventStream, KeyEvent},
    terminal,
};
use futures_util::stream::StreamExt;
use kitty_image::{
    Action, ActionDelete, ActionPut, ActionTransmission, Command, Format, Medium, WrappedCommand,
};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::{select, sync::RwLock};

/// Kitty protocol display handler
#[derive(Clone)]
pub struct KittyDisplay {
    config: DisplayConfig,
    stats: Arc<RwLock<DisplayStats>>,
    terminal_size: (u16, u16),
    image_id_counter: Arc<RwLock<u32>>,
}

impl KittyDisplay {
    /// Create new Kitty display handler
    pub fn new(config: DisplayConfig) -> Result<Self> {
        // Detect terminal size
        let terminal_size = config
            .terminal_size
            .unwrap_or_else(|| terminal::size().unwrap_or((80, 24)));

        log::info!(
            "Kitty display initialized - Terminal size: {}x{}",
            terminal_size.0,
            terminal_size.1
        );

        Ok(Self {
            config,
            stats: Arc::new(RwLock::new(DisplayStats::default())),
            terminal_size,
            image_id_counter: Arc::new(RwLock::new(1)),
        })
    }

    /// Main display loop - runs in dedicated thread
    pub async fn run_display_loop(
        &mut self,
        frame_rx: Receiver<ProcessedFrame>,
        cmd_rx: Receiver<PlaybackCommand>,
        status_tx: Sender<RenderStatus>,
    ) -> Result<()> {
        log::info!("Kitty display loop started");

        // Check Kitty protocol support
        if !self.check_kitty_support().await? {
            log::warn!("Kitty protocol not supported, falling back to basic mode");
        }

        // Clear terminal and setup
        self.setup_terminal().await?;

        // Create event stream for terminal events
        let mut event_stream = EventStream::new();

        let display_start = Instant::now();
        let mut frames_displayed = 0u64;
        let mut last_frame_time = Instant::now();
        let mut last_displayed_frame: Option<ProcessedFrame> = None;

        // Frame timing control for 30fps
        let target_fps = 30.0;
        let frame_duration = Duration::from_secs_f64(1.0 / target_fps);
        let mut next_frame_time = Instant::now();
        let mut playback_paused = false;

        loop {
            select! {
                // Handle terminal events (including resize)
                maybe_event = event_stream.next() => {
                    if let Some(Ok(event)) = maybe_event {
                        match event {
                            Event::Resize(cols, rows) => {
                                self.handle_terminal_resize(cols, rows, &last_displayed_frame).await?;
                            },
                            Event::Key(KeyEvent { .. }) => {
                                // Handle key events (like 'q' for quit) if needed

                                // For now, just logging
                                log::debug!("Key event received in display loop");
                            },
                            _ => {}
                        }
                    }
                }

                // Handle frame display
                _ = tokio::time::sleep(Duration::from_millis(1)) => {
                    let now = Instant::now();

                    // Check for commands (non-blocking)
                    if let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            PlaybackCommand::Stop => {
                                log::info!("Kitty display received stop command");
                                break;
                            },
                            PlaybackCommand::Play => {
                                playback_paused = false;
                                next_frame_time = now; // Reset timing
                                log::info!("Playback resumed");
                            },
                            PlaybackCommand::Pause => {
                                playback_paused = true;
                                log::info!("Playback Paused");
                            },
                            _ => {
                                log::debug!("Kitty display received command: {cmd:?}");
                            }
                        }
                    }
                    if !playback_paused && now > next_frame_time {
                        // Try to receive frame (non-blocking)
                        if let Ok(frame) = frame_rx.try_recv() {
                            // Display the frame
                            match self.display_frame(&frame).await {
                                Ok(_) => {
                                    frames_displayed += 1;
                                    last_displayed_frame = Some(frame.clone());

                                    // Calculate next frame time for precise timing
                                    next_frame_time = now + frame_duration;

                                    // Update stats
                                    self.update_display_stats(frames_displayed, display_start, last_frame_time).await;

                                    // Send status update periodically
                                    if frames_displayed % 30 == 0 {
                                        let stats = self.stats.read().await;
                                        let _ = status_tx.send(RenderStatus::DisplayStatus {
                                            frames_rendered: frames_displayed,
                                            display_fps: stats.display_fps,
                                            terminal_size: self.terminal_size,
                                        });
                                    }

                                    last_frame_time = Instant::now();
                                },
                                Err(e) => {
                                    log::error!("Failed to display frame {}: {e}", frame.frame_number);
                                    let _ = status_tx.send(RenderStatus::Error(format!("Display error: {e}")));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Cleanup
        self.cleanup_terminal().await?;
        log::info!("Kitty display loop completed");

        Ok(())
    }

    /// Handle terminal resize events
    async fn handle_terminal_resize(
        &mut self,
        new_cols: u16,
        new_rows: u16,
        last_frame: &Option<ProcessedFrame>,
    ) -> Result<()> {
        let old_size = self.terminal_size;
        self.update_terminal_size((new_cols, new_rows));

        log::info!(
            "Terminal resized: {}x{} -> {}x{}",
            old_size.0,
            old_size.1,
            new_cols,
            new_rows
        );

        // Clear terminal to avoid display artifacts
        self.clear_terminal().await?;

        // If we have a frame displayed, re-display it with new dimensions
        if let Some(frame) = last_frame {
            log::debug!("Re-displaying frame {} after resize", frame.frame_number);

            // Re-display the last frame with new terminal dimensions
            if let Err(e) = self.display_frame(frame).await {
                log::error!("Failed to re-display frame after resize: {e}");
            }
        }

        Ok(())
    }

    /// Clear terminal content
    async fn clear_terminal(&self) -> Result<()> {
        // Clear all images
        print!("\x1b_Ga=d;\x1b\\");

        // Clear screen content
        print!("\x1b[2J\x1b[H");

        io::stdout().flush()?;

        Ok(())
    }

    /// Check if terminal supports Kitty graphics protocol
    async fn check_kitty_support(&self) -> Result<bool> {
        // Check if this is a Kitty supported terminal
        if let Ok(term) = std::env::var("TERM") {
            let is_kitty = term.contains("kitty");

            log::info!("Terminal type: {term}, Kitty support: {is_kitty}");

            if is_kitty {
                // Send a query to check graphics support
                let query_action = Action::Query;
                let query_command = Command::new(query_action);
                let wrapped_query = WrappedCommand::new(query_command);

                print!("{wrapped_query}");
                io::stdout().flush()?;
            }

            Ok(is_kitty && self.config.kitty_features.transmit_images)
        } else {
            log::warn!("Could not determine terminal type");
            Ok(false)
        }
    }

    /// Setup terminal for video display
    async fn setup_terminal(&self) -> Result<()> {
        // Clear screen
        print!("\x1b[2J\x1b[H");

        // Hide cursor
        print!("\x1b[?25l");

        // Enter alternate screen buffer
        print!("\x1b[?1049h");

        io::stdout().flush()?;

        log::info!("Terminal setup complete for Kitty protocol");
        Ok(())
    }

    /// Display a single frame using Kitty protocol
    async fn display_frame(&mut self, frame: &ProcessedFrame) -> Result<()> {
        // Validate dimensions
        let (display_cols, display_rows) = self.calculate_display_size(frame.dimensions);

        if display_cols == 0 || display_rows == 0 {
            log::warn!(
                "Skipping frame with zero dimensions: {}x{}",
                display_cols,
                display_rows
            );
            return Ok(());
        }

        let (frame_width, frame_height) = frame.dimensions;
        if frame_width == 0 || frame_height == 0 {
            log::warn!(
                "Skipping frame with zero source dimensions: {}x{}",
                frame_width,
                frame_height
            );
            return Ok(());
        }

        // Create the Kitty graphics command
        let action = Action::TransmitAndDisplay(
            ActionTransmission {
                format: Format::Png,
                medium: Medium::File,
                width: frame_width,
                height: frame_height,
                ..Default::default()
            },
            ActionPut::default(),
        );

        // Get the image data
        let temp_file = format!("/tmp/kitty_frame_{}.png", frame.frame_number);
        frame.image.save(&temp_file)?; // Save as PNG

        // Create command with payload
        let command = Command::with_payload_from_path(action, temp_file.as_ref());

        // Wrap the command with proper escape sequences
        let wrapped_command = WrappedCommand::new(command);

        // Display the wrapped command
        print!("{wrapped_command}");
        io::stdout().flush()?;

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_file);

        log::debug!("Frame {} displayed via Kitty protocol", frame.frame_number);

        Ok(())
    }

    /// Calculate appropriate display size for terminal
    fn calculate_display_size(&self, frame_dimensions: (u32, u32)) -> (u16, u16) {
        let (term_cols, term_rows) = self.terminal_size;
        let (frame_width, frame_height) = frame_dimensions;

        // Calculate aspect ratio
        let frame_aspect = frame_width as f32 / frame_height as f32;
        let term_aspect = term_cols as f32 / term_rows as f32;

        // Fit frame to terminal while preserving aspect ratio
        let (display_cols, display_rows) = if frame_aspect > term_aspect {
            // Frame is wider, fit to terminal width
            let cols = term_cols;
            let rows = (term_cols as f32 / frame_aspect) as u16;

            (cols, rows.min(term_rows))
        } else {
            // Frame is taller, fit to terminal height
            let rows = term_rows;
            let cols = (term_rows as f32 * frame_aspect) as u16;

            (cols.min(term_cols), rows)
        };

        log::debug!(
            "Display size calculated: {}x{} -> {}x{}",
            frame_width,
            frame_height,
            display_cols,
            display_rows
        );

        (display_cols, display_rows)
    }

    // Update display stats
    async fn update_display_stats(
        &self,
        frames_displayed: u64,
        start_time: Instant,
        last_frame_time: Instant,
    ) {
        let mut stats = self.stats.write().await;

        stats.frames_displayed = frames_displayed;

        // Calculate display FPS
        let elapsed = start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            stats.display_fps = (frames_displayed as f64 / elapsed) as f32;
        }

        // Calculate avg display time
        stats.avg_display_time = last_frame_time.elapsed();

        // Log timing info periodically
        if frames_displayed % 300 == 0 {
            // Every 10 secs at 30fps
            let target_fps = 30.0;
            let fps_accuracy = (stats.display_fps / target_fps * 100.0).min(100.0);

            log::info!(
                "Display timing - Target: {target_fps:.1}fps, Actual: {:.1}fps ({fps_accuracy:.1}% accuracy)",
                stats.display_fps
            );
        }

        stats.terminal_refresh_rate = stats.display_fps;
    }

    /// Cleanup terminal state
    async fn cleanup_terminal(&self) -> Result<()> {
        // Clear all Kitty images properly
        let clear_action = Action::Delete(ActionDelete {
            hard: true,
            target: kitty_image::DeleteTarget::Placements,
        });
        let clear_command = Command::new(clear_action);
        let wrapped_clear = WrappedCommand::new(clear_command);

        print!("{wrapped_clear}");

        print!("\x1b[?1049l");
        print!("\x1b[?25h");
        print!("\x1b[2J\x1b[H");

        io::stdout().flush()?;

        Ok(())
    }

    /// Get display stats
    pub async fn get_stats(&self) -> DisplayStats {
        self.stats.read().await.clone()
    }

    /// Update terminal size (call when terminal is resized)
    pub fn update_terminal_size(&mut self, new_size: (u16, u16)) {
        self.terminal_size = new_size;
        log::info!("Terminal size updated to: {}x{}", new_size.0, new_size.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kitty_display_creation() {
        let config = DisplayConfig::default();
        let _display = KittyDisplay::new(config).unwrap();
    }

    #[test]
    fn test_display_size_calculation() {
        let config = DisplayConfig {
            terminal_size: Some((80, 24)),
            ..Default::default()
        };
        let display = KittyDisplay::new(config).unwrap();

        // Test 16:9 frame (720p)
        let (cols, rows) = display.calculate_display_size((1280, 720));

        assert!(cols <= 80);
        assert!(rows <= 24);

        // Test square frame
        let (cols, rows) = display.calculate_display_size((720, 720));

        assert!(cols <= 80);
        assert!(rows <= 24);
    }
}
