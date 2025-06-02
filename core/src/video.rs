pub mod decoder;
pub mod frame;

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use image::DynamicImage;

// Initialize FFmpeg only once
pub fn init() -> Result<()> {
    static INIT: std::sync::Once = std::sync::Once::new();
    let mut init_result = Ok(());

    INIT.call_once(|| {
        init_result = ffmpeg::init().context("Failed to initialize ffmpeg");
    });

    init_result
}

/// Respresent a decoded video frame with timestamp information
#[derive(Clone)]
pub struct VideoFrame {
    /// The frame data as an RGBA image
    pub image: DynamicImage,
    /// Presentation timestamp in seconds
    pub timestamp: f64,
    /// Duration of this frame in seconds
    pub duration: f64,
    /// Original frame width
    pub width: u32,
    /// Original frame height
    pub height: u32,
}

impl VideoFrame {
    /// Create a new VideoFrame from an RGBA image and timing information
    pub fn new(image: DynamicImage, timestamp: f64, duration: f64) -> Self {
        let width = image.width();
        let height = image.height();

        Self {
            image,
            timestamp,
            duration,
            width,
            height,
        }
    }

    /// Get frame data as RGBA bytes
    pub fn as_rgba_bytes(&self) -> &[u8] {
        self.image.as_rgba8().unwrap().as_raw()
    }

    /// Resize frame to target dimensions, maintaining aspect ratio if needed
        #[inline]
        pub fn resize(
            &self,
            target_width: u32,
            target_height: u32,
            maintain_aspect_ratio: bool,
        ) -> Self {
            // If dimensions already match, avoid resizing
            if self.width == target_width && self.height == target_height {
                return self.clone();
            }
        
            // Calculate actual dimensions accounting for aspect ratio
            let (new_width, new_height) = if maintain_aspect_ratio {
                let ratio = self.width as f32 / self.height as f32;

                // Determine dimensions that fit within target while maintaining aspect ratio
                if target_width as f32 / target_height as f32 > ratio {
                    // Height is the limiting factor
                    let new_height = target_height;
                    let new_width = (new_height as f32 * ratio) as u32;
                    (new_width, new_height)
                } else {
                    // Width is the limiting factor
                    let new_width = target_width;
                    let new_height = (new_width as f32 / ratio) as u32;
                    (new_width, new_height)
                }
            } else {
                (target_width, target_height)
            };
        
            // Choose filter type based on the scaling factor
            let scale_factor = (new_width as f32 / self.width as f32)
                .max(new_height as f32 / self.height as f32);
            
            let filter = if scale_factor < 0.7 || scale_factor > 1.5 {
                // For significant downscaling or upscaling, use higher quality
                image::imageops::FilterType::Triangle
            } else {
                // For minor resizing, use fastest algorithm
                image::imageops::FilterType::Nearest
            };

            let resized_image = self.image.resize_exact(new_width, new_height, filter);
            VideoFrame::new(resized_image, self.timestamp, self.duration)
        }

    /// Create a new resized VideoFrame from pre-processed RGBA data
    #[inline]
    pub fn resize_from_data(&self, width: u32, height: u32, data: Vec<u8>) -> Self {
        // Create the image directly from the raw data without additional allocations
        let rgba_image = image::RgbaImage::from_raw(width, height, data)
            .expect("Invalid image data dimensions");
        let dynamic_image = image::DynamicImage::ImageRgba8(rgba_image);
        
        VideoFrame::new(dynamic_image, self.timestamp, self.duration)
    }
    
    /// Check if the frame needs to be resized
    pub fn needs_resize(&self, target_width: u32, target_height: u32) -> bool {
        self.width != target_width || self.height != target_height
    }
    
    /// Get maximum dimension of the frame
    #[inline]
    pub fn max_dimension(&self) -> u32 {
        self.width.max(self.height)
    }
    
    /// Fast downscale for thumbnail or preview
    #[inline]
    pub fn fast_thumbnail(&self, max_dimension: u32) -> Self {
        // If already small enough, return as is
        if self.max_dimension() <= max_dimension {
            return self.clone();
        }
        
        // Calculate scaled dimensions keeping aspect ratio
        let ratio = self.width as f32 / self.height as f32;
        let (width, height) = if self.width > self.height {
            (max_dimension, (max_dimension as f32 / ratio).floor() as u32)
        } else {
            ((max_dimension as f32 * ratio).floor() as u32, max_dimension)
        };
        
        // Use nearest-neighbor for maximum speed
        let thumbnail = self.image.thumbnail(width, height);
        VideoFrame::new(thumbnail, self.timestamp, self.duration)
    }
}

/// Media information about a video file
#[derive(Clone)]
pub struct MediaInfo {
    pub duration: f64,               // Total duration in seconds
    pub width: u32,                  // Video width in pixels
    pub height: u32,                 // Video height in pixels
    pub frame_rate: f64,             // Frames per second
    pub format_name: String,         // Format name (e.g.; "mp4, "mkv")
    pub video_codec: String,         // Video codec name
    pub audio_codec: Option<String>, // Audio codec name (if audio is present)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn test_video_frame_resize_maintain_aspect() {
        // Create a 200x100 test image (2:1 aspect ratio)
        let test_image =
            ImageBuffer::<Rgba<u8>, Vec<u8>>::from_fn(200, 100, |_, _| Rgba([255, 0, 0, 255]));
        let frame = VideoFrame::new(DynamicImage::ImageRgba8(test_image), 0., 0.04);

        // Resize to 50x50 target with maintain_aspect = true
        let resized = frame.resize(50, 50, true);

        // Should be 50x25 to maintain the 2:1 aspect ratio
        assert_eq!(resized.width, 50);
        assert_eq!(resized.height, 25);
    }

    #[test]
    fn test_video_frame_resize_ignore_aspect() {
        // Create a 200x100 test image
        let test_image =
            ImageBuffer::<Rgba<u8>, Vec<u8>>::from_fn(200, 100, |_, _| Rgba([255, 0, 0, 255]));
        let frame = VideoFrame::new(DynamicImage::ImageRgba8(test_image), 0., 0.04);

        // Resize to 50x50 target with maintain_aspect = false
        let resized = frame.resize(50, 50, false);

        // Should be exactly 50x50
        assert_eq!(resized.width, 50);
        assert_eq!(resized.height, 50);
    }
}
