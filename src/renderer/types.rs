use image::RgbaImage;
use std::time::{Duration, Instant};

/// A processed video frame ready for display
#[derive(Debug, Clone)]
pub struct ProcessedFrame {
    /// Frame data as RGBA image
    pub image: RgbaImage,
    /// Frame timestamp in video timeline
    pub timestamp: Duration,
    /// Frame sequence number
    pub frame_number: u64,
    /// Frame dimensions
    pub dimensions: (u32, u32),
    /// Option GPU texture handle
    pub gpu_texture: Option<wgpu::Texture>,
}

/// Commands for controlling video playback
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackCommand {
    Play,
    Pause,
    Stop,
    Seek(Duration),
    SetSpeed(f32),
    NextFrame,
    PrevFrame,
}

/// Status updates from rendering components
#[derive(Debug, Clone)]
pub enum RenderStatus {
    /// GPU processing status
    GpuProcessing {
        frames_processed: u64,
        total_frames: u64,
        processing_fps: f32,
    },
    /// Buffer status
    BufferStatus {
        buffered_frames: u64,
        buffer_capacity: u64,
        buffer_percentage: f32,
    },
    /// Playback status
    PlaybackStatus {
        current_frame: u64,
        timestamp: Duration,
        is_playing: bool,
        playback_speed: f32,
    },
    /// Display status
    DisplayStatus {
        frames_rendered: u64,
        display_fps: f32,
        terminal_size: (u16, u16),
    },
    /// Error status
    Error(String),
}

/// Main renderer configuration
#[derive(Debug, Clone)]
pub struct RendererConfig {
    /// Frame buffer config
    pub buffer_capacity: usize,
    /// GPU processing config
    pub gpu_config: GpuConfig,
    /// Display config
    pub display_config: DisplayConfig,
    /// Timing config
    pub timing_config: TimingConfig,
}

/// GPU processing configuration
#[derive(Debug, Clone)]
pub struct GpuConfig {
    /// Preferred GPU adapter
    pub adapter_pref: wgpu::PowerPreference,
    /// Enable GPU acceleration
    pub enable_gpu: bool,
    /// Max GPU memory usage in MB
    pub max_gpu_mem_mb: u64,
    /// Parallel processing threads
    pub processing_threads: usize,
}

/// Display configuration for Kitty protocol
#[derive(Debug, Clone)]
pub struct DisplayConfig {
    /// Target terminal dimensions
    pub terminal_size: Option<(u16, u16)>,
    /// Image quality (0.0 - 1.0)
    pub image_quality: f32,
    /// Enable image compression
    pub enable_compression: bool,
    /// Kitty protocol features
    pub kitty_features: KittyFeatures,
}

/// Kitty protocol feature support
#[derive(Debug, Clone)]
pub struct KittyFeatures {
    /// Support for image transmission
    pub transmit_images: bool,
    /// Support for image display
    pub display_images: bool,
    /// Support for image deletion
    pub delete_images: bool,
    /// Support for image composition
    pub compose_images: bool,
}

/// Timing and scheduling configuration
#[derive(Debug, Clone)]
pub struct TimingConfig {
    /// Target playback FPS
    pub target_fps: f32,
    /// Buffer threshold percentage (0.0 - 1.0)
    pub buffer_threshold: f32,
    /// Enable adaptive buffering
    pub adaptive_buffering: bool,
    /// Frame timing precision
    pub timing_precision: Duration,
}

/// Video metadata information
#[derive(Debug, Clone)]
pub struct VideoMetadata {
    /// Video duration
    pub duration: Duration,
    /// Total frame count
    pub total_frames: u64,
    /// Frame rate
    pub fps: f32,
    /// Video dimensions
    pub dimensions: (u32, u32),
    /// Video codec
    pub codec: String,
    /// File size in bytes
    pub file_size: u64,
}

/// Frame buffer entry
#[derive(Debug)]
pub struct BufferEntry {
    /// The processed frame
    pub frame: ProcessedFrame,
    /// When this frame was processed
    pub processed_at: Instant,
    /// Memory usage of this frame
    pub mem_usage: usize,
}

/// GPU processing stats
#[derive(Debug, Clone, Default)]
pub struct GpuStats {
    /// Total frames processed
    pub frames_processed: u64,
    /// Processing rate (frames per second)
    pub processing_fps: f32,
    /// GPU memory usage in bytes
    pub gpu_mem_used: u64,
    /// Avg. processing time/frame
    pub avg_processing_time: Duration,
}

/// Detailed GPU Memory Stats
#[derive(Debug, Clone)]
pub struct GpuMemStats {
    /// Current GPU memory usage in bytes
    pub current_usage_bytes: u64,
    /// Peak GPU memory usage in bytes
    pub peak_usage_bytes: u64,
    /// Number of active textures
    pub active_textures: u64,
    /// Memory usage as percentage of limit
    pub usage_percentage: f32,
}

/// Display stats
#[derive(Debug, Clone, Default)]
pub struct DisplayStats {
    /// Total frames displayed
    pub frames_displayed: u64,
    /// Display rate (fps)
    pub display_fps: f32,
    /// Avg. frame display time
    pub avg_display_time: Duration,
    /// Terminal refresh rate
    pub terminal_refresh_rate: f32,
}

// Default implementations
impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            buffer_capacity: 900, // 30 seconds @ 30fps
            gpu_config: GpuConfig::default(),
            display_config: DisplayConfig::default(),
            timing_config: TimingConfig::default(),
        }
    }
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            adapter_pref: wgpu::PowerPreference::HighPerformance,
            enable_gpu: true,
            max_gpu_mem_mb: 256,
            processing_threads: num_cpus::get(),
        }
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            terminal_size: None, // Auto-detect
            image_quality: 0.8,
            enable_compression: true,
            kitty_features: KittyFeatures::default(),
        }
    }
}

impl Default for KittyFeatures {
    fn default() -> Self {
        Self {
            transmit_images: true,
            display_images: true,
            delete_images: true,
            compose_images: false,
        }
    }
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            target_fps: 30.0,
            buffer_threshold: 0.6, // 60% buffering strategy
            adaptive_buffering: true,
            timing_precision: Duration::from_millis(1),
        }
    }
}

// Unit tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_types_compile() {
        let _config = RendererConfig::default();
        let _gpu_config = GpuConfig::default();
        let _display_config = DisplayConfig::default();
        let _timing_config = TimingConfig::default();

        let _cmd = PlaybackCommand::Play;
        let _status = RenderStatus::Error("test".to_string());
    }
}
