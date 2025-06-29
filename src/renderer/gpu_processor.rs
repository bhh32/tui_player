//! GPU processing pipeline for video decoding and frame conversion

use crate::renderer::types::*;
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use ffmpeg_next as ffmpeg;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use wgpu::{Adapter, Device, Instance, Queue, Surface};

// Constants to standardize output to 720p @ 30fps
const TARGET_WIDTH: u32 = 1280; // 720p width
const TARGET_HEIGHT: u32 = 720; // 720p height
const TARGET_FPS: f32 = 30.0; // 30fps

/// GPU-accelerated video processor
#[derive(Clone)]
pub struct GpuProcessor {
    /// wgpu instance
    instance: Arc<Instance>,
    /// GPU adapter
    adapter: Arc<Adapter>,
    /// GPU device
    device: Arc<Device>,
    /// GPU command queue
    queue: Arc<Queue>,
    /// Processing config
    config: GpuConfig,
    /// Processing stats
    stats: Arc<RwLock<GpuStats>>,
    /// Frame processing time tracker
    frame_processing_times: Arc<RwLock<Vec<Duration>>>,
    /// GPU memory tracker
    gpu_mem_tracker: Arc<RwLock<GpuMemoryTracker>>,
}

/// GPU memory usage tracker
#[derive(Debug, Default)]
struct GpuMemoryTracker {
    /// Current texture memory usage in bytes
    texture_mem: u64,
    /// Peak texture memory usage
    peak_texture_mem: u64,
    /// Number of active textures
    active_textures: u64,
}

/// Video decoder wrapper
struct VideoDecoder {
    /// Ffmpeg format context
    format_context: ffmpeg::format::context::Input,
    /// Video stream index
    video_stream_idx: usize,
    /// Video decoder
    decoder: ffmpeg::decoder::Video,
    /// Frame converter for RGBA output
    converter: ffmpeg::software::scaling::Context,
    /// Current frame number
    current_frame: u64,
}

impl GpuProcessor {
    /// Create new GPU processor with specified config
    pub async fn new(config: GpuConfig) -> Result<Self> {
        // Init wgpu
        let instance = Arc::new(Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            backend_options: wgpu::BackendOptions::default(),
            flags: wgpu::InstanceFlags::default(),
        }));

        // Request adapter
        let adapter = Arc::new(
            instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: config.adapter_pref,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .context("Failed to find suitable GPU adapter")?,
        );

        // Request device and queue
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Video Processor Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
            })
            .await
            .context("Failed to create GPU device")?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            config,
            stats: Arc::new(RwLock::new(GpuStats::default())),
            frame_processing_times: Arc::new(RwLock::new(Vec::new())),
            gpu_mem_tracker: Arc::new(RwLock::new(GpuMemoryTracker::default())),
        })
    }

    /// Main processing loop - runs in dedicated thread
    pub async fn process_video(
        &mut self,
        video_path: &str,
        frame_buffer: Arc<RwLock<crate::renderer::frame_buffer::FrameBuffer>>,
        frame_tx: Sender<ProcessedFrame>,
        cmd_rx: Receiver<PlaybackCommand>,
        status_tx: Sender<RenderStatus>,
    ) -> Result<()> {
        // Init video decoder
        let mut decoder = self.init_video_decoder(video_path).await?;

        // Get video metadata
        let metadata = self.extract_metadata(&decoder)?;
        let total_frames = metadata.total_frames;

        // Caclualate 60% threshold
        let threshold_frame = (total_frames as f32 * 0.6) as u64;

        log::info!(
            "Starting GPU processing: {total_frames} total frames, threshold at frame {threshold_frame}"
        );

        let mut processing_start = Instant::now();
        let mut frames_processed = 0u64;

        // Main processing loop
        loop {
            // Check for commands (non-blocking)
            if let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    PlaybackCommand::Stop => {
                        log::info!("GPU processor received stop command");
                        break;
                    }
                    PlaybackCommand::Seek(timestamp) => {
                        decoder.seek_to_timestamp(timestamp)?;
                    }
                    _ => {} // Other commands handled by scheduler
                }
            }

            // Process next frame
            match self.process_next_frame(&mut decoder).await {
                Ok(Some(frame)) => {
                    frames_processed += 1;

                    // Send frame to buffer
                    if let Err(e) = frame_tx.send(frame) {
                        log::error!("Failed to send frame to buffer: {e}");
                        break;
                    }

                    // Update stats
                    self.update_processing_stats(frames_processed, total_frames, processing_start)
                        .await;

                    // Send status update
                    let _ = status_tx.send(RenderStatus::GpuProcessing {
                        frames_processed,
                        total_frames,
                        processing_fps: self
                            .calculate_processing_fps(frames_processed, processing_start),
                    });

                    // Check if the threshold has been reached for initial buffering
                    if frames_processed == threshold_frame {
                        log::info!("Reached 60% buffer threshold, playback can begin");
                    }
                }
                Ok(None) => {
                    log::info!("Video processing complete");
                    break;
                }
                Err(e) => {
                    log::error!("Frame processing error: {e}");
                    status_tx.send(RenderStatus::Error(e.to_string()));
                    break;
                }
            }
        }

        Ok(())
    }

    /// Init video decoder for the given file
    async fn init_video_decoder(&self, video_path: &str) -> Result<VideoDecoder> {
        // Init ffmpeg
        ffmpeg::init().context("Failed to initialize ffmpeg")?;

        // Open input file
        let format_context =
            ffmpeg::format::input(&video_path).context("Failed to open video file")?;

        // Find video stream
        let video_stream = format_context
            .streams()
            .best(ffmpeg::media::Type::Video)
            .context("No video stream found")?;

        let video_stream_idx = video_stream.index();

        // Create decoder
        let context_decoder =
            ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())
                .context("Failed to create codec context")?;

        let decoder = context_decoder
            .decoder()
            .video()
            .context("Failed to create video decoder")?;

        // Create scaling context for RGBA conversion
        let converter = ffmpeg::software::scaling::Context::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            ffmpeg::format::Pixel::RGBA,
            TARGET_WIDTH, // Always scale to 720p
            TARGET_HEIGHT,
            ffmpeg::software::scaling::Flags::BILINEAR,
        )
        .context("Failed to create scaling context")?;

        log::info!(
            "Video decoder initialized: {}x{} -> {}x{} @ {}fps",
            decoder.width(),
            decoder.height(),
            TARGET_WIDTH,
            TARGET_HEIGHT,
            TARGET_FPS
        );

        Ok(VideoDecoder {
            format_context,
            video_stream_idx,
            decoder,
            converter,
            current_frame: 0,
        })
    }

    /// Process the next video frame
    async fn process_next_frame(
        &self,
        decoder: &mut VideoDecoder,
    ) -> Result<Option<ProcessedFrame>> {
        let frame_start_time = Instant::now();
        let packet = ffmpeg::Packet::empty();

        // Read packets until a video frame is gotten
        for (stream, packet) in decoder.format_context.packets() {
            if stream.index() == decoder.video_stream_idx {
                decoder.decoder.send_packet(&packet)?;

                let mut decoded_frame = ffmpeg::frame::Video::empty();

                if decoder.decoder.receive_frame(&mut decoded_frame).is_ok() {
                    // Convert frame to RGBA
                    let mut rgba_frame = ffmpeg::frame::Video::empty();
                    decoder.converter.run(&decoded_frame, &mut rgba_frame)?;

                    // Create GPU texture
                    let gpu_texture = self.create_frame_texture(&rgba_frame).await?;

                    // Convert to ProcessedFrame format
                    let processed_frame = self.convert_to_processed_frame(
                        rgba_frame,
                        decoder.current_frame,
                        Some(gpu_texture),
                    )?;

                    // Record processing time
                    let processing_time = frame_start_time.elapsed();
                    self.record_processing_time(processing_time).await;

                    decoder.current_frame += 1;
                    return Ok(Some(processed_frame));
                }
            }
        }

        Ok(None) // End of stream
    }

    /// Create GPU texture from video frame
    async fn create_frame_texture(&self, frame: &ffmpeg::frame::Video) -> Result<wgpu::Texture> {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("video_frame_texture"),
            size: wgpu::Extent3d {
                width: TARGET_WIDTH,
                height: TARGET_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Copy frame data to GPU texture
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            frame.data(0),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TARGET_WIDTH * 4), // RGBA = 4 bytes
                rows_per_image: Some(TARGET_HEIGHT),
            },
            wgpu::Extent3d {
                width: TARGET_WIDTH,
                height: TARGET_HEIGHT,
                depth_or_array_layers: 1,
            },
        );

        Ok(texture)
    }

    /// Convert ffmpeg frame to ProcessedFrame
    fn convert_to_processed_frame(
        &self,
        frame: ffmpeg::frame::Video,
        frame_number: u64,
        gpu_texture: Option<wgpu::Texture>,
    ) -> Result<ProcessedFrame> {
        // Convert frame data to RGBAImage
        let frame_data = frame.data(0).to_vec();
        let image = image::RgbaImage::from_raw(TARGET_WIDTH, TARGET_HEIGHT, frame_data)
            .context("Failed to create RgbaImage from frame data")?;

        // Calculate timestamp based on frame rate
        let timestamp = Duration::from_secs_f64(frame_number as f64 / 30.0); // Assuming 30fps

        Ok(ProcessedFrame {
            image,
            timestamp,
            frame_number,
            dimensions: (TARGET_WIDTH, TARGET_HEIGHT),
            gpu_texture,
        })
    }

    // Extract video metadata
    fn extract_metadata(&self, decoder: &VideoDecoder) -> Result<VideoMetadata> {
        let stream = decoder
            .format_context
            .streams()
            .nth(decoder.video_stream_idx)
            .context("Video stream not found")?;

        let duration = Duration::from_secs(
            decoder.format_context.duration() as u64 / ffmpeg::ffi::AV_TIME_BASE as u64,
        );

        let total_frames = (duration.as_secs_f64() * TARGET_FPS as f64) as u64;

        // Get original video info for logging
        let original_fps = stream.rate();
        log::info!(
            "Video metadata - Original: {}x{} @ {:.2}fps, Output: {}x{} @ {}fps, Duration: {:.2}s, Frames: {}",
            decoder.decoder.width(),
            decoder.decoder.height(),
            original_fps.0 as f32 / original_fps.1 as f32,
            TARGET_WIDTH,
            TARGET_HEIGHT,
            TARGET_FPS,
            duration.as_secs_f64(),
            total_frames
        );

        Ok(VideoMetadata {
            duration,
            total_frames,
            fps: TARGET_FPS,
            dimensions: (TARGET_WIDTH, TARGET_HEIGHT),
            codec: decoder.decoder.id().name().to_string(),
            file_size: 0, // Need to stat the file to get this
        })
    }

    /// Caculate current processing FPS
    fn calculate_processing_fps(&self, frames_processed: u64, start_time: Instant) -> f32 {
        let elapsed = start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            frames_processed as f32 / elapsed as f32
        } else {
            0.0
        }
    }

    /// Update processing stats
    async fn update_processing_stats(
        &self,
        frames_processed: u64,
        total_frames: u64,
        start_time: Instant,
    ) {
        let mut stats = self.stats.write().await;

        // Basic stats
        stats.frames_processed = frames_processed;
        stats.processing_fps = self.calculate_processing_fps(frames_processed, start_time);

        // GPU memory usage
        let mem_tracker = self.gpu_mem_tracker.read().await;
        stats.gpu_mem_used = mem_tracker.texture_mem;

        // Avg. processing time
        let processing_time = self.frame_processing_times.read().await;

        if !processing_time.is_empty() {
            let total_time: Duration = processing_time.iter().sum();
            stats.avg_processing_time = total_time / processing_time.len() as u32;
        }

        // Log performance analytics periodically
        if frames_processed % 100 == 0 {
            log::debug!(
                "GPU Stats - Frames: {}/{}, FPS: {:.2}, GPU Memory: {:.2}MB, Avg Time: {:2}ms",
                frames_processed,
                total_frames,
                stats.processing_fps,
                stats.gpu_mem_used as f64 / (1024.0 * 1024.0),
                stats.avg_processing_time.as_millis()
            );
        }
    }

    /// Get current processing stats
    pub async fn get_stats(&self) -> GpuStats {
        self.stats.read().await.clone()
    }

    // Helper methods for memory and timing tracking

    /// Calculate texture memory usage
    fn calculate_texture_mem_usage(&self, frame: &ffmpeg::frame::Video) -> u64 {
        // 720p RGBA = 1280 * 720 * 4 = 3,686,400 bytes (~3.5MB)
        (TARGET_WIDTH * TARGET_HEIGHT * 4) as u64
    }

    /// Update GPU memory usage tracking
    async fn update_gpu_mem_usage(&self, texture_size: u64, is_allocation: bool) {
        let mut tracker = self.gpu_mem_tracker.write().await;

        if is_allocation {
            tracker.texture_mem += texture_size;
            tracker.active_textures += 1;

            // Track peak usage
            if tracker.texture_mem > tracker.peak_texture_mem {
                tracker.peak_texture_mem = tracker.texture_mem;
            }
        } else {
            // Texture deallocation
            tracker.texture_mem = tracker.texture_mem.saturating_sub(texture_size);
            tracker.active_textures = tracker.active_textures.saturating_sub(1);
        }
    }

    /// Record frame processing time
    async fn record_processing_time(&self, processing_time: Duration) {
        let mut times = self.frame_processing_times.write().await;

        // Keep a rolling window of the last 100 processing times
        if times.len() >= 100 {
            times.remove(0);
        }

        times.push(processing_time);
    }

    /// Get detailed GPU memory stats
    pub async fn get_memory_stats(&self) -> GpuMemStats {
        let tracker = self.gpu_mem_tracker.read().await;

        GpuMemStats {
            current_usage_bytes: tracker.texture_mem,
            peak_usage_bytes: tracker.peak_texture_mem,
            active_textures: tracker.active_textures,
            usage_percentage: (tracker.texture_mem as f64
                / (self.config.max_gpu_mem_mb * 1024 * 1024) as f64
                * 100.0) as f32,
        }
    }
}

impl VideoDecoder {
    /// Seek to specifc timestamp
    fn seek_to_timestamp(&mut self, timestamp: Duration) -> Result<()> {
        let seek_timestamp = (timestamp.as_secs_f64() * ffmpeg::ffi::AV_TIME_BASE as f64) as i64;
        self.format_context.seek(seek_timestamp, ..seek_timestamp)?;

        Ok(())
    }
}
