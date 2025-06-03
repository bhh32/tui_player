mod local;
mod youtube;

pub use local::LocalMediaPlayer;
pub use youtube::{YouTubePlayer, YouTubeConfig, YouTubeVideoInfo, extract_youtube_id, is_youtube_url};
use std::any::Any;

/// Common for all media players
pub trait MediaPlayer {
    /// Get information about the current media
    fn get_media_info(&self) -> Option<crate::MediaInfo>;
    
    /// Get current playback position in seconds
    fn get_position(&self) -> f64;
    
    /// Check if playback is paused
    fn is_paused(&self) -> bool;
    
    /// Get current volume (0-100)
    fn get_volume(&self) -> i32 {
        50 // Default implementation returns 50%
    }
    
    /// Check if audio is muted
    fn is_muted(&self) -> bool {
        false // Default implementation returns not muted
    }
    
    /// Set volume (0-100)
    fn set_volume(&mut self, _volume: i32) -> anyhow::Result<()> {
        Ok(()) // Default implementation does nothing
    }
    
    /// Toggle mute state
    fn toggle_mute(&mut self) -> anyhow::Result<()> {
        Ok(()) // Default implementation does nothing
    }
    
    /// Stop playback and release resources
    fn stop(&mut self) -> anyhow::Result<()>;
    fn update(&mut self) -> anyhow::Result<()>;
    fn toggle_pause(&mut self);
    fn seek(&mut self, timestamp_secs: f64) -> anyhow::Result<()>;
    
    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn Any;
}
