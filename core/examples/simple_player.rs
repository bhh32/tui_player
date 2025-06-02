use anyhow::{Result, Context};
use std::io::Write;
use core::{
    VideoDecoder,
    render::{RenderConfig, RenderMethod},
};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{env, time::Duration};

fn main() -> Result<()> {
    // Set up logging
    env_logger::init();

    // Get video path from command line
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <video_file>", args[0]);
        return Ok(());
    }

    let video_path = &args[1];
    println!("Opening video: {}", video_path);

    // Set up terminal
    terminal::enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;

    // Create video decoder
    let mut decoder = VideoDecoder::new(video_path)?;
    let info = decoder.get_media_info();

    println!("Video info:");
    println!("  Duration: {:.2} seconds", info.duration);
    println!("  Size: {}x{}", info.width, info.height);
    println!("  Format: {}", info.format_name);
    println!("  Video codec: {}", info.video_codec);
    println!("  Audio codec: {:?}", info.audio_codec);
    println!("\nPress 'q' to quit, space to pause/resume, left/right arrows to seek");

    // Create renderer
    let config = RenderConfig {
        method: RenderMethod::Blocks, // Use blocks for best compatibility
        width: Some(80),              // Use smaller width for better compatibility
        height: Some(40),             // Use smaller height for better compatibility
        maintain_aspect: true,
        x: 0,
        y: 2, // Leave space for the status line
        adaptive_resolution: true,    // Enable adaptive resolution
        quality: 0.8,                 // Start with good quality
        target_fps: 30.0,             // Target 30 FPS
        enable_threading: true,       // Enable multi-threaded processing
        max_frame_dimension: Some(1024), // Limit max frame size for performance
        enable_gpu: true,            // Enable GPU acceleration for performance
    };

    let mut renderer = core::render::TerminalRenderer::new(config)
        .context("Failed to create terminal renderer")?;

    // Clear the screen initially
    print!("\x1B[2J\x1B[1;1H");
    std::io::stdout().flush().context("Failed to flush stdout")?;

    // Playback loop
    let frame_duration = Duration::from_secs_f64(1.0 / info.frame_rate);
    let mut paused = false;

    loop {
        // Check for key events
        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(' ') => paused = !paused,
                    KeyCode::Left => {
                        let new_pos = (decoder.get_media_info().duration * 0.1).min(info.duration);
                        decoder.seek(new_pos)?;
                    }
                    KeyCode::Right => {
                        let new_pos = (decoder.get_media_info().duration * 0.9).min(info.duration);
                        decoder.seek(new_pos)?;
                    }
                    _ => {}
                }
            }
        }

        // If not paused, decode and render the next frame
        if !paused {
            if let Some(frame) = decoder.decode_next_frame()? {
                // Move cursor to top of the screen to show status without interfering with rendering
                print!("\x1B[1;1H");
                print!(
                    "Frame: {:.2}/{:.2}s | Press 'q' to quit, space to pause/resume, arrow keys to seek",
                    frame.timestamp, info.duration
                );

                // Try to render the frame with better error handling
                match renderer.render(&frame) {
                    Ok(_) => {
                        // Sleep to maintain frame rate
                        std::thread::sleep(frame_duration);
                    },
                    Err(e) => {
                        // Move cursor to top of screen to show error
                        print!("\x1B[1;1H");
                        print!("Error: {}", e);
                        std::io::stdout().flush()?;
                        
                        // Sleep a bit longer on error to avoid error spam
                        std::thread::sleep(frame_duration * 2);
                        
                        // Try to continue with next frame
                        continue;
                    }
                }
            } else {
                // End of video
                terminal::disable_raw_mode()?;
                execute!(std::io::stdout(), LeaveAlternateScreen)?;

                println!("\nEnd of video reached.");
                break;
            }
        }
    }

    // Clean up with better error handling
    if let Err(e) = terminal::disable_raw_mode() {
        eprintln!("Warning: Failed to disable raw mode: {}", e);
    }
    
    if let Err(e) = execute!(std::io::stdout(), LeaveAlternateScreen) {
        eprintln!("Warning: Failed to leave alternate screen: {}", e);
    }

    Ok(())
}
