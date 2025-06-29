pub mod frame_buffer;
pub mod gpu_processor;
pub mod kitty_display;
pub mod scheduler;
pub mod types;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use std::sync::Arc;
use tokio::sync::RwLock;

pub use types::*;

/// Main video renderer coordinating all components
pub struct VideoRenderer {
    /// Frame buffer for pre-processed frames (60% buffering strategy)
    frame_buffer: Arc<RwLock<frame_buffer::FrameBuffer>>,
    /// GPU processing pipeline for frame conversion
    gpu_processor: gpu_processor::GpuProcessor,
    /// Kitty protocol display handler
    kitty_display: kitty_display::KittyDisplay,
    /// Playback timing and coordination
    scheduler: scheduler::PlaybackScheduler,
    /// Communication channels between threads
    channels: RendererChannels,
}

/// Inter-thread communication channels
struct RendererChannels {
    frame_tx: Sender<ProcessedFrame>,
    frame_rx: Receiver<ProcessedFrame>,

    cmd_tx: Sender<PlaybackCommand>,
    cmd_rx: Receiver<PlaybackCommand>,

    status_tx: Sender<RenderStatus>,
    status_rx: Receiver<RenderStatus>,
}

impl VideoRenderer {
    /// Create new renderer with specified configuration
    pub async fn new(config: RendererConfig) -> Result<Self> {
        // Initialize communication channels
        let channels = Self::create_channels();

        // Initialize core components
        let frame_buffer = Arc::new(RwLock::new(
            frame_buffer::FrameBuffer::with_capacity(config.buffer_capacity).await?,
        ));

        let gpu_processor = gpu_processor::GpuProcessor::new(config.gpu_config).await?;
        let kitty_display = kitty_display::KittyDisplay::new(config.display_config)?;
        let scheduler = scheduler::PlaybackScheduler::new(config.timing_config);

        Ok(Self {
            frame_buffer,
            gpu_processor,
            kitty_display,
            scheduler,
            channels,
        })
    }

    /// Start the three-thread processing pipeline
    pub async fn start(&mut self, video_path: &str) -> Result<()> {
        /*
            Spawn the three main threads:
            1. GPU processing thread (decode + convert frames)
            2. Display thread (Kitty protocol output)
            3. Scheduler thread (timing + buffering logic)
        */

        self.spawn_gpu_thread(video_path).await?;
        self.spawn_display_thread().await?;
        self.spawn_scheduler_thread().await?;

        Ok(())
    }

    /// Send playback command (play/pause/seek/stop)
    pub fn command(&self, cmd: PlaybackCommand) -> Result<()> {
        self.channels.cmd_tx.send(cmd)?;
        Ok(())
    }

    // Private helper methods
    fn create_channels() -> RendererChannels {
        let (frame_tx, frame_rx) = crossbeam_channel::unbounded();
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (status_tx, status_rx) = crossbeam_channel::unbounded();

        RendererChannels {
            frame_tx,
            frame_rx,

            cmd_tx,
            cmd_rx,
            status_tx,
            status_rx,
        }
    }

    async fn spawn_gpu_thread(&self, path: &str) -> Result<()> {
        let frame_buffer = Arc::clone(&self.frame_buffer);
        let mut gpu_processor = self.gpu_processor.clone();
        let frame_tx = self.channels.frame_tx.clone();
        let cmd_rx = self.channels.cmd_rx.clone();
        let status_tx = self.channels.status_tx.clone();
        let video_path = path.to_string();

        std::thread::spawn(move || {
            // Create a new tokio runtime for this thread
            let rt = tokio::runtime::Runtime::new().unwrap();

            rt.block_on(async move {
                match gpu_processor
                    .process_video(&video_path, frame_buffer, frame_tx, cmd_rx, status_tx)
                    .await
                {
                    Ok(_) => log::info!("GPU processing thread completed successfully"),
                    Err(e) => log::error!("GPU processing thread error: {e}"),
                }
            });
        });

        Ok(())
    }

    async fn spawn_display_thread(&self) -> Result<()> {
        let mut kitty_display = self.kitty_display.clone();
        let frame_rx = self.channels.frame_rx.clone();
        let cmd_rx = self.channels.cmd_rx.clone();
        let status_tx = self.channels.status_tx.clone();

        tokio::spawn(async move {
            // Display loop
            match kitty_display
                .run_display_loop(frame_rx, cmd_rx, status_tx)
                .await
            {
                Ok(_) => log::info!("Display thread completed"),
                Err(e) => log::error!("Display thread error: {e}"),
            }
        });

        Ok(())
    }

    async fn spawn_scheduler_thread(&self) -> Result<()> {
        let mut scheduler = self.scheduler.clone();
        let frame_buffer = Arc::clone(&self.frame_buffer);
        let cmd_rx = self.channels.cmd_rx.clone();
        let status_tx = self.channels.status_tx.clone();

        tokio::spawn(async move {
            // Scheduler loop - manages 60% buffering logic and playback timing
            match scheduler
                .run_scheduler_loop(frame_buffer, cmd_rx, status_tx)
                .await
            {
                Ok(_) => log::info!("Scheduler thread completed successfully"),
                Err(e) => log::error!("Scheduler thread error: {e}"),
            }
        });

        Ok(())
    }
}
