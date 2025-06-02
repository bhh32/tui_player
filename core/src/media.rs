mod local;
mod youtube;

pub use local::LocalMediaPlayer;
pub use youtube::{YouTubePlayer, YouTubeConfig, YouTubeVideoInfo, extract_youtube_id, is_youtube_url};

/// Common for all media players
pub trait MediaPlayer {
    /// Get information about the current media
    fn get_media_info(&self) -> Option<crate::MediaInfo>;
    
    /// Get current playback position in seconds
    fn get_position(&self) -> f64;
    
    /// Check if playback is paused
    fn is_paused(&self) -> bool;
    
    /// Stop playback and release resources
    fn stop(&mut self) -> anyhow::Result<()>;
    fn update(&mut self) -> anyhow::Result<()>;
    fn toggle_pause(&mut self);
    fn seek(&mut self, timestamp_secs: f64) -> anyhow::Result<()>;
}
