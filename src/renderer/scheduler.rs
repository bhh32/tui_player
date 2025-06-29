//! Playback scheduler and timing coordinator

use crate::renderer::types::*;
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Playback scheduler for timing and coordination
#[derive(Clone)]
pub struct PlaybackScheduler {
    config: TimingConfig,
    status: Arc<RwLock<PlaybackStatus>>,
}

/// Current playback status
#[derive(Debug, Clone)]
pub struct PlaybackStatus {
    pub is_playing: bool,
    pub current_frame: u64,
    pub current_timestamp: std::time::Duration,
    pub playback_speed: f32,
}

impl Default for PlaybackStatus {
    fn default() -> Self {
        Self {
            is_playing: false,
            current_frame: 0,
            current_timestamp: std::time::Duration::ZERO,
            playback_speed: 1.0,
        }
    }
}

impl PlaybackScheduler {
    /// Create new playback scheduler
    pub fn new(config: TimingConfig) -> Self {
        Self {
            config,
            status: Arc::new(RwLock::new(PlaybackStatus::default())),
        }
    }

    /// Main scheduler loop - runs in dedicated thread
    pub async fn run_scheduler_loop(
        &mut self,
        _frame_buffer: Arc<RwLock<crate::renderer::frame_buffer::FrameBuffer>>,
        _cmd_rx: Receiver<PlaybackCommand>,
        _status_tx: Sender<RenderStatus>,
    ) -> Result<()> {
        // TODO: Implement actual scheduling logic
        log::info!("Scheduler loop started");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Ok(())
    }

    /// Get current playback status
    pub async fn get_status(&self) -> PlaybackStatus {
        self.status.read().await.clone()
    }

    /// Update playback status
    async fn update_status<F>(&self, updater: F)
    where
        F: FnOnce(&mut PlaybackStatus),
    {
        let mut status = self.status.write().await;
        updater(&mut *status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_creation() {
        let config = TimingConfig::default();
        let _scheduler = PlaybackScheduler::new(config);
    }

    #[tokio::test]
    async fn test_scheduler_status() {
        let config = TimingConfig::default();
        let scheduler = PlaybackScheduler::new(config);

        let status = scheduler.get_status().await;
        assert!(!status.is_playing);
        assert_eq!(status.current_frame, 0);
    }
}
