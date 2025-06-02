// Optimizations in this module:
// - GPU acceleration for resizing (optional)
// - Adaptive resolution: reduces quality in case of low FPS
// - Terminal-specific graphics protocols: Kitty, iTerm2, Sixel, or fallback to Unicode blocks
//
// SIMD/Parallelization/Color Cache/Column Skipping/Dirty Row Diffing optimizations:
// - simd_blend_alpha: SIMD-accelerated alpha blending for RGBA pixels
// - get_color_code: caches ANSI color codes for fg/bg pairs
// - render_blocks: skips fully transparent columns, parallelizes row rendering, minimizes ANSI output, and only redraws dirty rows
//
// See tests in tests.rs for coverage of these features.
mod gpu;
#[cfg(test)]
mod tests;

use log::{debug, error, info, trace, warn};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rayon::prelude::*;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use crossterm::terminal;

use crate::video::VideoFrame;

use crate::render::gpu::GpuProcessor;
use rodio::{Decoder as RodioDecoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;

static GPU_PROCESSOR: Lazy<Mutex<Option<GpuProcessor>>> = Lazy::new(|| {
    // Initialize with additional error handling
    let processor = pollster::block_on(async {
        match GpuProcessor::new().await {
            Ok(processor) => {
                info!("GPU processor initialized successfully");
                Some(processor)
            }
            Err(e) => {
                error!("Failed to initialize GPU processor: {}", e);
                None
            }
        }
    });
    Mutex::new(processor)
});

fn get_gpu_processor() -> &'static Mutex<Option<GpuProcessor>> {
    &GPU_PROCESSOR
}

/// Supported terminal graphics protocols
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMethod {
    /// Kitty terminal graphics protocol
    Kitty,
    /// iTerm2 terminal graphics protocol
    ITerm,
    /// Sixel graphics protocol
    Sixel,
    /// Fallback using Unicode half blocks
    Blocks,
    /// Auto-detect the best available method
    Auto,
}

/// Configuration for terminal rendering
#[derive(Clone)]
pub struct RenderConfig {
    /// Method to use for rendering
    pub method: RenderMethod,
    /// Target width in terminal cells (None = auto)
    pub width: Option<u32>,
    /// Target height in terminal cells (None = auto)
    pub height: Option<u32>,
    /// Maintain aspect ratio when scaling
    pub maintain_aspect: bool,
    /// X offset in terminal cells
    pub x: u16,
    /// Y offset in terminal cells
    pub y: u16,
    /// Quality factor (0.1 to 1.0) - lower means faster but lower quality
    pub quality: f32,
    /// Enable adaptive resolution - reduces quality when frame rate drops
    pub adaptive_resolution: bool,
    /// Target FPS to maintain
    pub target_fps: f32,
    /// Enable multi-threaded processing when possible
    pub enable_threading: bool,
    /// Maximum frame size to process (to prevent memory issues)
    pub max_frame_dimension: Option<u32>,
    /// Enable GPU acceleration (disable for compatibility)
    pub enable_gpu: bool,
}

impl Default for RenderConfig {
    fn default() -> Self {
        // Detect if we're running in a CI/CD environment where GPU might not be available
        let is_ci = std::env::var("CI").unwrap_or_default() == "true"
            || std::env::var("GITHUB_ACTIONS").is_ok()
            || std::env::var("GITLAB_CI").is_ok();

        Self {
            method: RenderMethod::Auto,
            width: None,
            height: None,
            maintain_aspect: true,
            x: 0,
            y: 0,
            quality: 0.8,
            adaptive_resolution: true,
            target_fps: 30.0,
            enable_threading: true,
            max_frame_dimension: Some(1024),
            // Disable GPU by default in CI environments
            enable_gpu: !is_ci,
        }
    }
}

/// Renderer for displaying video frames in the terminal
pub struct TerminalRenderer {
    config: RenderConfig,
    effective_method: RenderMethod,
    term_width: u16,
    term_height: u16,
    // Performance tracking
    last_frame_time: std::time::Instant,
    frame_times: std::collections::VecDeque<f64>,
    current_quality_factor: f32,
    frames_since_quality_adjust: usize,
    // Kitty optimization
    last_kitty_temp_file: Option<std::path::PathBuf>,
    // Output buffer cache (to avoid allocations)
    #[allow(dead_code)]
    output_buffer: String,
    color_code_cache: Mutex<HashMap<([u16; 3], [u16; 3]), String>>,
    prev_frame_hash: Option<Vec<u64>>, // For dirty rectangle/frame diffing
}

impl TerminalRenderer {
    /// Create a new terminal renderer with the given configuration
    pub fn new(config: RenderConfig) -> Result<Self> {
        // Get terminal size
        let (term_width, term_height) = terminal::size().context("Failed to get terminal size")?;

        // Auto-detect the best rendering method if set to Auto
        let effective_method = if config.method == RenderMethod::Auto {
            Self::detect_best_method()
        } else {
            config.method
        };

        info!("Creating renderer with method: {:?}", effective_method);
        debug!("Terminal size: {}x{}", term_width, term_height);

        // Log GPU acceleration status
        if config.enable_gpu {
            // Pre-initialize GPU processor in background to speed up first frame
            if effective_method != RenderMethod::Blocks {
                std::thread::spawn(|| {
                    debug!("Pre-initializing GPU processor in background thread");
                    // Just accessing the static will trigger initialization
                    let processor = get_gpu_processor();
                    match processor.try_lock() {
                        Some(guard) => {
                            if guard.is_some() {
                                debug!("GPU processor initialization complete");
                            } else {
                                warn!("GPU processor initialization failed");
                            }
                        }
                        None => warn!("Could not acquire lock to check GPU processor"),
                    }
                });
            }
            log::info!("GPU acceleration enabled");
        } else {
            log::info!("GPU acceleration disabled");
        }

        // For Kitty and iTerm methods, verify terminal capabilities
        if (effective_method == RenderMethod::Kitty || effective_method == RenderMethod::ITerm)
            && effective_method != RenderMethod::Auto
        {
            // Check terminal environment variables
            let term = std::env::var("TERM").unwrap_or_default();
            if term.contains("kitty") || std::env::var("KITTY_WINDOW_ID").is_ok() {
                log::info!("Detected Kitty terminal");
            } else if term.contains("iterm") || std::env::var("ITERM_SESSION_ID").is_ok() {
                log::info!("Detected iTerm terminal");
            } else {
                log::warn!(
                    "Selected {:?} but terminal doesn't appear to support it",
                    effective_method
                );
                log::warn!("TERM={}, continuing anyway", term);
            }
        }

        // Initialize with current time
        let now = std::time::Instant::now();

        Ok(Self {
            config,
            effective_method,
            term_width,
            term_height,
            last_frame_time: now,
            frame_times: std::collections::VecDeque::with_capacity(30),
            current_quality_factor: 1.0,
            frames_since_quality_adjust: 0,
            last_kitty_temp_file: None,
            output_buffer: String::with_capacity(term_width as usize * term_height as usize * 25),
            color_code_cache: Mutex::new(HashMap::new()),
            prev_frame_hash: None,
        })
    }

    /// Detect the best available rendering method for the current terminal
    pub fn detect_best_method() -> RenderMethod {
        // Check for Kitty support, but use more cautious detection
        if std::env::var("KITTY_WINDOW_ID").is_ok() {
            log::info!("Detected Kitty terminal, will try Kitty protocol first");
            // Don't immediately return, just note it for now
            // We'll validate it actually works during rendering
            if Self::check_kitty_support() {
                return RenderMethod::Kitty;
            } else {
                log::warn!(
                    "Kitty terminal detected but graphics protocol not working, falling back to Blocks"
                );
                return RenderMethod::Blocks;
            }
        }

        // Check for iTerm support
        if std::env::var("ITERM_SESSION_ID").is_ok() {
            log::info!("Detected iTerm terminal");
            return RenderMethod::ITerm;
        }

        // Check for TERM types that support Sixel
        let term = std::env::var("TERM").unwrap_or_default();
        if term.contains("sixel") || term == "mlterm" || term == "yaft-256color" {
            log::info!("Detected Sixel-capable terminal: {}", term);
            return RenderMethod::Sixel;
        }

        // Try to detect Sixel support with a capability check
        if let Ok(true) = Self::check_sixel_support() {
            log::info!("Detected Sixel support via capability check");
            return RenderMethod::Sixel;
        }

        // Additional iTerm detection (iTerm might not set ITERM_SESSION_ID in all cases)
        if term.contains("iterm") {
            log::info!("Detected iTerm terminal via TERM: {}", term);
            return RenderMethod::ITerm;
        }

        // Fallback to block characters
        log::info!("No graphical protocol detected, using Unicode blocks");
        RenderMethod::Blocks
    }

    /// Check if the terminal supports Sixel by querying device attributes
    fn check_sixel_support() -> Result<bool> {
        use std::io::{Read, Write};
        use std::time::Duration;

        // This is a somewhat hacky way to check for Sixel support
        // We send a Device Attributes (DA) query and wait for a response
        // The response should contain "4" in the list of attributes if Sixel is supported

        // Save terminal settings
        let _term = terminal::enable_raw_mode()?;

        // Send Primary Device Attributes query
        print!("\x1B[c");
        std::io::stdout().flush()?;

        // Read response with timeout
        let mut buffer = [0u8; 64];
        let mut response = Vec::new();

        // Set stdin to non-blocking
        let stdin = std::io::stdin();
        let mut handle = stdin.lock();

        // Wait up to 100ms for a response
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_millis(100) {
            match handle.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => response.extend_from_slice(&buffer[0..n]),
                Err(_) => {
                    // Wait a bit and try again
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
            }

            // Check if we have a complete response
            let response_str = String::from_utf8_lossy(&response);
            if response_str.contains("\x1B[") && response_str.contains("c") {
                break;
            }
        }

        // Restore terminal settings
        terminal::disable_raw_mode()?;

        // Check response for Sixel support (attribute 4)
        let response_str = String::from_utf8_lossy(&response);
        Ok(response_str.contains("\x1B[?") && response_str.contains(";4;"))
    }

    /// Render a video frame to the terminal
    pub fn render(&mut self, frame: &VideoFrame) -> Result<()> {
        // Calculate FPS and adjust quality if needed
        self.update_performance_metrics();

        // Calculate target dimensions with quality adjustment
        let (width, height) = self.calculate_dimensions_with_quality(frame);

        // Skip resizing if dimensions already match to improve performance
        let resized_frame = if !frame.needs_resize(width, height) {
            log::trace!("Skipping resize - dimensions already match");
            frame.clone()
        } else if width < 32 || height < 32 {
            // For extremely small sizes, use fast nearest-neighbor resizing
            log::trace!("Using fast resize for small target dimensions");
            frame.fast_thumbnail(width.max(height))
        } else if self.effective_method != RenderMethod::Blocks && self.config.enable_gpu {
            // Try GPU-accelerated resizing for graphical protocols with fallback to CPU
            // Only if GPU acceleration is enabled in config
            let processor_mutex = get_gpu_processor();

            // Safely lock the processor mutex with timeout to prevent deadlocks
            let mut processor_guard = match processor_mutex.try_lock_for(Duration::from_millis(100))
            {
                Some(guard) => guard,
                None => {
                    warn!("GPU processor mutex lock timed out, falling back to CPU");
                    // Don't return, just fall back to CPU rendering
                    return self.render_blocks(&frame.resize(
                        width,
                        height,
                        self.config.maintain_aspect,
                    ));
                }
            };

            // Check if GPU processor is available
            if processor_guard.is_none() {
                debug!("GPU processor not available, using CPU resizing");
                // Fall back to CPU rendering
                return self.render_blocks(&frame.resize(
                    width,
                    height,
                    self.config.maintain_aspect,
                ));
            }

            let processor = processor_guard.as_mut().unwrap();
            trace!(
                "Using GPU acceleration for {}x{} -> {}x{}",
                frame.width, frame.height, width, height
            );

            // Attempt GPU processing with fallback to CPU on error, using a timeout
            let process_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Use a timeout to prevent hanging on GPU operations
                let start_time = std::time::Instant::now();
                let timeout = Duration::from_millis(1000);

                let result = processor.process_frame(frame, width, height);

                if start_time.elapsed() > timeout {
                    warn!(
                        "GPU processing took too long ({:?}), consider disabling GPU acceleration",
                        start_time.elapsed()
                    );
                }

                result
            }));

            match process_result {
                Ok(resized_data) => {
                    // GPU processing succeeded - resize_from_data returns a VideoFrame directly
                    frame.resize_from_data(width, height, resized_data)
                }
                Err(e) => {
                    // GPU processing failed, log error and fall back to CPU
                    error!("GPU processing failed: {:?}", e);
                    warn!("Falling back to CPU resizing");
                    frame.resize(width, height, self.config.maintain_aspect)
                }
            }
        } else {
            // Use CPU resizing for blocks rendering (less overhead for simple output)
            if self.effective_method == RenderMethod::Blocks {
                trace!("Using CPU resize for blocks rendering");
            } else {
                debug!("GPU acceleration disabled, using CPU resizing");
            }
            frame.resize(width, height, self.config.maintain_aspect)
        };

        // Position cursor at the specified location before rendering
        // This ensures the video is drawn at the correct position
        // Use buffered output to prevent flickering
        let mut stdout = std::io::stdout();
        let _ = write!(stdout, "\x1B[{};{}H", self.config.y + 1, self.config.x + 1);
        // Don't flush here - we'll flush after rendering

        // Track previous method for fallback mechanics
        // Replace static mut fallback state with safe statics
        static LAST_METHOD: Lazy<StdMutex<Option<RenderMethod>>> = Lazy::new(|| StdMutex::new(None));
        static KITTY_FAILED: Lazy<StdMutex<bool>> = Lazy::new(|| StdMutex::new(false));
        static ITERM_FAILED: Lazy<StdMutex<bool>> = Lazy::new(|| StdMutex::new(false));
        static SIXEL_FAILED: Lazy<StdMutex<bool>> = Lazy::new(|| StdMutex::new(false));

        // Determine which method to use, with better fallback handling
        let current_method = {
            // If any method has previously failed, avoid using it
            if *KITTY_FAILED.lock().unwrap() && self.effective_method == RenderMethod::Kitty {
                debug!("Kitty previously failed, using Blocks instead");
                RenderMethod::Blocks
            } else if *ITERM_FAILED.lock().unwrap() && self.effective_method == RenderMethod::ITerm {
                debug!("iTerm previously failed, using Blocks instead");
                RenderMethod::Blocks
            } else if *SIXEL_FAILED.lock().unwrap() && self.effective_method == RenderMethod::Sixel {
                debug!("Sixel previously failed, using Blocks instead");
                RenderMethod::Blocks
            } else if let Some(method) = *LAST_METHOD.lock().unwrap() {
                if method != RenderMethod::Auto {
                    method
                } else {
                    self.effective_method
                }
            } else {
                self.effective_method
            }
        };

        // Render based on the effective method with timeout protection
        let result = match current_method {
            RenderMethod::Kitty => {
                let kitty_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // Use a timeout for Kitty rendering
                    let kitty_timeout = std::time::Duration::from_millis(2000);
                    let start_time = std::time::Instant::now();

                    let render_result = self.render_kitty(&resized_frame);

                    if start_time.elapsed() > kitty_timeout {
                        warn!(
                            "Kitty rendering took too long ({:?}), may not be functioning properly",
                            start_time.elapsed()
                        );
                    }

                    render_result
                }));

                match kitty_result {
                    Ok(Ok(_)) => {
                        debug!("Kitty rendering succeeded");
                        Ok(())
                    }
                    Ok(Err(e)) => {
                        error!("Kitty rendering failed: {}", e);
                        warn!("Permanently falling back to blocks rendering");
                        *KITTY_FAILED.lock().unwrap() = true;
                        *LAST_METHOD.lock().unwrap() = Some(RenderMethod::Blocks);
                        let _ = write!(std::io::stdout(), "\x1B[2J\x1B[H");
                        std::io::stdout().flush().ok();
                        self.render_blocks(&resized_frame)
                    }
                    Err(e) => {
                        error!("Kitty rendering panicked: {:?}", e);
                        warn!("Permanently falling back to blocks rendering");
                        *KITTY_FAILED.lock().unwrap() = true;
                        *LAST_METHOD.lock().unwrap() = Some(RenderMethod::Blocks);
                        let _ = write!(std::io::stdout(), "\x1B[2J\x1B[H");
                        std::io::stdout().flush().ok();
                        self.render_blocks(&resized_frame)
                    }
                }
            }
            RenderMethod::ITerm => {
                match self.render_iterm(&resized_frame) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        error!("iTerm rendering failed: {}", e);
                        warn!("Permanently falling back to blocks rendering");
                        *ITERM_FAILED.lock().unwrap() = true;
                        *LAST_METHOD.lock().unwrap() = Some(RenderMethod::Blocks);
                        let _ = write!(std::io::stdout(), "\x1B[2J\x1B[H");
                        std::io::stdout().flush().ok();
                        self.render_blocks(&resized_frame)
                    }
                }
            }
            RenderMethod::Sixel => {
                match self.render_sixel(&resized_frame) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        error!("Sixel rendering failed: {}", e);
                        warn!("Permanently falling back to blocks rendering");
                        *SIXEL_FAILED.lock().unwrap() = true;
                        *LAST_METHOD.lock().unwrap() = Some(RenderMethod::Blocks);
                        let _ = write!(std::io::stdout(), "\x1B[2J\x1B[H");
                        std::io::stdout().flush().ok();
                        self.render_blocks(&resized_frame)
                    }
                }
            }
            RenderMethod::Blocks => self.render_blocks(&resized_frame),
            RenderMethod::Auto => {
                debug!("Using auto rendering method, defaulting to blocks");
                let result = self.render_blocks(&resized_frame);
                *LAST_METHOD.lock().unwrap() = Some(RenderMethod::Blocks);
                result
            }
        };

        // Store the successful method for future frames
        if result.is_ok() {
            *LAST_METHOD.lock().unwrap() = Some(current_method);
        }

        // Update frame time after rendering
        self.last_frame_time = std::time::Instant::now();

        // Play audio if present in the VideoFrame
        if let Some(ref audio_path) = frame.audio_path {
            self.play_audio(audio_path.clone());
        }

        result
    }

    /// Play audio from a file path (WAV/MP3/OGG/AAC/FLAC supported by rodio)
    pub fn play_audio<P: AsRef<std::path::Path> + Send + 'static>(&self, path: P) {
        std::thread::spawn(move || {
            if let Ok((_stream, stream_handle)) = OutputStream::try_default() {
                if let Ok(file) = File::open(path) {
                    let source = RodioDecoder::new(BufReader::new(file));
                    if let Ok(source) = source {
                        let sink = Sink::try_new(&stream_handle).unwrap();
                        sink.append(source);
                        sink.sleep_until_end();
                    }
                }
            }
        });
    }

    // Example usage: call this in your render logic if you have an audio file to play
    // self.play_audio("/path/to/audio/file.mp3");

    // Calculate dimensions based on config and terminal size
    fn calculate_dimensions(&self, frame: &VideoFrame) -> (u32, u32) {
        let width = self
            .config
            .width
            .unwrap_or((self.term_width as u32).saturating_sub(self.config.x as u32));
        let height = self
            .config
            .height
            .unwrap_or((self.term_height as u32).saturating_sub(self.config.y as u32));

        if self.config.maintain_aspect {
            let frame_aspect = frame.width as f32 / frame.height as f32;
            let target_aspect = width as f32 / height as f32;

            if target_aspect > frame_aspect {
                // Target is wider than source, constrain by height
                let new_width = (height as f32 * frame_aspect) as u32;
                return (new_width, height);
            } else {
                // Target is taller than source, constrain by width
                let new_height = (width as f32 / frame_aspect) as u32;
                return (width, new_height);
            }
        }

        (width, height)
    }

    // Calculate dimensions with quality factor applied
    fn calculate_dimensions_with_quality(&self, frame: &VideoFrame) -> (u32, u32) {
        let (base_width, base_height) = self.calculate_dimensions(frame);

        // Apply quality factor (adjust dimensions)
        let quality = self.config.quality * self.current_quality_factor;
        let quality = quality.max(0.2).min(1.0); // Ensure quality stays between 0.2 and 1.0

        // For blocks rendering, we can use even lower quality as it's less noticeable
        let effective_quality = if self.effective_method == RenderMethod::Blocks {
            quality
        } else {
            // For graphical protocols, ensure we don't go too low as it becomes very noticeable
            quality.max(0.4)
        };

        let scaled_width = ((base_width as f32) * effective_quality) as u32;
        let scaled_height = ((base_height as f32) * effective_quality) as u32;

        // Ensure dimensions are at least 32x32
        let min_dim = if self.effective_method == RenderMethod::Blocks {
            16
        } else {
            32
        };
        (scaled_width.max(min_dim), scaled_height.max(min_dim))
    }

    // Update performance metrics and adjust quality factor if needed
    fn update_performance_metrics(&mut self) {
        let now = std::time::Instant::now();
        let frame_time = now.duration_since(self.last_frame_time).as_secs_f64();

        // Only count if it's been a reasonable time (avoid extremely short intervals)
        if frame_time > 0.001 && frame_time < 1.0 {
            // Ignore suspiciously long frames (process suspension)
            // Add to rolling window of frame times
            self.frame_times.push_back(frame_time);
            if self.frame_times.len() > 15 {
                self.frame_times.pop_front();
            }
        }

        // Only adjust quality every 15 frames to avoid oscillation and allow stabilization
        self.frames_since_quality_adjust += 1;
        if self.config.adaptive_resolution && self.frames_since_quality_adjust >= 15 {
            self.frames_since_quality_adjust = 0;

            // Calculate current FPS based on recent frame times
            let avg_frame_time = if !self.frame_times.is_empty() {
                // Use median instead of mean for better stability
                let mut times = self.frame_times.iter().copied().collect::<Vec<_>>();
                times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let mid = times.len() / 2;
                if times.len() % 2 == 0 && !times.is_empty() {
                    (times[mid - 1] + times[mid]) / 2.0
                } else if !times.is_empty() {
                    times[mid]
                } else {
                    0.033 // Default to 30 FPS
                }
            } else {
                0.033 // Default to 30 FPS
            };

            let current_fps = if avg_frame_time > 0.0 {
                1.0 / avg_frame_time
            } else {
                30.0
            };
            let target_fps = self.config.target_fps as f64;

            // Log performance info
            log::debug!(
                "Current FPS: {:.1}, Target: {:.1}, Quality: {:.2}, Frame time: {:.1}ms, Method: {:?}",
                current_fps,
                target_fps,
                self.current_quality_factor,
                avg_frame_time * 1000.0,
                self.effective_method
            );

            // More gentle adjustments to prevent quality oscillation
            if current_fps < target_fps * 0.4 {
                // Way too slow, reduce quality significantly
                self.current_quality_factor = (self.current_quality_factor * 0.7).max(0.2);
                log::debug!(
                    "Performance very low, reducing quality to {:.2}",
                    self.current_quality_factor
                );
            } else if current_fps < target_fps * 0.7 {
                // We're running slow, reduce quality more gently
                self.current_quality_factor = (self.current_quality_factor * 0.85).max(0.2);
                log::debug!(
                    "Performance low, reducing quality to {:.2}",
                    self.current_quality_factor
                );
            } else if current_fps > target_fps * 1.7 && self.current_quality_factor < 1.0 {
                // We have lots of headroom, increase quality gradually
                self.current_quality_factor = (self.current_quality_factor * 1.2).min(1.0);
                log::debug!(
                    "Performance very good, increasing quality to {:.2}",
                    self.current_quality_factor
                );
            } else if current_fps > target_fps * 1.3 && self.current_quality_factor < 1.0 {
                // We have headroom, increase quality very gradually
                self.current_quality_factor = (self.current_quality_factor * 1.07).min(1.0);
                log::debug!(
                    "Performance good, increasing quality to {:.2}",
                    self.current_quality_factor
                );
            }
        }
    }

    // Implement specific rendering methods
    /// Check if terminal actually supports Kitty graphics protocol
    fn check_kitty_support() -> bool {
        // Check for KITTY_WINDOW_ID environment variable
        let has_env = std::env::var("KITTY_WINDOW_ID").is_ok();
        let term = std::env::var("TERM").unwrap_or_default();
        let term_matches = term.contains("kitty");

        if !has_env && !term_matches {
            return false;
        }

        // Try a simple test to see if we can display a small image
        // This will catch cases where the terminal reports as Kitty but doesn't
        // actually support the graphics protocol
        let test_result = std::panic::catch_unwind(|| -> bool {
            // Create a tiny 1x1 test image
            let mut test_image = image::RgbaImage::new(1, 1);
            test_image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
            let dynamic_img = image::DynamicImage::ImageRgba8(test_image);

            // Save to temp file
            let temp_file = std::env::temp_dir().join("kitty_test.png");
            if let Err(_) = dynamic_img.save(&temp_file) {
                return false;
            }

            // Try to display with kitty protocol
            let medium = kitty_image::Medium::File;
            let format = kitty_image::Format::Png;

            let action = kitty_image::Action::TransmitAndDisplay(
                kitty_image::ActionTransmission {
                    format,
                    medium,
                    width: 1,
                    height: 1,
                    ..Default::default()
                },
                kitty_image::ActionPut::default(),
            );

            let command = kitty_image::Command::with_payload_from_path(action, &temp_file);
            let wrapped = kitty_image::WrappedCommand::new(command);

            // Try to write to stdout - if this fails, Kitty protocol isn't working
            let mut stdout = std::io::stdout();
            let write_result = write!(stdout, "{}", wrapped);

            // Clean up
            let _ = std::fs::remove_file(temp_file);

            write_result.is_ok()
        });

        match test_result {
            Ok(true) => {
                info!("Kitty graphics protocol test succeeded");
                true
            }
            _ => {
                warn!("Kitty graphics protocol test failed, will use fallback rendering");
                false
            }
        }
    }

    fn render_kitty(&mut self, frame: &VideoFrame) -> Result<()> {
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        use std::io::Write;
        use image::ImageEncoder;

        // Encode the frame as PNG in-memory
        let mut png_data = Vec::new();
        {
            let img = frame.image.to_rgba8();
            let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
            encoder.write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ColorType::Rgba8.into(),
            )?;
        }
        let encoded = BASE64.encode(&png_data);
        // Kitty protocol: ESC_G ... ESC\\
        // See https://sw.kovidgoyal.net/kitty/graphics-protocol/
        let mut stdout = std::io::stdout();
        // Position cursor
        write!(stdout, "\x1B[{};{}H", self.config.y + 1, self.config.x + 1)?;
        // Compose the escape sequence
        let seq = format!(
            "\x1B_Gf=100,a=T,s={},v={},m=1;{}\x1B\\",
            frame.width,
            frame.height,
            encoded
        );
        write!(stdout, "{}", seq)?;
        stdout.flush()?;
        Ok(())
    }

    fn render_iterm(&self, frame: &VideoFrame) -> Result<()> {
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
        use image::ImageEncoder;
        use std::io::Write;
        // Encode the frame as PNG in-memory
        let mut png_data = Vec::new();
        {
            let img = frame.image.to_rgba8();
            let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
            encoder.write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ColorType::Rgba8.into(),
            )?;
        }
        let encoded = BASE64.encode(&png_data);
        // iTerm2: ESC ] 1337 ; File = ... : base64 ST
        let args = format!("inline=1;width={};height={}", frame.width, frame.height);
        let seq = format!("\x1B]1337;File={}:{}\x07", args, encoded);
        let mut stdout = std::io::stdout();
        write!(stdout, "\x1B[{};{}H", self.config.y + 1, self.config.x + 1)?;
        write!(stdout, "{}", seq)?;
        stdout.flush()?;
        Ok(())
    }

    fn render_sixel(&self, frame: &VideoFrame) -> Result<()> {
        use std::io::Write;
        // Use the 'sixel' crate if available, otherwise do a minimal quantization
        // Here, we do a minimal quantization and encode manually
        let img = frame.image.to_rgba8();
        let width = img.width() as usize;
        let height = img.height() as usize;
        // Quantize to 16 colors (simple median cut)
        let mut palette = vec![[0u8, 0u8, 0u8]; 16];
        for (i, color) in palette.iter_mut().enumerate() {
            let v = (i * 255 / 15) as u8;
            *color = [v, v, v];
        }
        // Sixel header
        let mut sixel = Vec::new();
        sixel.extend_from_slice(b"\x1BPq");
        for (i, color) in palette.iter().enumerate() {
            write!(&mut sixel, "#{};2;{};{};{}", i, color[0], color[1], color[2])?;
        }
        let rows = (height + 5) / 6;
        for y_block in 0..rows {
            for color_idx in 0..palette.len() {
                let mut col_data = Vec::new();
                for x in 0..width {
                    let mut pattern = 0u8;
                    for bit in 0..6 {
                        let y = y_block * 6 + bit;
                        if y < height {
                            let px = img.get_pixel(x as u32, y as u32).0;
                            let idx = palette.iter().enumerate().min_by_key(|(_, c)| {
                                let dr = px[0] as i16 - c[0] as i16;
                                let dg = px[1] as i16 - c[1] as i16;
                                let db = px[2] as i16 - c[2] as i16;
                                dr * dr + dg * dg + db * db
                            }).map(|(i, _)| i).unwrap_or(0);
                            if idx == color_idx { pattern |= 1 << bit; }
                        }
                    }
                    col_data.push(pattern);
                }
                let mut started = false;
                for &pat in &col_data {
                    if pat != 0 {
                        if !started {
                            write!(&mut sixel, "#{}", color_idx)?;
                            started = true;
                        }
                        sixel.push(b'?' + pat);
                    } else if started {
                        sixel.push(b' ');
                    }
                }
                if started { sixel.push(b'$'); }
            }
            sixel.push(b'-');
        }
        sixel.extend_from_slice(b"\x1B\\");
        let mut stdout = std::io::stdout();
        write!(stdout, "\x1B[{};{}H", self.config.y + 1, self.config.x + 1)?;
        stdout.write_all(&sixel)?;
        stdout.flush()?;
        Ok(())
    }

    // Helper method to find the closest color in the palette
    fn closest_color_idx(&self, pixel: [u8; 4], palette: &[[u8; 3]]) -> usize {
        // Skip fully transparent pixels
        if pixel[3] == 0 {
            return 0; // Use black/background for transparent pixels
        }

        // Find the closest color by simple RGB distance
        let mut best_idx = 0;
        let mut best_dist = u32::MAX;

        // Apply alpha blending with black background
        let r = (pixel[0] as u32 * pixel[3] as u32) / 255;
        let g = (pixel[1] as u32 * pixel[3] as u32) / 255;
        let b = (pixel[2] as u32 * pixel[3] as u32) / 255;

        for (i, color) in palette.iter().enumerate() {
            let dr = r as i32 - color[0] as i32;
            let dg = g as i32 - color[1] as i32;
            let db = b as i32 - color[2] as i32;

            let dist = (dr * dr + dg * dg + db * db) as u32;

            if dist < best_dist {
                best_dist = dist;
                best_idx = i;
            }
        }

        best_idx
    }

    fn get_color_code(&self, fg: [u16; 3], bg: [u16; 3]) -> String {
        let mut cache = self.color_code_cache.lock();
        let key = (fg, bg);
        cache.entry(key).or_insert_with(|| {
            format!(
                "\x1B[38;2;{};{};{};48;2;{};{};{}m",
                fg[0], fg[1], fg[2], bg[0], bg[1], bg[2]
            )
        }).clone()
    }

    fn simd_blend_alpha(top: &[u8], bot: &[u8]) -> ([u16; 3], [u16; 3]) {
        // Stable, non-SIMD alpha blending for two RGBA pixels
        // Each slice must be 4 bytes (RGBA)
        let blend = |fg: &[u8; 4], bg: &[u8; 4]| -> [u16; 3] {
            let a = fg[3] as u16;
            let r = (fg[0] as u16 * a + bg[0] as u16 * (255 - a)) / 255;
            let g = (fg[1] as u16 * a + bg[1] as u16 * (255 - a)) / 255;
            let b = (fg[2] as u16 * a + bg[2] as u16 * (255 - a)) / 255;
            [r, g, b]
        };
        ([blend(&top.try_into().unwrap(), &[0,0,0,255]), blend(&bot.try_into().unwrap(), &[0,0,0,255])][0],
         [blend(&bot.try_into().unwrap(), &[0,0,0,255])][0])
    }

    fn render_blocks(&mut self, frame: &VideoFrame) -> Result<()> {
        use std::io::Write;
        let img = frame.image.to_rgba8();
        let width = img.width() as usize;
        let height = img.height() as usize;
        let visible_width = width.min(self.term_width as usize - self.config.x as usize);
        let visible_height = (height.min(self.term_height as usize * 2 - self.config.y as usize) + 1) / 2;
        if visible_width == 0 || visible_height == 0 {
            warn!("Cannot render frame - visible area is zero: {}x{}", visible_width, visible_height);
            return Ok(());
        }
        let mut stdout = std::io::stdout();
        if let Err(e) = write!(stdout, "\x1B[s\x1B[?25l") {
            error!("Failed to set cursor state: {}", e);
            return Err(anyhow!("Failed to set cursor state: {}", e));
        }
        let mut output = String::with_capacity(visible_width * visible_height * 25);
        output.clear();
        write!(stdout, "\x1B[{};{}H", self.config.y + 1, self.config.x + 1).ok();
        // Precompute column transparency for skipping
        let mut col_transparent = vec![true; visible_width];
        for x in 0..visible_width {
            for y in 0..height {
                if img.get_pixel(x as u32, y as u32).0[3] > 0 {
                    col_transparent[x] = false;
                    break;
                }
            }
        }
        // Frame diffing: hash each row for dirty rectangle
        let mut row_hashes = vec![0u64; visible_height];
        let mut dirty_rows = vec![true; visible_height];
        for y in 0..visible_height {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for x in 0..visible_width {
                let y_top = y * 2;
                let y_bottom = y * 2 + 1;
                let top = img.get_pixel(x as u32, y_top as u32).0;
                let bot = if y_bottom < height {
                    img.get_pixel(x as u32, y_bottom as u32).0
                } else {
                    [0, 0, 0, 255]
                };
                top.hash(&mut hasher);
                bot.hash(&mut hasher);
            }
            row_hashes[y] = hasher.finish();
            if let Some(prev) = &self.prev_frame_hash {
                if prev.get(y) == Some(&row_hashes[y]) {
                    dirty_rows[y] = false;
                }
            }
        }
        // Parallel row rendering
        let rendered_rows: Vec<String> = (0..visible_height).into_par_iter().map(|y| {
            if dirty_rows[y] {
                let mut row = String::with_capacity(visible_width * 25);
                row.push_str(&format!("\x1B[{};{}H", self.config.y as usize + y + 1, self.config.x as usize + 1));
                let mut last_fg_color = [999u16; 3];
                let mut last_bg_color = [999u16; 3];
                let mut last_code = String::new();
                let mut run_len = 0;
                let mut run_char = ' ';
                let mut top_rgba = [0u8; 4];
                let mut bot_rgba = [0u8; 4];
                for x in 0..visible_width {
                    if col_transparent[x] {
                        if run_char != ' ' {
                            row.push_str(&last_code);
                            for _ in 0..run_len { row.push(run_char); }
                            run_len = 0;
                        }
                        run_char = ' ';
                        run_len += 1;
                        continue;
                    }
                    let y_top = y * 2;
                    let y_bottom = y * 2 + 1;
                    top_rgba.copy_from_slice(&img.get_pixel(x as u32, y_top as u32).0);
                    if y_bottom < height {
                        bot_rgba.copy_from_slice(&img.get_pixel(x as u32, y_bottom as u32).0);
                    } else {
                        bot_rgba = [0, 0, 0, 255];
                    }
                    if top_rgba[3] == 0 && bot_rgba[3] == 0 {
                        if run_char != ' ' {
                            row.push_str(&last_code);
                            for _ in 0..run_len { row.push(run_char); }
                            run_len = 0;
                        }
                        run_char = ' ';
                        run_len += 1;
                        continue;
                    }
                    let (top_rgb, bot_rgb) = Self::simd_blend_alpha(&top_rgba, &bot_rgba);
                    let fg_changed = top_rgb != last_fg_color;
                    let bg_changed = bot_rgb != last_bg_color;
                    let code = if fg_changed || bg_changed {
                        self.get_color_code(top_rgb, bot_rgb)
                    } else {
                        last_code.clone()
                    };
                    if run_char != '▀' || code != last_code {
                        if run_len > 0 {
                            row.push_str(&last_code);
                            for _ in 0..run_len { row.push(run_char); }
                        }
                        run_char = '▀';
                        run_len = 1;
                        last_code = code.clone();
                        last_fg_color = top_rgb;
                        last_bg_color = bot_rgb;
                    } else {
                        run_len += 1;
                    }
                }
                if run_len > 0 {
                    row.push_str(&last_code);
                    for _ in 0..run_len { row.push(run_char); }
                }
                row.push_str("\x1B[0m");
                row
            } else {
                String::new()
            }
        }).collect();
        for row in rendered_rows {
            if !row.is_empty() {
                output.push_str(&row);
            }
        }
        output.push_str("\x1B[0m\x1B[u\x1B[?25h");
        if let Err(e) = write!(stdout, "{}", output) {
            error!("Failed to write blocks output: {}", e);
            return Err(anyhow!("Failed to write blocks output: {}", e));
        }
        if let Err(e) = stdout.flush() {
            error!("Failed to flush stdout: {}", e);
            return Err(anyhow!("Failed to flush stdout: {}", e));
        }
        // Save row hashes for next frame
        self.prev_frame_hash = Some(row_hashes);
        Ok(())
    }
}
