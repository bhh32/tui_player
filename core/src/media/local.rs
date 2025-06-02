use crate::render::{RenderConfig, TerminalRenderer};
use crate::{MediaInfo, MediaPlayer, VideoDecoder};
use anyhow::Result;
use log::debug;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A player for local media files
pub struct LocalMediaPlayer {
    decoder: VideoDecoder,
    renderer: TerminalRenderer,
    _path: PathBuf,
    current_timestamp: f64,
    last_frame_time: Instant,
    frame_duration: Duration,
    paused: bool,
}

impl LocalMediaPlayer {
    /// Create a new local media player
    pub fn new<P: AsRef<Path>>(path: P, render_config: Option<RenderConfig>) -> Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        let decoder = VideoDecoder::new(&path_buf)?;

        // Get media info to determine frame rate
        let info = decoder.get_media_info();
        let frame_duration = Duration::from_secs_f64(1. / info.frame_rate);

        // Create renderer with provided config or default
        let render_config = render_config.unwrap_or_default();
        let renderer = TerminalRenderer::new(render_config)?;

        Ok(Self {
            decoder,
            renderer,
            _path: path_buf,
            current_timestamp: 0.,
            last_frame_time: Instant::now(),
            frame_duration,
            paused: false,
        })
    }

    /// Get information about the current media
    pub fn get_media_info(&self) -> MediaInfo {
        self.decoder.get_media_info()
    }
}

impl MediaPlayer for LocalMediaPlayer {
    /// Get information about the current media
    fn get_media_info(&self) -> Option<crate::MediaInfo> {
        Some(self.decoder.get_media_info())
    }

    /// Get current playback position in seconds
    fn get_position(&self) -> f64 {
        self.current_timestamp
    }

    /// Check if playback is paused
    fn is_paused(&self) -> bool {
        self.paused
    }

    /// Stop playback and release resources
    fn stop(&mut self) -> Result<()> {
        // No specific resources to release for local player
        Ok(())
    }

    fn update(&mut self) -> Result<()> {
        if self.paused {
            return Ok(());
        }

        let elapsed = self.last_frame_time.elapsed();

        // Check if it's time to render the next frame
        if elapsed >= self.frame_duration {
            self.last_frame_time = Instant::now();

            // Decode and render the next frame
            if let Some(frame) = self.decoder.decode_next_frame()? {
                self.current_timestamp = frame.timestamp;
                self.renderer.render(&frame)?;
                debug!(
                    "Rendered frame at timestamp: {:.2}s",
                    self.current_timestamp
                );
            } else {
                // EOF reached
                debug!("End of video reached");
            }
        }

        Ok(())
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        self.last_frame_time = Instant::now(); // Reset frame timing
    }

    fn seek(&mut self, timestamp_secs: f64) -> Result<()> {
        self.decoder.seek(timestamp_secs)?;
        self.current_timestamp = timestamp_secs;
        self.last_frame_time = Instant::now(); // Reset frame timing

        // Immediately decode and display a frame at the new position
        if let Some(frame) = self.decoder.decode_next_frame()? {
            self.renderer.render(&frame)?;
        }

        Ok(())
    }
}
