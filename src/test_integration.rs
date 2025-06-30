use crate::renderer::{RendererConfig, VideoRenderer};
use anyhow::Result;
use std::{path::Path, time::Duration};
use tokio::time::sleep;

/// Test the video renderer with a real video file
pub async fn test_video_file(video_path: &str) -> Result<()> {
    println!("🎬 Testing video renderer with: {video_path}");

    // Verify file exists
    if !Path::new(video_path).exists() {
        return Err(anyhow::anyhow!("Video file not found: {video_path}"));
    }

    // Create renderer config
    let config = RendererConfig::default();
    println!(
        "📋 Using config: buffer_capacity={}, target_fps={}",
        config.buffer_capacity, config.timing_config.target_fps
    );

    // Init renderer
    println!("🔧 Initializing video renderer...");
    let mut renderer = VideoRenderer::new(config).await?;

    // Start processing
    println!("🚀 Starting video processing pipeline...");
    renderer.start(video_path).await?;

    // Let it run for a few seconds to test
    println!("⏳ Processing video for 10 seconds...");
    sleep(Duration::from_secs(10)).await;

    // Send play command
    println!("\u{25B6} Sending play command...");
    renderer.command(crate::renderer::types::PlaybackCommand::Play)?;

    // Run for another 10 seconds
    println!("🎥 Playing for 15 seconds...");
    sleep(Duration::from_secs(15)).await;

    // Test pause
    println!("\u{23F8} Pausing playback...");
    renderer.command(crate::renderer::types::PlaybackCommand::Pause)?;
    sleep(Duration::from_secs(3)).await;

    // Test resume
    println!("\u{25B6} Resuming playback...");
    renderer.command(crate::renderer::types::PlaybackCommand::Play)?;

    // Test stop
    println!("\u{23F9} Stopping playback...");
    renderer.command(crate::renderer::types::PlaybackCommand::Stop)?;

    println!("✅ Test completed successfully!");
    Ok(())
}

/// Create a simple test video if none exists
pub async fn create_test_video() -> Result<String> {
    let test_dir = "test_videos";
    std::fs::create_dir_all(test_dir)?;

    let test_video_path = format!("{test_dir}/test.mp4");

    if !Path::new(&test_video_path).exists() {
        println!("📁 Test video directory created: {test_dir}");
        println!("💡 Please place test video file at: {test_video_path}");
        println!("You can use any .mp4, .avi, .mkv, or other video file");
        println!("For testing, a short (30-60 second) video works best");
        return Err(anyhow::anyhow!(
            "No test file found. Please place a video file at: {test_video_path}"
        ));
    }

    Ok(test_video_path)
}

/// Run basic renderer component tests
pub async fn test_renderer_components() -> Result<()> {
    println!("🧪 Testing renderer components...");

    // Test config
    let config = RendererConfig::default();
    println!("✅ Config creation: OK");

    // Test renderer creation (without starting)
    match VideoRenderer::new(config).await {
        Ok(_) => println!("✅ Renderer creation: OK"),
        Err(e) => {
            eprintln!("x Renderer creation failed: {e}");
            return Err(e);
        }
    }

    println!("✅ All component tests passed!");
    Ok(())
}
