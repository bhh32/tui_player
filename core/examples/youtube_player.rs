use anyhow::{Context, Result};
use std::{env, io::Write, thread, time::Duration};

use core::{
    detect_media_type, MediaSourceType, YouTubePlayer, YouTubeConfig, MediaPlayer,
    render::{RenderConfig, RenderMethod},
};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

fn main() -> Result<()> {
    // Set up logging
    env_logger::init();

    // Get YouTube URL or video ID from command line
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <youtube_url_or_id>", args[0]);
        return Ok(());
    }

    let url_or_id = &args[1];
    
    // Check if yt-dlp is installed
    if let Err(_status) = std::process::Command::new("yt-dlp")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .status()
    {
        eprintln!("Error: yt-dlp is not installed or not in your PATH");
        eprintln!("\nPlease install yt-dlp first:");
        eprintln!("\nFor Debian/Ubuntu:");
        eprintln!("    sudo apt-get install yt-dlp");
        eprintln!("    # If not available in your repository:");
        eprintln!("    sudo curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o /usr/local/bin/yt-dlp");
        eprintln!("    sudo chmod a+rx /usr/local/bin/yt-dlp");
        eprintln!("\nFor macOS:");
        eprintln!("    brew install yt-dlp");
        eprintln!("\nFor other systems:");
        eprintln!("    pip install yt-dlp");
        return Ok(());
    }
    
    // Detect media type
    let media_type = detect_media_type(url_or_id);
    if media_type != MediaSourceType::YouTube && url_or_id.len() != 11 {
        eprintln!("Error: Not a valid YouTube URL or ID: {}", url_or_id);
        eprintln!("Please provide a YouTube URL (e.g., https://www.youtube.com/watch?v=dQw4w9WgXcQ)");
        eprintln!("Or a YouTube video ID (e.g., dQw4w9WgXcQ)");
        return Ok(());
    }

    println!("Initializing YouTube player with yt-dlp for: {}", url_or_id);

    // Set up terminal
    terminal::enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;

    // Configure the YouTube player with yt-dlp
    let youtube_config = YouTubeConfig {
        quality: 1,                         // 0 is best, higher numbers are lower quality
        format: Some("mp4".to_string()),    // Prefer MP4 format
        max_resolution: Some("720p".to_string()), // Limit resolution for better performance
        ytdlp_path: None,                   // Auto-detect yt-dlp path
        ..Default::default()
    };

    // Configure the renderer
    let render_config = RenderConfig {
        method: RenderMethod::Auto,         // Auto-detect best rendering method
        width: Some(120),                   // Adjust width as needed
        height: Some(60),                   // Adjust height as needed
        maintain_aspect: true,
        x: 0,
        y: 2,                              // Leave space for the status line
        adaptive_resolution: true,         // Enable adaptive resolution
        quality: 0.8,                      // Start with good quality
        target_fps: 30.0,                  // Target 30 FPS
        enable_threading: true,
        max_frame_dimension: Some(1024),
        enable_gpu: true,                  // Use GPU acceleration if available
    };

    // Create a YouTube player
    let mut player = match YouTubePlayer::new(url_or_id, Some(render_config), Some(youtube_config)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to create YouTube player: {}", e);
            eprintln!("\nPossible solutions:");
            eprintln!("1. Check your internet connection");
            eprintln!("2. Make sure yt-dlp is properly installed");
            eprintln!("3. Try updating yt-dlp: yt-dlp -U");
            return Err(e);
        }
    };

    // Clear the screen initially
    print!("\x1B[2J\x1B[1;1H");
    println!("Loading YouTube video using yt-dlp... Please wait...");
    std::io::stdout().flush()?;

    // Initialize the player (this fetches video info and prepares the decoder)
    if let Err(e) = player.initialize() {
        // Clear screen to show error clearly
        print!("\x1B[2J\x1B[1;1H");
        
        eprintln!("Error initializing YouTube player:");
        eprintln!("{:?}", e);
        eprintln!("\nPossible solutions:");
        eprintln!("1. The video might be age-restricted or private");
        eprintln!("2. The video might be blocked in your region");
        eprintln!("3. Make sure yt-dlp is installed and up to date:");
        eprintln!("   - Update yt-dlp: yt-dlp -U");
        eprintln!("4. Check your internet connection");
        return Err(e.context("Failed to initialize YouTube player"));
    }

    // Get video info
    let media_info = player.get_media_info()
        .context("Failed to get media info")?;
    
    let youtube_info = player.get_youtube_info()
        .context("Failed to get YouTube info")?;

    println!("\x1B[2J\x1B[1;1H"); // Clear screen again
    println!("YouTube Video: {}", youtube_info.title);
    println!("Duration: {:.2} seconds", media_info.duration);
    println!("Resolution: {}x{}", media_info.width, media_info.height);
    println!("Codec: {}", media_info.video_codec);
    if let Some(uploader) = &youtube_info.uploader {
        println!("Uploader: {}", uploader);
    }
    if let Some(view_count) = youtube_info.view_count {
        println!("Views: {}", view_count);
    }
    println!("\nPress 'q' to quit, space to pause/resume, left/right arrows to seek");
    std::io::stdout().flush()?;

    // Playback loop
    let mut last_status_update = std::time::Instant::now();
    let frame_duration = Duration::from_secs_f64(1.0 / media_info.frame_rate);

    loop {
        // Update player state (decode and render next frame if needed)
        if let Err(e) = player.update() {
            // Print error but continue playback
            print!("\x1B[1;1HError: {}", e);
            std::io::stdout().flush()?;
        }

        // Update status line every 100ms
        if last_status_update.elapsed() >= Duration::from_millis(100) {
            print!("\x1B[1;1HPosition: {:.2}/{:.2}s | FPS: {:.1} | {} | Press 'q' to quit, space to pause/resume, left/right arrows to seek",
                player.get_position(),
                media_info.duration,
                1.0 / frame_duration.as_secs_f64(),
                if player.is_paused() { "PAUSED" } else { "PLAYING" }
            );
            std::io::stdout().flush()?;
            last_status_update = std::time::Instant::now();
        }

        // Check for key events with short timeout
        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(' ') => {
                        player.toggle_pause();
                    }
                    KeyCode::Left => {
                        let new_pos = (player.get_position() - 5.0).max(0.0);
                        if let Err(e) = player.seek(new_pos) {
                            print!("\x1B[1;1HSeek error: {}", e);
                            std::io::stdout().flush()?;
                        }
                    }
                    KeyCode::Right => {
                        let new_pos = (player.get_position() + 5.0).min(media_info.duration);
                        if let Err(e) = player.seek(new_pos) {
                            print!("\x1B[1;1HSeek error: {}", e);
                            std::io::stdout().flush()?;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Sleep a tiny bit to avoid using 100% CPU
        thread::sleep(Duration::from_millis(1));
    }

    // Clean up with better error handling
    if let Err(e) = terminal::disable_raw_mode() {
        eprintln!("Warning: Failed to disable raw mode: {}", e);
    }
    
    if let Err(e) = execute!(std::io::stdout(), LeaveAlternateScreen) {
        eprintln!("Warning: Failed to leave alternate screen: {}", e);
    }

    println!("YouTube playback ended. Thanks for using yt-dlp TUI player!");
    Ok(())
}