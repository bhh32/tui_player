pub mod config;
pub mod media;
pub mod render;
pub mod video;

use std::path::Path;
use anyhow::{Result, anyhow};

// Re-exports
pub use video::{MediaInfo, VideoFrame, decoder::VideoDecoder};
pub use media::{MediaPlayer, LocalMediaPlayer, YouTubePlayer, YouTubeConfig, YouTubeVideoInfo};

/// Type of media source
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaSourceType {
    /// Local file on disk
    LocalFile,
    /// YouTube video
    YouTube,
    /// Unsupported media type
    Unsupported,
}

/// Detect media type from URL or file path
pub fn detect_media_type(url_or_path: &str) -> MediaSourceType {
    // Check if it's a YouTube URL
    if media::is_youtube_url(url_or_path) {
        return MediaSourceType::YouTube;
    }
    
    // Check if it's a local file
    if Path::new(url_or_path).exists() {
        return MediaSourceType::LocalFile;
    }
    
    // Couldn't determine the media type
    MediaSourceType::Unsupported
}

/// Create appropriate media player based on URL or file path
pub fn create_media_player(
    url_or_path: &str, 
    render_config: Option<render::RenderConfig>
) -> Result<Box<dyn MediaPlayer>> {
    match detect_media_type(url_or_path) {
        MediaSourceType::LocalFile => {
            let player = LocalMediaPlayer::new(url_or_path, render_config)?;
            Ok(Box::new(player))
        },
        MediaSourceType::YouTube => {
            let player = YouTubePlayer::new(url_or_path, render_config, None)?;
            Ok(Box::new(player))
        },
        MediaSourceType::Unsupported => {
            Err(anyhow!("Unsupported media type: {}", url_or_path))
        }
    }
}
