//! Kitty protocol display handler

use crate::renderer::types::*;
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use crossterm::terminal;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

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

        let display_start = Instant::now();
        let mut frames_displayed = 0u64;
        let mut last_frame_time = Instant::now();

        loop {
            // Check for commands (non-blocking)
            if let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    PlaybackCommand::Stop => {
                        log::info!("Kitty display received stop command");
                        break;
                    }
                    _ => {
                        // Other commands will be handled by scheduler
                        log::debug!("Kitty display received command: {cmd:?}");
                    }
                }
            }

            // Try to receive frame (non-blocking)
            if let Ok(frame) = frame_rx.try_recv() {
                // Display frame
                match self.display_frame(&frame).await {
                    Ok(_) => {
                        frames_displayed += 1;

                        // Update stats
                        self.update_display_stats(frames_displayed, display_start, last_frame_time)
                            .await;

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
                    }
                    Err(e) => {
                        log::error!("Failed to display frame {}: {e}", frame.frame_number);
                        let _ = status_tx.send(RenderStatus::Error(format!("Display error: {e}")));
                    }
                }
            } else {
                // No frame available, sleep briefly
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }

        // Cleanup
        self.cleanup_terminal().await?;
        log::info!("Kitty display loop completed");

        Ok(())
    }

    /// Check if terminal supports Kitty graphics protocol
    async fn check_kitty_support(&self) -> Result<bool> {
        // Query terminal capabilities using Kitty protocol detection
        print!("\x1b_Gi=1,a=q;\x1b\\");
        io::stdout().flush()?;

        // For now, assume support if config enables it
        // TODO: Parse the terminal response
        Ok(self.config.kitty_features.transmit_images)
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
        Ok(())
    }

    /// Display a single frame using Kitty protocol
    async fn display_frame(&mut self, frame: &ProcessedFrame) -> Result<()> {
        // Get next image ID
        let image_id = {
            let mut counter = self.image_id_counter.write().await;
            let id = *counter;
            *counter += 1;
            id
        };

        // Calculate display dimensions based on terminal size
        let (display_cols, display_rows) = self.calculate_display_size(frame.dimensions);

        // Encode frame as base64 for transmission
        let encoded_data = self.encode_frame_for_kitty(frame).await?;

        // Clear previous image (if any)
        if image_id > 1 {
            self.clear_previous_image(image_id - 1).await?;
        }

        // Position cursor for image display
        print!("\x1b[1;1H"); // Move to top-left

        // Send Kitty graphics command
        let kitty_command = format!(
            "\x1b_Gf=100,a=T,i={},s={},v={},c={},r={};\x1b\\",
            image_id,
            frame.dimensions.0, // source width
            frame.dimensions.1, // source height
            display_cols,       // display columns
            display_rows,       // display rows
        );

        print!("{kitty_command}");

        // Send image data in chunks
        self.send_image_data(&encoded_data, image_id).await?;

        // Display the image
        let display_command = format!("\x1b_Ga=p,i={image_id};\x1b\\");
        print!("{display_command}");

        io::stdout().flush()?;

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

    /// Encode frame data for Kitty protocol transmission
    async fn encode_frame_for_kitty(&self, frame: &ProcessedFrame) -> Result<String> {
        // Convert RgbaImage to bytes
        let image_bytes = frame.image.as_raw();

        // Encode as base64
        let encoded = base64::encode(image_bytes);

        Ok(encoded)
    }

    /// Send image data to terminal in chunks
    async fn send_image_data(&self, encoded_data: &str, image_id: u32) -> Result<()> {
        const CHUNK_SIZE: usize = 4096; // Kitty protocol chunk size limit

        let chunks: Vec<&str> = encoded_data
            .as_bytes()
            .chunks(CHUNK_SIZE)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect();

        for (i, chunk) in chunks.iter().enumerate() {
            let is_last = i == chunks.len() - 1;
            let more_flag = if is_last { 0 } else { 1 };

            let chunk_command = format!("\x1b_Gm={more_flag},i={image_id};\x1b\\{chunk}");

            print!("{chunk_command}");
        }

        io::stdout().flush()?;
        Ok(())
    }

    /// Clear previous image from terminal
    async fn clear_previous_image(&self, image_id: u32) -> Result<()> {
        let clear_command = format!("\x1b_Ga=d,i={image_id};\x1b\\");

        print!("{clear_command}");
        io::stdout().flush()?;

        Ok(())
    }

    /// Update display statistics
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

        // Estimate terminal refresh rate (simplified)
        stats.terminal_refresh_rate = stats.display_fps;
    }

    /// Cleanup terminal state
    async fn cleanup_terminal(&self) -> Result<()> {
        // Clear all images
        print!("\x1b_Ga=d;\x1b\\");

        // Exit alternate screen buffer
        print!("\x1b[?10491");

        // Show cursor
        print!("\x1b[?25h");

        // Clear screen
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
        assert!(cols <= 24);
    }
}
