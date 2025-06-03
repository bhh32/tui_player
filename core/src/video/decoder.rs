use super::{MediaInfo, VideoFrame as Frame, init};
use anyhow::{Context, Result, anyhow};
use ffmpeg_next as ffmpeg;
use image::{DynamicImage, RgbaImage};
use std::path::Path;
use std::process::Command;
use tempfile::NamedTempFile;

pub struct VideoDecoder {
    format_context: ffmpeg::format::context::Input,
    video_stream_index: usize,
    codec_context: ffmpeg::codec::decoder::Video,
    scaler: ffmpeg::software::scaling::context::Context,
    frame_rate: f64,
    time_base: f64,
    next_pts: i64,
    eof: bool,
}

impl VideoDecoder {
    /// Create a new video decoder for the specified file path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        // Init ffmpeg
        init().context("Failed to initialize FFmpeg - check FFmpeg library installation")?;

        // Log the file path being opened
        let path_str = path.as_ref().to_string_lossy();
        log::info!("Opening video file: {}", path_str);

        // Verify file exists and is readable before attempting to open
        if !path.as_ref().exists() {
            return Err(anyhow!("Video file does not exist: {}", path_str));
        }
        
        if !path.as_ref().is_file() {
            return Err(anyhow!("Path exists but is not a file: {}", path_str));
        }

        // Open the file with better error context
        let format_context = match ffmpeg::format::input(&path) {
            Ok(ctx) => {
                log::debug!("Opened media file successfully, format: {}", ctx.format().name());
                ctx
            },
            Err(e) => {
                log::error!("Failed to open input file: {} - Error: {}", path_str, e);
                return Err(anyhow!("Failed to open '{}' as a media file. Error: {}", path_str, e));
            }
        };

        // List all streams for debugging
        log::debug!("Media contains {} streams:", format_context.streams().count());
        for (i, s) in format_context.streams().enumerate() {
            log::debug!("  Stream #{}: Type: {:?}, Codec: {}", 
                        i, 
                        s.parameters().medium(), 
                        s.parameters().id().name());
        }

        // Find the first video stream
        let (video_stream_index, stream) = format_context
            .streams()
            .enumerate()
            .find(|(_, s)| s.parameters().medium() == ffmpeg::media::Type::Video)
            .ok_or_else(|| {
                log::error!("No video stream found in: {}", path_str);
                anyhow!("No video stream found in the file: {}. The file may be an audio-only file or use an unsupported codec.", path_str)
            })?;

        log::info!("Found video stream at index {}, codec: {}", 
                  video_stream_index, 
                  stream.parameters().id().name());

        // Get codec parameters and stream information with better error handling
        let context_decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .with_context(|| {
                log::error!("Failed to create decoder context - codec may be unsupported");
                format!("Failed to create decoder context for codec: {}. This codec may not be supported by your FFmpeg build.", 
                       stream.parameters().id().name())
            })?;

        let decoder = context_decoder
            .decoder()
            .video()
            .with_context(|| {
                log::error!("Failed to create video decoder for codec: {}", stream.parameters().id().name());
                format!("Failed to create video decoder for codec: {}. This may be due to a missing codec library.", 
                       stream.parameters().id().name())
            })?;

        // Validate video dimensions
        if decoder.width() == 0 || decoder.height() == 0 {
            log::error!("Invalid video dimensions: {}x{}", decoder.width(), decoder.height());
            return Err(anyhow!("Invalid video dimensions: {}x{}", decoder.width(), decoder.height()));
        }

        // Calculate frame rate
        let frame_rate = f64::from(stream.rate().0) / f64::from(stream.rate().1);
        let time_base = f64::from(stream.time_base().0) / f64::from(stream.time_base().1);
        
        // Validate frame rate
        let frame_rate = if frame_rate.is_nan() || frame_rate <= 0.0 || frame_rate > 1000.0 {
            log::warn!("Unusual frame rate detected: {:.2} fps, using default 30fps", frame_rate);
            // Default to a reasonable frame rate if the detected one is invalid
            30.0
        } else {
            frame_rate
        };
        
        log::debug!(
        "Video details - Frame rate: {:.2} fps, time base: {:.6}, dimensions: {}x{}, format: {:?}",
        frame_rate,
        time_base,
        decoder.width(),
        decoder.height(),
        decoder.format()
    );

        // Create a scaler to convert to RGB
        let scaler = match ffmpeg::software::scaling::context::Context::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            ffmpeg::format::Pixel::RGBA,
            decoder.width(),
            decoder.height(),
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        ) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to create video scaler: {}", e);
                return Err(anyhow!("Failed to create video scaler for pixel format '{:?}': {}",
                                  decoder.format(), e));
            }
        };

        log::info!(
            "Successfully initialized video decoder for {}: {}x{} @ {:.2}fps",
            path_str,
            decoder.width(),
            decoder.height(),
            frame_rate
        );

        Ok(Self {
            format_context,
            video_stream_index,
            codec_context: decoder,
            scaler,
            frame_rate,
            time_base,
            next_pts: 0,
            eof: false,
        })
    }

    /// Get information about the media file
    pub fn get_media_info(&self) -> MediaInfo {
        // Safely get stream info, falling back to sensible defaults if needed
        let _ = match self.format_context.stream(self.video_stream_index) {
            Some(s) => s,
            None => {
                log::warn!("Failed to get stream info: stream not found");
                // Return the best info we can without the stream
                return MediaInfo {
                    duration: self.format_context.duration() as f64
                        / ffmpeg::ffi::AV_TIME_BASE as f64,
                    width: self.codec_context.width(),
                    height: self.codec_context.height(),
                    frame_rate: self.frame_rate,
                    format_name: self.format_context.format().name().to_string(),
                    video_codec: self.codec_context.id().name().to_string(),
                    audio_codec: None,
                };
            }
        };

        // Get audio codec info if available
        let audio_codec = self
            .format_context
            .streams()
            .find(|s| s.parameters().medium() == ffmpeg::media::Type::Audio)
            .map(|s| s.parameters().id().name().to_string());

        // Calculate accurate duration
        let duration = self.format_context.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64;

        log::debug!(
            "Media info: {}x{} @ {:.2}fps, duration: {:.2}s, codec: {}, audio: {}",
            self.codec_context.width(),
            self.codec_context.height(),
            self.frame_rate,
            duration,
            self.codec_context.id().name(),
            audio_codec.as_deref().unwrap_or("none")
        );

        MediaInfo {
            duration,
            width: self.codec_context.width(),
            height: self.codec_context.height(),
            frame_rate: self.frame_rate,
            format_name: self.format_context.format().name().to_string(),
            video_codec: self.codec_context.id().name().to_string(),
            audio_codec,
        }
    }

    /// Decode the next frame from the video
    pub fn decode_next_frame(&mut self) -> Result<Option<Frame>> {
        if self.eof {
            return Ok(None);
        }

        let mut decoded_frame = None;
        let start_time = std::time::Instant::now();

        // Read packets until a video frame is gotten or EOF is reached
        while decoded_frame.is_none() {
            // Check for timeout to avoid hangs with detailed diagnostics
            if start_time.elapsed() > std::time::Duration::from_secs(5) {
                log::error!("Decoder timeout detected at position {}s - possible corruption", 
                    self.next_pts as f64 * self.time_base);
                return Err(anyhow!(
                    "Timeout while decoding frame at position {}s - possible corrupted video segment or codec deadlock",
                    self.next_pts as f64 * self.time_base
                ));
            }

            match self.format_context.packets().next() {
                Some((stream, packet)) if stream.index() == self.video_stream_index => {
                    // Send packet with improved error context
                    if let Err(e) = self.codec_context.send_packet(&packet) {
                        let position = packet.pts().map_or_else(
                            || self.next_pts as f64 * self.time_base,
                            |pts| pts as f64 * self.time_base
                        );
                        log::warn!("Error sending packet to decoder at position {:.2}s: {}", position, e);
                        // Try to continue by flushing and skipping this packet
                        self.codec_context.flush();
                        continue;
                    }

                    let mut frame = ffmpeg::util::frame::video::Video::empty();
                    match self.codec_context.receive_frame(&mut frame) {
                        Ok(_) => {
                            let pts = frame.pts().unwrap_or(self.next_pts);
                            self.next_pts = pts + 1;

                            // Calculate timestamp and duration
                            let timestamp = pts as f64 * self.time_base;
                            let duration = 1.0 / self.frame_rate;

                            // Convert to RGB using the scaler with error context
                            let mut rgb_frame = ffmpeg::util::frame::video::Video::empty();
                            if let Err(e) = self.scaler.run(&frame, &mut rgb_frame) {
                                log::warn!("Error scaling frame: {}", e);
                                return Err(anyhow!("Failed to scale video frame: {}", e));
                            }

                            // Convert to image::DynamicImage
                            let width = rgb_frame.width();
                            let height = rgb_frame.height();
                            let data = rgb_frame.data(0).to_vec();

                            // Validate frame data and dimensions before creating image
                            if width == 0 || height == 0 {
                                let error_msg = format!("Invalid frame dimensions: {}x{}", width, height);
                                log::error!("{}", error_msg);
                                return Err(anyhow!(error_msg));
                            }
                            
                            let expected_size = width as usize * height as usize * 4;
                            if data.len() < expected_size {
                                let error_msg = format!(
                                    "Corrupted frame data: size {} bytes (expected at least {} bytes for {}x{})",
                                    data.len(),
                                    expected_size,
                                    width,
                                    height
                                );
                                log::error!("{}", error_msg);
                                // Skip this frame and try the next one
                                self.next_pts += 1;
                                continue;
                            }
                            
                            // Create image with better error handling
                            let image = match RgbaImage::from_raw(width, height, data.clone()) {
                                Some(img) => img,
                                None => {
                                    let error_msg = format!(
                                        "Failed to create image: {}x{} with {} bytes",
                                        width,
                                        height,
                                        data.len()
                                    );
                                    log::error!("{}", error_msg);
                                    return Err(anyhow!(error_msg));
                                }
                            };

                            log::trace!("Decoded frame at timestamp {:.2}s", timestamp);
                            decoded_frame = Some(Frame::new(
                                DynamicImage::ImageRgba8(image),
                                timestamp,
                                duration,
                            ));
                        }
                        Err(ffmpeg::Error::Other {
                            errno: ffmpeg::error::EAGAIN,
                        }) => continue,
                        Err(e) => {
                            // For corrupted frame errors, try to recover rather than fail completely
                            let position = self.next_pts as f64 * self.time_base;
                            log::warn!("Error receiving frame at position {:.2}s: {}", position, e);
                            
                            if e.to_string().contains("corrupted") {
                                log::warn!("Detected corrupted frame, attempting to skip and continue");
                                // Skip this frame and try the next one
                                self.next_pts += 1;
                                continue;
                            }
                            
                            return Err(anyhow!("Failed to receive frame from decoder at position {:.2}s: {}", 
                                position, e));
                        }
                    }
                }
                Some(_) => continue,
                None => {
                    // EOF reached
                    self.eof = true;
                    log::debug!("End of video file reached");
                    break;
                }
            }
        }

        // Log frame decode time if it's unusually slow
        let decode_time = start_time.elapsed();
        if decode_time > std::time::Duration::from_millis(50) {
            log::warn!("Slow frame decode: {}ms", decode_time.as_millis());
        }

        Ok(decoded_frame)
    }

    /// Seek to a specific timestamp in seconds
    pub fn seek(&mut self, timestamp_secs: f64) -> Result<()> {
        log::debug!("Seeking to position {:.2}s", timestamp_secs);

        // Validate timestamp is within bounds
        let duration = self.format_context.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64;
        let timestamp_secs = timestamp_secs.max(0.0).min(duration);

        // Convert to FFmpeg's internal timestamp format
        let timestamp = (timestamp_secs * ffmpeg::ffi::AV_TIME_BASE as f64) as i64;

        // Maximum seek attempts for corrupted videos
        let max_seek_attempts = 3;
        let mut attempt = 0;
        let mut last_error = None;
        
        // Try seeking multiple times with different flags if needed
        while attempt < max_seek_attempts {
            let flags = match attempt {
                0 => 0,                               // Default seek
                1 => ffmpeg::ffi::AVSEEK_FLAG_ANY,    // Seek to any frame, not just keyframes
                _ => ffmpeg::ffi::AVSEEK_FLAG_BACKWARD // Seek to keyframe before timestamp
            };
            
            match self.format_context.seek(timestamp, (flags as i64)..(flags as i64)) {
                Ok(_) => {
                    log::debug!("Seek successful to {:.2}s on attempt {}", timestamp_secs, attempt + 1);
                    
                    // Flush decoder buffers
                    self.codec_context.flush();
                    self.eof = false;
                    
                    // Calculate new PTS value in stream timebase
                    self.next_pts = (timestamp_secs / self.time_base) as i64;
                    
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("Seek attempt {} failed to {:.2}s: {}", attempt + 1, timestamp_secs, e);
                    last_error = Some(e);
                    attempt += 1;
                }
            }
        }

        // All seek attempts failed
        if let Some(e) = last_error {
            log::error!("All seek attempts to {:.2}s failed: {}", timestamp_secs, e);
            return Err(anyhow!("Failed to seek to {:.2}s after multiple attempts: {}", timestamp_secs, e));
        } else {
            return Err(anyhow!("Failed to seek to {:.2}s for unknown reason", timestamp_secs));
        }
    }

    /// Decode the current frame without advancing (for paused playback)
    pub fn decode_current_frame(&mut self) -> Result<Option<Frame>> {
        // If we've reached EOF, just return None
        if self.eof {
            return Ok(None);
        }
        
        // Store current position
        let current_pts = self.next_pts;
        
        // Decode a frame (this will advance the position)
        let frame = self.decode_next_frame()?;
        
        // If we got a frame, seek back to maintain position
        if frame.is_some() {
            let timestamp = (current_pts as f64) * self.time_base;
            let _ = self.seek(timestamp);
        }
        
        Ok(frame)
    }

    /// Extract the audio stream to a temporary WAV file and return its path
    pub fn extract_audio_to_tempfile<P: AsRef<Path>>(input_path: P) -> Result<std::path::PathBuf> {
        // Create a temp file for the audio
        let temp_file = NamedTempFile::new()?.into_temp_path();
        let temp_path = temp_file.to_path_buf();
        // Use ffmpeg CLI to extract audio as WAV (universal, no codec issues)
        let status = Command::new("ffmpeg")
            .args(&["-y", "-i", input_path.as_ref().to_str().unwrap(), "-vn", "-acodec", "pcm_s16le", temp_path.to_str().unwrap()])
            .status()
            .context("Failed to run ffmpeg to extract audio")?;
        if !status.success() {
            return Err(anyhow!("ffmpeg failed to extract audio"));
        }
        Ok(temp_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // Helper function to create a test video file
    fn create_test_video() -> Result<tempfile::TempDir> {
        // Simulating the test for now
        Ok(tempdir()?)
    }

    #[test]
    #[ignore]
    fn test_decoder_initialization() {
        let temp_dir = create_test_video().unwrap();
        let video_path = temp_dir.path().join("test.mp4");

        // Assuming test.mp4 file in test resources
        // When rewriting working test, copy to the temp directory

        let decoder = VideoDecoder::new(&video_path);
        assert!(decoder.is_ok());
    }
}
