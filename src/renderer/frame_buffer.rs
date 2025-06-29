//! Frame buffer manager with GPU memory allocation and 60% buffering strategy

use crate::renderer::types::*;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::{Device, Queue};

/// Frame buffer manager for pre-processed video frames
pub struct FrameBuffer {
    /// Ring buffer for processed frames
    frames: VecDeque<BufferEntry>,
    /// Max buffer capacity
    capacity: usize,
    /// Current buffer size in bytes
    current_mem_usage: usize,
    /// Max memory usage in bytes
    max_mem_usage: usize,
    /// GPU device for texture management
    gpu_device: Option<Arc<Device>>,
    /// GPU queue for operations
    gpu_queue: Option<Arc<Queue>>,
    /// Buffer stats
    stats: BufferStats,
}

/// Buffer statistics and metrics
#[derive(Debug, Clone, Default)]
pub struct BufferStats {
    /// Total frames buffered
    pub total_frames_buffered: u64,
    /// Frames dropped due to buffer full
    pub frames_dropped: u64,
    /// Current buffer utilization (0.0 - 1.0)
    pub buffer_utilization: f32,
    /// Memory utilization (0.0 - 1.0)
    pub mem_utilization: f32,
    /// Avg. frame size in bytes
    pub avg_frame_size: usize,
}

impl FrameBuffer {
    /// Create new frame buffer with specified capacity
    pub async fn with_capacity(capacity: usize) -> Result<Self> {
        let max_mem_usage = Self::calculate_max_mem(capacity);

        Ok(Self {
            frames: VecDeque::with_capacity(capacity),
            capacity,
            current_mem_usage: 0,
            max_mem_usage,
            gpu_device: None,
            gpu_queue: None,
            stats: BufferStats::default(),
        })
    }

    /// Add a processed frame to the buffer
    pub fn push_frame(&mut self, frame: ProcessedFrame) -> Result<()> {
        // Calculate frame mem usage
        let frame_mem = self.calculate_frame_mem(&frame);

        // Check if space needs to be made
        while self.should_drop_frame(frame_mem) {
            if let Some(dropped) = self.frames.pop_front() {
                self.current_mem_usage -= dropped.mem_usage;
                self.stats.frames_dropped += 1;

                // Clean up GPU texture if present
                if let Some(texture) = &dropped.frame.gpu_texture {
                    self.cleanup_gpu_texture(texture);
                }
            } else {
                break;
            }
        }

        // Create buffer entry
        let entry = BufferEntry {
            frame,
            processed_at: Instant::now(),
            mem_usage: frame_mem,
        };

        // Add to buffer
        self.frames.push_back(entry);
        self.current_mem_usage += frame_mem;
        self.stats.total_frames_buffered += 1;

        // Update stats
        self.update_stats();

        Ok(())
    }

    /// Get frame by sequence number
    pub fn get_frame(&self, frame_number: u64) -> Option<&ProcessedFrame> {
        self.frames
            .iter()
            .find(|entry| entry.frame.frame_number == frame_number)
            .map(|entry| &entry.frame)
    }

    /// Get frame by timestamp (cloest match)
    pub fn get_frame_by_timestamp(&self, timestamp: Duration) -> Option<&ProcessedFrame> {
        self.frames
            .iter()
            .min_by_key(|entry| {
                let diff = if entry.frame.timestamp > timestamp {
                    entry.frame.timestamp - timestamp
                } else {
                    timestamp - entry.frame.timestamp
                };

                diff.as_millis()
            })
            .map(|entry| &entry.frame)
    }

    /// Get the enxt frame after the given frame number
    pub fn get_next_frame(&self, after_frame: u64) -> Option<&ProcessedFrame> {
        self.frames
            .iter()
            .find(|entry| entry.frame.frame_number > after_frame)
            .map(|entry| &entry.frame)
    }

    /// Check if buffer has reached the 60% threshold
    pub fn has_reached_threshold(&self, threshold: f32) -> bool {
        let current_percentage = self.frames.len() as f32 / self.capacity as f32;
        current_percentage >= threshold
    }

    /// Get current buffer status
    pub fn get_status(&self) -> RenderStatus {
        RenderStatus::BufferStatus {
            buffered_frames: self.frames.len() as u64,
            buffer_capacity: self.capacity as u64,
            buffer_percentage: self.frames.len() as f32 / self.capacity as f32,
        }
    }

    /// Get buffer stats
    pub fn get_stats(&self) -> &BufferStats {
        &self.stats
    }

    /// Clear all frames from buffer
    pub fn clear(&mut self) {
        // Clean up GPU textures
        for entry in &self.frames {
            if let Some(texture) = &entry.frame.gpu_texture {
                self.cleanup_gpu_texture(texture);
            }
        }

        self.frames.clear();
        self.current_mem_usage = 0;
        self.stats = BufferStats::default();
    }

    /// Get frame count in buffer
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Get the buffer capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    // Private helper methods

    /// Calculate max memory usage based on capacity
    fn calculate_max_mem(capacity: usize) -> usize {
        // Estimate ~2MB/frame (1920x1080 RGBA = ~8MB, compressed ~2MB)
        capacity * 2 * 1024 * 1024
    }

    /// Calculate memory usage for a frame
    fn calculate_frame_mem(&self, frame: &ProcessedFrame) -> usize {
        let (width, height) = frame.dimensions;
        let pixel_data_size = (width * height * 4) as usize; // RGBA = 4b/pixel

        // Add GPU texture memory if present
        let gpu_mem = if frame.gpu_texture.is_some() {
            pixel_data_size // GPU texture roughly same size
        } else {
            0
        };

        pixel_data_size + gpu_mem
    }

    /// Check if a frame should be dropped to make space
    fn should_drop_frame(&self, incoming_frame_size: usize) -> bool {
        // Drop if buffer is full or mem usage would exceed limit
        self.frames.len() >= self.capacity
            || (self.current_mem_usage + incoming_frame_size) > self.max_mem_usage
    }

    /// Create GPU texture for frame
    async fn create_gpu_texture(&self, frame: &ProcessedFrame) -> Result<wgpu::Texture> {
        let device = self
            .gpu_device
            .as_ref()
            .context("GPU device not initialized")?;

        let (width, height) = frame.dimensions;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("frame_texture_{}", frame.frame_number)),
            size: wgpu::Extent3d {
                width,
                height,
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
        if let Some(queue) = &self.gpu_queue {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.image,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }

        Ok(texture)
    }

    /// Clean up GPU texture resources
    fn cleanup_gpu_texture(&self, _texture: &wgpu::Texture) {
        // Texture cleanup happens automatically in wgpu when dropped
        // This method exists for future manual cleanup if needed.
    }

    /// Update buffer stats
    fn update_stats(&mut self) {
        if !self.frames.is_empty() {
            self.stats.buffer_utilization = self.frames.len() as f32 / self.capacity as f32;
            self.stats.mem_utilization = self.current_mem_usage as f32 / self.max_mem_usage as f32;
            self.stats.avg_frame_size = self.current_mem_usage / self.frames.len();
        }
    }
}

// Thread-safe wrapper for frame buffer
pub type SharedFrameBuffer = Arc<RwLock<FrameBuffer>>;

// Unit tests
#[cfg(test)]
mod tests {
    use super::FrameBuffer;

    #[tokio::test]
    async fn test_frame_buffer_creation() {
        let buffer = FrameBuffer::with_capacity(10).await.unwrap();

        assert_eq!(buffer.capacity(), 10);
    }
}
