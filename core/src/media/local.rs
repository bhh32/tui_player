use crate::render::{RenderConfig, TerminalRenderer};
use crate::{FrameBuffer, MediaInfo, MediaPlayer, VideoDecoder};
use anyhow::Result;
use log::{debug, warn};
use std::any::Any;
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
    audio_handle: Option<std::thread::JoinHandle<()>>,
    audio_start: Option<Instant>,
    frame_buffer: FrameBuffer,
    prefetching_active: bool,
    volume: i32, // Volume level (0-100)
    muted: bool, // Whether audio is muted
    audio_control_tx: Option<std::sync::mpsc::Sender<AudioControl>>,
}

/// Audio control commands sent to the audio thread
enum AudioControl {
    SetVolume(i32),
    ToggleMute(bool),
    Stop,
}

impl LocalMediaPlayer {
    /// Create a new local media player
    pub fn new<P: AsRef<Path>>(path: P, render_config: Option<RenderConfig>) -> Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        let decoder = VideoDecoder::new(&path_buf)?;
        let info = decoder.get_media_info();
        let frame_duration = Duration::from_secs_f64(1. / info.frame_rate);
        let render_config = render_config.unwrap_or_default();
        let renderer = TerminalRenderer::new(render_config)?;

        // Initialize frame buffer - size based on frame rate (about 5 seconds of video)
        let buffer_capacity = (5.0 * info.frame_rate).ceil() as usize;
        debug!(
            "Creating frame buffer with capacity for {} frames",
            buffer_capacity
        );
        let frame_buffer = FrameBuffer::new(buffer_capacity);

        // Check if media has audio stream before attempting extraction
        let has_audio = decoder.get_media_info().audio_codec.is_some();

        // Extract audio to temp file and prepare for playback if the video has audio
        let (audio_path, audio_start) = if has_audio {
            debug!("Media has audio stream, attempting extraction");
            match VideoDecoder::extract_audio_to_tempfile(&path_buf) {
                Ok(audio_path) => {
                    debug!("Audio extracted successfully to temporary file");
                    (Some(audio_path.clone()), Some(Instant::now()))
                }
                Err(e) => {
                    warn!("Failed to extract audio: {}", e);
                    (None, None)
                }
            }
        } else {
            debug!("Media has no audio stream, skipping audio extraction");
            (None, None)
        };

        // Create audio control channel
        let (audio_tx, audio_rx) = std::sync::mpsc::channel();

        // Modify audio thread to handle volume and mute commands
        let audio_handle = if let Some(audio_path_value) = audio_path {
            let audio_tx_clone = audio_tx.clone();
            let audio_path_clone = audio_path_value.clone();
            Some(std::thread::spawn(move || {
                use rodio::{Decoder as RodioDecoder, OutputStream, Sink};
                use std::fs::File;
                use std::io::BufReader;

                if let Ok((_stream, stream_handle)) = OutputStream::try_default() {
                    let mut sink_option: Option<Sink> = None;

                    if let Ok(file) = File::open(&audio_path_clone) {
                        if let Ok(source) = RodioDecoder::new(BufReader::new(file)) {
                            if let Ok(sink) = Sink::try_new(&stream_handle) {
                                sink.append(source);
                                sink.set_volume(0.5); // Initial 50% volume
                                sink_option = Some(sink);
                            }
                        }
                    }

                    // Process audio control messages
                    if let Some(sink) = sink_option {
                        let mut running = true;
                        while running {
                            if let Ok(cmd) = audio_rx.try_recv() {
                                match cmd {
                                    AudioControl::SetVolume(vol) => {
                                        let normalized_vol = vol as f32 / 100.0;
                                        sink.set_volume(normalized_vol);
                                        debug!("Audio volume set to {}", vol);
                                    }
                                    AudioControl::ToggleMute(muted) => {
                                        if muted {
                                            sink.set_volume(0.0);
                                            debug!("Audio muted");
                                        } else {
                                            let normalized_vol = 0.5; // Default to 50% when unmuting
                                            sink.set_volume(normalized_vol);
                                            debug!("Audio unmuted");
                                        }
                                    }
                                    AudioControl::Stop => {
                                        running = false;
                                        debug!("Audio playback stopping");
                                    }
                                }
                            }

                            // Check if playback has finished
                            if sink.empty() {
                                debug!("Audio playback completed");
                                break;
                            }

                            // Sleep to avoid busy waiting
                            std::thread::sleep(Duration::from_millis(100));
                        }

                        // Clean up
                        sink.stop();
                    }
                }

                // Signal that we're done
                let _ = audio_tx_clone.send(AudioControl::Stop);
            }))
        } else {
            None
        };

        Ok(Self {
            decoder,
            renderer,
            _path: path_buf,
            current_timestamp: 0.,
            last_frame_time: Instant::now(),
            frame_duration,
            paused: false,
            audio_handle,
            audio_start,
            frame_buffer,
            prefetching_active: false,
            volume: 50,   // Default to 50% volume
            muted: false, // Start unmuted
            audio_control_tx: Some(audio_tx),
        })
    }

    /// Get information about the current media
    pub fn get_media_info(&self) -> MediaInfo {
        self.decoder.get_media_info()
    }

    /// Get the current buffer status
    pub fn get_buffer_status(&self) -> Option<(usize, usize, f64)> {
        Some(self.frame_buffer.status())
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
        // Clean up audio resources if needed
        if self.audio_handle.is_some() {
            debug!("Stopping audio playback");
            // Send stop command to audio thread
            if let Some(tx) = &self.audio_control_tx {
                let _ = tx.send(AudioControl::Stop);
                debug!("Sent stop command to audio thread");
            }
        }

        // Clear the frame buffer
        self.frame_buffer.clear();

        debug!("Player stopped and resources released");
        Ok(())
    }

    /// Get current volume (0-100)
    fn get_volume(&self) -> i32 {
        self.volume
    }

    /// Check if audio is muted
    fn is_muted(&self) -> bool {
        self.muted
    }

    /// Set volume (0-100)
    fn set_volume(&mut self, volume: i32) -> anyhow::Result<()> {
        // Ensure volume is in valid range
        let clamped_volume = volume.clamp(0, 100);
        self.volume = clamped_volume;

        // Update audio if not muted
        if !self.muted {
            if let Some(tx) = &self.audio_control_tx {
                let _ = tx.send(AudioControl::SetVolume(clamped_volume));
                debug!("Volume set to {}", clamped_volume);
            }
        }

        Ok(())
    }

    /// Toggle mute state
    fn toggle_mute(&mut self) -> anyhow::Result<()> {
        self.muted = !self.muted;

        // Update audio
        if let Some(tx) = &self.audio_control_tx {
            let _ = tx.send(AudioControl::ToggleMute(self.muted));
            debug!("Mute toggled to {}", self.muted);
        }

        Ok(())
    }

    fn update(&mut self) -> Result<()> {
        // CRITICAL FIX: If paused, render the current frame with special marker
        if self.paused {
            debug!("Player is paused - rendering current frame");

            // Make sure prefetching is disabled when paused
            if self.prefetching_active {
                self.prefetching_active = false;
                debug!("Disabled prefetching in paused state");
            }

            // First try to get frame from buffer
            let frame_result = if let Some(mut frame) = self.frame_buffer.current_frame() {
                debug!("Using frame from buffer for paused display");
                // Mark this as a paused frame with negative timestamp
                frame.timestamp = -1.0;
                self.renderer.render(&frame)?;
                debug!(
                    "Rendered paused frame from buffer at position: {:.2}s",
                    self.current_timestamp
                );
                Ok(Some(frame))
            } else {
                // Fall back to decoder if not in buffer
                self.decoder.decode_current_frame()
            };

            // Process the frame result
            match frame_result {
                Ok(Some(mut frame)) => {
                    if frame.timestamp >= 0.0 {
                        // Only process if not already processed from buffer
                        // Mark this as a paused frame with negative timestamp
                        frame.timestamp = -1.0;
                        self.renderer.render(&frame)?;
                        debug!(
                            "Rendered paused frame at position: {:.2}s",
                            self.current_timestamp
                        );

                        // Add to buffer for future use if not already there
                        let _ = self.frame_buffer.add_frame(frame.clone());
                    }
                }
                Ok(None) => {
                    debug!("No frame available at paused position");
                }
                Err(e) => {
                    warn!("Error getting paused frame: {}", e);
                }
            }

            // Sleep a bit to reduce CPU usage when paused
            std::thread::sleep(Duration::from_millis(100));
            return Ok(());
        }

        // Use audio clock for sync if available
        let elapsed = if let Some(start) = self.audio_start {
            start.elapsed()
        } else {
            self.last_frame_time.elapsed()
        };

        // Check if it's time to render the next frame
        if elapsed >= self.frame_duration {
            if self.audio_start.is_none() {
                self.last_frame_time = Instant::now();
            }

            // Try to get frame from buffer first
            let frame = if let Some(frame) = self.frame_buffer.next_frame() {
                debug!("Using frame from buffer at {:.2}s", frame.timestamp);
                Some(frame)
            } else {
                // If not in buffer, decode it directly
                match self.decoder.decode_next_frame()? {
                    Some(frame) => {
                        // Add to buffer for future use
                        let _ = self.frame_buffer.add_frame(frame.clone());
                        Some(frame)
                    }
                    None => None,
                }
            };

            // Render the frame if we got one
            if let Some(frame) = frame {
                self.current_timestamp = frame.timestamp;
                self.renderer.render(&frame)?;
                debug!(
                    "Rendered frame at timestamp: {:.2}s",
                    self.current_timestamp
                );

                // Prefetch next few frames if we're not at capacity yet
                let (frames_in_buffer, capacity, _) = self.frame_buffer.status();
                if frames_in_buffer < capacity / 2 {
                    // Try to decode one more frame ahead
                    if let Ok(Some(next_frame)) = self.decoder.decode_next_frame() {
                        debug!("Prefetched frame at {:.2}s", next_frame.timestamp);
                        let _ = self.frame_buffer.add_frame(next_frame);
                        // Set prefetching flag to true if not already set
                        if !self.prefetching_active {
                            self.prefetching_active = true;
                            debug!("Prefetching activated");
                        }
                    }
                } else if self.prefetching_active && frames_in_buffer >= capacity / 2 {
                    // If buffer is filling up, we can stop aggressive prefetching
                    self.prefetching_active = false;
                    debug!("Prefetching deactivated - buffer sufficiently filled");
                }
            } else {
                // EOF reached
                debug!("End of video reached");
            }
        }

        Ok(())
    }

    fn toggle_pause(&mut self) {
        // CRUCIAL FIX: Toggle pause state and log it prominently
        self.paused = !self.paused;
        log::warn!(
            "LOCAL PLAYER: Playback paused state changed to: {}",
            self.paused
        );
        self.last_frame_time = Instant::now(); // Reset frame timing

        if self.paused {
            // Capture current frame to buffer if not already there
            if self.frame_buffer.current_frame().is_none() {
                if let Ok(Some(frame)) = self.decoder.decode_current_frame() {
                    debug!("Adding current frame to buffer for pause state");
                    let _ = self.frame_buffer.add_frame(frame);
                }
            }
            // Stop prefetching when paused
            if self.prefetching_active {
                self.prefetching_active = false;
                debug!("Stopped prefetching due to pause state");
            }
        } else {
            // When unpausing, ensure buffer is up-to-date for the current position
            self.frame_buffer.seek(self.current_timestamp);
            // Consider starting prefetch when unpausing
            if !self.prefetching_active {
                self.prefetching_active = true;
                debug!("Starting prefetching after unpause");
            }
        }
    }

    fn seek(&mut self, timestamp_secs: f64) -> Result<()> {
        // CRUCIAL FIX: Log seek action prominently
        log::warn!("LOCAL PLAYER: Seeking to {:.2}s", timestamp_secs);

        // Clear the frame buffer as its contents are now invalid
        self.frame_buffer.clear();

        // Reset prefetching state
        self.prefetching_active = false;

        // Ensure seek operation completes before returning
        let seek_result = self.decoder.seek(timestamp_secs);
        if let Err(ref e) = seek_result {
            log::warn!("LOCAL PLAYER: Seek failed: {}", e);
            return seek_result;
        }

        // Update state
        self.current_timestamp = timestamp_secs;
        self.last_frame_time = Instant::now(); // Reset frame timing

        // Update the buffer position
        self.frame_buffer.seek(timestamp_secs);

        // Force immediate frame update after seek
        log::warn!("LOCAL PLAYER: Decoding frame at new position");
        match self.decoder.decode_next_frame() {
            Ok(Some(frame)) => {
                log::warn!("LOCAL PLAYER: Rendering frame at new position");
                // Add the frame to buffer
                let _ = self.frame_buffer.add_frame(frame.clone());
                // Important: Set timestamp from the decoded frame
                self.current_timestamp = frame.timestamp;
                self.renderer.render(&frame)?;

                // Prefetch a few frames for smoother playback after seek
                for _ in 0..3 {
                    if let Ok(Some(next_frame)) = self.decoder.decode_next_frame() {
                        debug!("Prefetched post-seek frame at {:.2}s", next_frame.timestamp);
                        let _ = self.frame_buffer.add_frame(next_frame);
                        // Enable prefetching after seek
                        self.prefetching_active = true;
                    } else {
                        break; // Stop if we can't decode more frames
                    }
                }
            }
            Ok(None) => {
                log::warn!("LOCAL PLAYER: No frame at seek position (EOF)");
            }
            Err(e) => {
                log::warn!("LOCAL PLAYER: Error decoding frame after seek: {}", e);
                return Err(e);
            }
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
