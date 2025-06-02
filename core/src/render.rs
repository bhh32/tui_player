mod gpu;
#[cfg(test)]
mod tests;

use log::{debug, error, info, trace, warn};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use crossterm::terminal;

use crate::video::VideoFrame;

use crate::render::gpu::GpuProcessor;

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
        static mut LAST_METHOD: Option<RenderMethod> = None;
        static mut KITTY_FAILED: bool = false;
        static mut ITERM_FAILED: bool = false;
        static mut SIXEL_FAILED: bool = false;

        // Determine which method to use, with better fallback handling
        let current_method = unsafe {
            // If any method has previously failed, avoid using it
            if KITTY_FAILED && self.effective_method == RenderMethod::Kitty {
                debug!("Kitty previously failed, using Blocks instead");
                RenderMethod::Blocks
            } else if ITERM_FAILED && self.effective_method == RenderMethod::ITerm {
                debug!("iTerm previously failed, using Blocks instead");
                RenderMethod::Blocks
            } else if SIXEL_FAILED && self.effective_method == RenderMethod::Sixel {
                debug!("Sixel previously failed, using Blocks instead");
                RenderMethod::Blocks
            } else if let Some(method) = LAST_METHOD {
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
                // Add timeout protection for Kitty rendering
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
                        unsafe {
                            KITTY_FAILED = true;
                            LAST_METHOD = Some(RenderMethod::Blocks);
                        }
                        // Try to clear any partial output first
                        let _ = write!(std::io::stdout(), "\x1B[2J\x1B[H");
                        std::io::stdout().flush().ok();
                        self.render_blocks(&resized_frame)
                    }
                    Err(e) => {
                        error!("Kitty rendering panicked: {:?}", e);
                        warn!("Permanently falling back to blocks rendering");
                        unsafe {
                            KITTY_FAILED = true;
                            LAST_METHOD = Some(RenderMethod::Blocks);
                        }
                        // Try to clear any partial output first
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
                        unsafe {
                            ITERM_FAILED = true;
                            LAST_METHOD = Some(RenderMethod::Blocks);
                        }
                        // Try to clear any partial output first
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
                        unsafe {
                            SIXEL_FAILED = true;
                            LAST_METHOD = Some(RenderMethod::Blocks);
                        }
                        // Try to clear any partial output first
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
                unsafe {
                    LAST_METHOD = Some(RenderMethod::Blocks);
                }
                result
            }
        };

        // Store the successful method for future frames
        if result.is_ok() {
            unsafe {
                LAST_METHOD = Some(current_method);
            }
        }

        // Update frame time after rendering
        self.last_frame_time = std::time::Instant::now();

        result
    }

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
        use kitty_image::{Action, ActionTransmission, Command, Format, Medium, WrappedCommand};
        use std::io::Write;
        use std::time::Instant;

        // Create a new temp file for each frame to avoid issues with locked files
        // Generate a unique filename based on timestamp and a random number
        let rand_suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();

        let temp_file = std::env::temp_dir().join(format!(
            "kitty_frame_{}_{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            rand_suffix
        ));

        // Clean up previous temp file if it exists
        if let Some(ref old_path) = self.last_kitty_temp_file {
            if old_path.exists() {
                let _ = std::fs::remove_file(old_path);
            }
        }

        // Store new temp file path
        self.last_kitty_temp_file = Some(temp_file.clone());

        // Setup for kitty rendering
        let medium = Medium::File;
        let format = Format::Png;

        // Log information about the frame
        debug!(
            "Rendering Kitty frame: {}x{} to file {:?}",
            frame.width, frame.height, temp_file
        );

        // Time the image saving operation
        let save_start = Instant::now();

        // Save the frame as PNG - most reliable format for Kitty
        match frame.image.save(&temp_file) {
            Ok(_) => {
                // File saved successfully
                let save_time = save_start.elapsed();
                if save_time.as_millis() > 50 {
                    warn!("Image save took {}ms", save_time.as_millis());
                }
            }
            Err(e) => {
                error!("Failed to save image: {}", e);
                return Err(anyhow!("Failed to save frame to file: {}", e));
            }
        }

        // Verify the file exists and has content
        if !temp_file.exists() {
            return Err(anyhow!("Temp file not created at {:?}", temp_file));
        }

        let metadata = std::fs::metadata(&temp_file)?;
        if metadata.len() == 0 {
            return Err(anyhow!("Temp file is empty: {:?}", temp_file));
        }

        // Create the action with the correct dimensions
        let action = Action::TransmitAndDisplay(
            ActionTransmission {
                format,
                medium,
                width: frame.width as u32,
                height: frame.height as u32,
                // Position using offsets - x, y coordinates
                offset: self.config.x as u32,
                ..Default::default()
            },
            kitty_image::ActionPut::default(),
        );

        // Create a command with the file path
        debug!("Creating Kitty command with file: {:?}", temp_file);
        let command = Command::with_payload_from_path(action, &temp_file);

        // Wrap the command in escape codes
        let wrapped_command = WrappedCommand::new(command);

        // Position cursor at the specified location and render with buffer to prevent flicker
        let mut stdout = std::io::stdout();

        // Position cursor
        if let Err(e) = write!(stdout, "\x1B[{};{}H", self.config.y + 1, self.config.x + 1) {
            error!("Failed to position cursor: {}", e);
            return Err(anyhow!("Failed to position cursor: {}", e));
        }

        // Time the Kitty command execution
        let command_start = Instant::now();

        // Write the command with timeout protection
        if let Err(e) = write!(stdout, "{}", wrapped_command) {
            error!("Failed to write Kitty command: {}", e);
            return Err(anyhow!("Failed to write Kitty command: {}", e));
        }

        // Flush all at once to prevent flicker
        if let Err(e) = stdout.flush() {
            error!("Failed to flush stdout: {}", e);
            return Err(anyhow!("Failed to flush stdout: {}", e));
        }

        let command_time = command_start.elapsed();
        if command_time.as_millis() > 100 {
            warn!(
                "Kitty command execution took {}ms - this may indicate issues",
                command_time.as_millis()
            );
            if command_time.as_millis() > 1000 {
                // If rendering is consistently very slow, it's likely not working properly
                error!("Kitty rendering is extremely slow, may not be functioning properly");
                return Err(anyhow!("Kitty rendering timeout - too slow"));
            }
        }

        debug!("Kitty rendering complete");
        Ok(())
    }

    fn render_iterm(&self, frame: &VideoFrame) -> Result<()> {
        use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
        use image::ImageEncoder;
        use std::io::Write;

        log::debug!(
            "Rendering frame with iTerm protocol (width: {}, height: {})",
            frame.width,
            frame.height
        );

        // Create a buffer for the PNG data
        let mut png_data = Vec::new();

        // Encode the image as PNG directly to the buffer
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

        // Base64 encode the PNG data
        let encoded = BASE64.encode(&png_data);

        // Build iTerm2 image protocol escape sequence
        // Format: ESC ] 1337 ; File = [arguments] : base64 data ST
        let args = format!("inline=1;width={};height={}", frame.width, frame.height);

        // iTerm2 uses OSC escape sequence (ESC]), terminated with BEL (^G)
        let image_sequence = format!("\x1B]1337;File={}:{}\x07", args, encoded);

        // Position cursor at the specified location
        print!("\x1B[{};{}H", self.config.y + 1, self.config.x + 1);

        // Output the image data
        print!("{}", image_sequence);
        std::io::stdout().flush()?;

        Ok(())
    }

    fn render_sixel(&self, frame: &VideoFrame) -> Result<()> {
        use std::io::Write;
        use std::time::Instant;

        log::debug!(
            "Rendering frame with Sixel protocol (width: {}, height: {})",
            frame.width,
            frame.height
        );

        // Performance measurement
        let start_time = Instant::now();

        // Get RGBA data from the frame
        let img = frame.image.to_rgba8();

        // Sixel uses a color palette, so we need to quantize the image
        // We'll use a simple fixed palette for performance
        let mut sixel_data = Vec::new();

        // Sixel header
        sixel_data.extend_from_slice(b"\x1BPq");

        // Define our color palette (we'll use a reduced set for performance)
        // Actual Sixel allows up to 256 colors, but we'll use fewer for speed
        let colors = [
            // Basic colors - black, red, green, yellow, blue, magenta, cyan, white
            [0, 0, 0],
            [255, 0, 0],
            [0, 255, 0],
            [255, 255, 0],
            [0, 0, 255],
            [255, 0, 255],
            [0, 255, 255],
            [255, 255, 255],
            // Some grayscale levels
            [85, 85, 85],
            [170, 170, 170],
            // Some additional colors for better representation
            [128, 0, 0],
            [0, 128, 0],
            [128, 128, 0],
            [0, 0, 128],
            [128, 0, 128],
            [0, 128, 128],
        ];

        // Define the palette in Sixel format
        for (i, color) in colors.iter().enumerate() {
            write!(
                &mut sixel_data,
                "#{};2;{};{};{}",
                i, color[0], color[1], color[2]
            )?;
        }

        // Process the image in blocks of 6 pixels (Sixel height)
        let width = frame.width as usize;
        let height = frame.height as usize;

        // Calculate the number of 6-pixel rows we need
        let rows = (height + 5) / 6;

        // For each row of 6 pixels
        for y_block in 0..rows {
            // For each color in our palette
            for color_idx in 0..colors.len() {
                let mut has_pixels = false;

                // For each pixel in the row
                for x in 0..width {
                    // Build the 6-bit pattern for this pixel column
                    let mut pattern = 0;

                    // Check 6 pixels vertically
                    for bit in 0..6 {
                        let y = y_block * 6 + bit;
                        if y < height {
                            // Get pixel color
                            let pixel = img.get_pixel(x as u32, y as u32).0;

                            // Find the closest color in our palette
                            if self.closest_color_idx(pixel, &colors) == color_idx {
                                pattern |= 1 << bit;
                                has_pixels = true;
                            }
                        }
                    }

                    // If this pixel column has pixels of the current color
                    if pattern > 0 {
                        // Select the color
                        if !has_pixels {
                            write!(&mut sixel_data, "#{}", color_idx)?;
                            has_pixels = true;
                        }

                        // Encode the pattern
                        // Add 63 to get into the Sixel character range (ASCII 63 to 126)
                        sixel_data.push(b'?' + pattern);
                    }
                }

                // End of the line for this color
                if has_pixels {
                    sixel_data.push(b'$');
                }
            }

            // Move to the next row of 6 pixels
            sixel_data.push(b'-');
        }

        // Sixel footer
        sixel_data.extend_from_slice(b"\x1B\\");

        // Position cursor at the specified location
        print!("\x1B[{};{}H", self.config.y + 1, self.config.x + 1);

        // Output the Sixel data
        std::io::stdout().write_all(&sixel_data)?;
        std::io::stdout().flush()?;

        let elapsed = start_time.elapsed();
        if elapsed.as_millis() > 50 {
            log::warn!("Sixel rendering took {}ms", elapsed.as_millis());
        }

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

    fn render_blocks(&self, frame: &VideoFrame) -> Result<()> {
        use std::io::Write;

        // Get RGB data from the frame
        let img = frame.image.to_rgba8();
        let width = img.width() as usize;
        let height = img.height() as usize;

        // Calculate the visible area within the terminal
        let visible_width = width.min(self.term_width as usize - self.config.x as usize);
        let visible_height =
            (height.min(self.term_height as usize * 2 - self.config.y as usize) + 1) / 2;

        // Safety checks to prevent out-of-bounds accesses
        if visible_width == 0 || visible_height == 0 {
            warn!(
                "Cannot render frame - visible area is zero: {}x{}",
                visible_width, visible_height
            );
            return Ok(());
        }

        // Get stdout reference once to avoid multiple allocations
        let mut stdout = std::io::stdout();

        // Save cursor position and hide cursor
        if let Err(e) = write!(stdout, "\x1B[s\x1B[?25l") {
            error!("Failed to set cursor state: {}", e);
            return Err(anyhow!("Failed to set cursor state: {}", e));
        }

        // Reuse pre-allocated string buffer to avoid allocations
        let mut output = String::with_capacity(visible_width * visible_height * 25);
        output.clear(); // Clear any previous content

        // Optional: clear the rendering area first to prevent artifacts
        write!(stdout, "\x1B[{};{}H", self.config.y + 1, self.config.x + 1).ok();

        // Render using half-blocks - build the complete string first, then print once
        for y in 0..visible_height {
            // Move cursor to the start of the current line using simple string formatting
            output.push_str(&format!(
                "\x1B[{};{}H",
                self.config.y as usize + y + 1,
                self.config.x as usize + 1
            ));

            // For wide terminal windows, optimize by checking if entire rows can be skipped
            let mut all_transparent = true;
            for x in 0..visible_width {
                let y_top = y * 2;
                let y_bottom = y * 2 + 1;

                // Quick check for any non-transparent pixel in this row
                let top_alpha = img.get_pixel(x as u32, y_top as u32).0[3];
                let bottom_alpha = if y_bottom < height {
                    img.get_pixel(x as u32, y_bottom as u32).0[3]
                } else {
                    0
                };

                if top_alpha > 0 || bottom_alpha > 0 {
                    all_transparent = false;
                    break;
                }
            }

            // Skip row if all pixels are transparent
            if all_transparent {
                output.push(' ');
                continue;
            }

            // Process row in chunks for better cache locality
            const CHUNK_SIZE: usize = 32; // Increased from 16 to 32
            for chunk_start in (0..visible_width).step_by(CHUNK_SIZE) {
                let chunk_end = (chunk_start + CHUNK_SIZE).min(visible_width);

                // Cache for repeated color codes
                let mut last_fg_color = [999u16; 3]; // Invalid color to force first update
                let mut last_bg_color = [999u16; 3];

                for x in chunk_start..chunk_end {
                    // Each block character represents two vertically stacked pixels
                    let y_top = y * 2;
                    let y_bottom = y * 2 + 1;

                    // Get top pixel color
                    let top_color = img.get_pixel(x as u32, y_top as u32).0;

                    // Get bottom pixel color (black if at the bottom edge)
                    let bottom_color = if y_bottom < height {
                        img.get_pixel(x as u32, y_bottom as u32).0
                    } else {
                        [0, 0, 0, 255]
                    };

                    // Skip transparent pixels
                    if top_color[3] == 0 && bottom_color[3] == 0 {
                        // If we had colors set before, reset them
                        if last_fg_color[0] != 999 || last_bg_color[0] != 999 {
                            output.push_str("\x1B[0m");
                            last_fg_color = [999, 999, 999];
                            last_bg_color = [999, 999, 999];
                        }
                        output.push(' ');
                        continue;
                    }

                    // Calculate effective colors accounting for alpha using fast bit shifts
                    let top_r = ((top_color[0] as u16 * top_color[3] as u16) + 127) >> 8;
                    let top_g = ((top_color[1] as u16 * top_color[3] as u16) + 127) >> 8;
                    let top_b = ((top_color[2] as u16 * top_color[3] as u16) + 127) >> 8;

                    let bot_r = ((bottom_color[0] as u16 * bottom_color[3] as u16) + 127) >> 8;
                    let bot_g = ((bottom_color[1] as u16 * bottom_color[3] as u16) + 127) >> 8;
                    let bot_b = ((bottom_color[2] as u16 * bottom_color[3] as u16) + 127) >> 8;

                    // Check if colors changed from last pixel
                    let fg_changed = top_r != last_fg_color[0]
                        || top_g != last_fg_color[1]
                        || top_b != last_fg_color[2];
                    let bg_changed = bot_r != last_bg_color[0]
                        || bot_g != last_bg_color[1]
                        || bot_b != last_bg_color[2];

                    // Only output color codes if they changed
                    if fg_changed || bg_changed {
                        if fg_changed && bg_changed {
                            // Change both foreground and background at once
                            output.push_str(&format!(
                                "\x1B[38;2;{};{};{};48;2;{};{};{}m",
                                top_r, top_g, top_b, bot_r, bot_g, bot_b
                            ));
                            last_fg_color = [top_r, top_g, top_b];
                            last_bg_color = [bot_r, bot_g, bot_b];
                        } else if fg_changed {
                            // Change only foreground
                            output.push_str(&format!("\x1B[38;2;{};{};{}m", top_r, top_g, top_b));
                            last_fg_color = [top_r, top_g, top_b];
                        } else if bg_changed {
                            // Change only background
                            output.push_str(&format!("\x1B[48;2;{};{};{}m", bot_r, bot_g, bot_b));
                            last_bg_color = [bot_r, bot_g, bot_b];
                        }
                    }

                    // Add the half-block character
                    output.push('▀');
                }
            }
        }

        // Restore cursor position and make it visible again
        output.push_str("\x1B[0m\x1B[u\x1B[?25h");

        // Print all at once and flush
        if let Err(e) = write!(stdout, "{}", output) {
            error!("Failed to write blocks output: {}", e);
            return Err(anyhow!("Failed to write blocks output: {}", e));
        }

        if let Err(e) = stdout.flush() {
            error!("Failed to flush stdout: {}", e);
            return Err(anyhow!("Failed to flush stdout: {}", e));
        }

        Ok(())
    }
}
