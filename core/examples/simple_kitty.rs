use anyhow::Result;
use core::{
    VideoDecoder, VideoFrame,
    render::{RenderConfig, RenderMethod},
};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    collections::VecDeque,
    env,
    io::Write,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

// Our frame structures
struct DecodedFrame {
    frame: Arc<VideoFrame>,
    timestamp: f64,
}

struct PreparedFrame {
    image_path: Arc<std::path::PathBuf>,
    timestamp: f64,
}

// Control messages for inter-thread communication
enum PlayerControl {
    Play,      // Resume playback
    Pause,     // Pause playback
    Seek(f64), // Seek to specific time in seconds
    Stop,      // Stop playback completely
}

// Thread-safe queue using VecDeque
struct ThreadSafeQueue<T> {
    items: Mutex<VecDeque<T>>,
    max_size: usize,
}

impl<T> ThreadSafeQueue<T> {
    fn new(max_size: usize) -> Self {
        Self {
            items: Mutex::new(VecDeque::with_capacity(max_size)),
            max_size,
        }
    }

    fn push(&self, item: T) -> bool {
        let mut queue = self.items.lock().unwrap();
        if queue.len() >= self.max_size {
            false // Queue is full
        } else {
            queue.push_back(item);
            true // Successfully added
        }
    }

    fn pop(&self) -> Option<T> {
        let mut queue = self.items.lock().unwrap();
        queue.pop_front()
    }

    fn len(&self) -> usize {
        let queue = self.items.lock().unwrap();
        queue.len()
    }

    fn clear(&self) {
        let mut queue = self.items.lock().unwrap();
        queue.clear();
    }

    fn is_full(&self) -> bool {
        let queue = self.items.lock().unwrap();
        queue.len() >= self.max_size
    }

    // Method removed - functionality covered by is_full()
}

fn main() -> Result<()> {
    // Set up logging with more verbose output
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Log library version and platform info
    log::info!("Starting simple_kitty example");
    log::info!("OS: {}", std::env::consts::OS);
    log::info!(
        "Detected terminal type: {}",
        std::env::var("TERM").unwrap_or_else(|_| "unknown".to_string())
    );

    // Check if running in Kitty terminal
    if let Ok(kitty_id) = std::env::var("KITTY_WINDOW_ID") {
        log::info!("Running in Kitty terminal (ID: {})", kitty_id);
        log::info!("GPU acceleration disabled for compatibility with Kitty terminal");
    } else {
        log::warn!("Not running in Kitty terminal - graphics protocol may not work properly");
    }

    // Get video path from command line
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <video_file>", args[0]);
        return Ok(());
    }

    let video_path = &args[1];
    println!("Opening video: {}", video_path);

    // Verify file exists
    if !std::path::Path::new(video_path).exists() {
        return Err(anyhow::anyhow!("Video file not found: {}", video_path));
    }

    // Set up terminal
    terminal::enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;

    // Create video decoder to get initial info
    let decoder = VideoDecoder::new(video_path)?;
    let info = decoder.get_media_info();

    println!("Video info:");
    println!("  Duration: {:.2} seconds", info.duration);
    println!("  Size: {}x{}", info.width, info.height);
    println!("  Format: {}", info.format_name);
    println!("  Video codec: {}", info.video_codec);
    println!("  Audio codec: {:?}", info.audio_codec);
    println!("\nPress 'q' to quit, space to pause/resume, left/right arrows to seek");

    // Calculate appropriate size
    let terminal_width = terminal::size()?.0 as usize;
    let terminal_height = terminal::size()?.1 as usize;

    // Use a smaller scaling factor for better performance
    let scaling_factor = 10;

    // Try to auto-detect the best rendering method
    let detected_method = core::render::TerminalRenderer::detect_best_method();
    log::info!("Detected render method: {:?}", detected_method);

    // Force Kitty protocol if specified, otherwise use auto-detected method
    let render_method = if std::env::var("FORCE_KITTY").is_ok() {
        log::info!("Forcing Kitty protocol");
        RenderMethod::Kitty
    } else {
        detected_method
    };

    // Create a renderer with adaptive resolution
    let config_width = Some((terminal_width * scaling_factor) as u32);
    let config_height = Some((terminal_height * scaling_factor) as u32);
    let config_x = 0;
    let config_y = 2; // Leave space for the status line

    let config = RenderConfig {
        method: render_method,
        width: config_width,
        height: config_height,
        maintain_aspect: true,
        x: config_x,
        y: config_y,
        adaptive_resolution: true,       // Enable adaptive resolution
        quality: 0.8,                    // Start with good quality
        target_fps: 30.0,                // Target 30 FPS
        enable_threading: true,          // Enable multi-threaded processing
        max_frame_dimension: Some(1024), // Limit max frame size for performance
        enable_gpu: false, // Disable GPU acceleration due to alignment issues in Kitty
    };

    // Store these values for display
    let display_x = config_x;
    let display_y = config_y;

    // Create the renderer with error handling
    let _renderer = match core::render::TerminalRenderer::new(config) {
        Ok(r) => r,
        Err(e) => {
            log::error!("Failed to create renderer: {:?}", e);
            // Try with fallback method if Kitty failed
            if render_method == RenderMethod::Kitty {
                log::info!("Trying fallback to Blocks rendering");
                let fallback_config = RenderConfig {
                    method: RenderMethod::Blocks,
                    width: config_width,
                    height: config_height,
                    maintain_aspect: true,
                    x: config_x,
                    y: config_y,
                    adaptive_resolution: true,
                    quality: 0.8,
                    target_fps: 30.0,
                    enable_threading: true,
                    max_frame_dimension: Some(1024),
                    enable_gpu: false,
                };
                core::render::TerminalRenderer::new(fallback_config)?
            } else {
                return Err(e);
            }
        }
    };

    // Clear the screen initially
    println!("\x1B[2J\x1B[1;1H");

    // Playback variables
    let _frame_duration = Duration::from_secs_f64(1.0 / info.frame_rate);
    let paused = Arc::new(Mutex::new(false));
    let mut current_position = 0.0;

    // Create shared queues
    let decode_queue_size = 10;
    let display_queue_size = 25;

    // Create Arc queues
    let decode_queue = Arc::new(ThreadSafeQueue::<DecodedFrame>::new(decode_queue_size));
    let display_queue = Arc::new(ThreadSafeQueue::<PreparedFrame>::new(display_queue_size));

    // IMPORTANT FIX: Create explicit clones for each thread to avoid ownership issues
    let decoder_thread_queue = decode_queue.clone();
    let prepare_thread_decode_queue = decode_queue.clone();
    let prepare_thread_display_queue = display_queue.clone();
    let main_thread_display_queue = display_queue.clone();
    let main_thread_decode_queue = decode_queue.clone();

    // Create a shared running state for the threads
    let running = Arc::new(Mutex::new(true));
    let running_decoder = running.clone();
    let running_prepare = running.clone();

    // Control channel for inter-thread communication
    let (control_sender, control_receiver) = mpsc::channel::<PlayerControl>();

    // Create temp directory for prepared frames
    let temp_dir = std::env::temp_dir().join("kitty_frames");
    std::fs::create_dir_all(&temp_dir)?;

    // Clone necessary variables for threads
    let paused_clone = paused.clone();
    let decoder_video_path = video_path.to_string();
    let temp_dir_clone = temp_dir.clone();
    // Control channel is passed directly to the decoder thread

    // ========== DECODER THREAD ==========
    let decoder_thread = thread::spawn(move || {
        println!("\x1B[3;1HDecoder thread starting...");
        std::io::stdout().flush().unwrap();

        let mut decoder = match VideoDecoder::new(&decoder_video_path) {
            Ok(d) => d,
            Err(e) => {
                println!("\x1B[4;1HFailed to create decoder: {}", e);
                return;
            }
        };

        let mut frames_decoded = 0;

        // Main decoder loop
        while *running_decoder.lock().unwrap() {
            // Check if we should pause
            let mut is_paused = *paused_clone.lock().unwrap();
            if is_paused {
                // While paused, only check for control messages
                thread::sleep(Duration::from_millis(50));
                
                // Still check for control messages while paused
                match control_receiver.try_recv() {
                    Ok(PlayerControl::Play) => {
                        println!("\x1B[6;1HDecoder thread: resuming playback");
                        let mut paused_guard = paused_clone.lock().unwrap();
                        *paused_guard = false;
                        is_paused = false;
                    },
                    Ok(PlayerControl::Seek(pos)) => {
                        println!("\x1B[6;1HDecoder seeking to {:.2}s", pos);
                        if let Err(e) = decoder.seek(pos) {
                            println!("\x1B[7;1HSeek failed: {}", e);
                        } else {
                            // Clear the queues on seek
                            decoder_thread_queue.clear();
                        }
                    },
                    Ok(PlayerControl::Stop) => {
                        println!("\x1B[6;1HDecoder thread: stopping");
                        let mut running_state = running_decoder.lock().unwrap();
                        *running_state = false;
                    },
                    Err(mpsc::TryRecvError::Empty) => {
                        // No control message while paused, continue waiting
                    },
                    Err(mpsc::TryRecvError::Disconnected) => {
                        println!("\x1B[6;1HDecoder thread: control channel disconnected");
                        let mut running_state = running_decoder.lock().unwrap();
                        *running_state = false;
                    },
                    _ => {}
                }
                
                // Skip decoding while paused
                if is_paused {
                    continue;
                }
            }

            // Check for control messages when not paused
            match control_receiver.try_recv() {
                Ok(PlayerControl::Pause) => {
                    println!("\x1B[6;1HDecoder thread: pausing playback");
                    let mut paused_guard = paused_clone.lock().unwrap();
                    *paused_guard = true;
                    continue;
                },
                Ok(PlayerControl::Seek(pos)) => {
                    println!("\x1B[6;1HDecoder seeking to {:.2}s", pos);
                    if let Err(e) = decoder.seek(pos) {
                        println!("\x1B[7;1HSeek failed: {}", e);
                    } else {
                        // Clear the queues on seek
                        decoder_thread_queue.clear();
                    }
                },
                Ok(PlayerControl::Stop) => {
                    println!("\x1B[6;1HDecoder thread: stopping");
                    let mut running_state = running_decoder.lock().unwrap();
                    *running_state = false;
                    break;
                },
                Err(mpsc::TryRecvError::Empty) => {
                    // No control message, continue normal operation
                },
                Err(mpsc::TryRecvError::Disconnected) => {
                    println!("\x1B[6;1HDecoder thread: control channel disconnected");
                    let mut running_state = running_decoder.lock().unwrap();
                    *running_state = false;
                    break;
                },
                _ => {}
            }

            // Check if the decode queue is full
            if decoder_thread_queue.is_full() {
                // Queue is full, wait a bit
                thread::sleep(Duration::from_millis(5));
                continue;
            }

            // Decode the next frame
            match decoder.decode_next_frame() {
                Ok(Some(frame)) => {
                    frames_decoded += 1;

                    if frames_decoded % 30 == 0 {
                        println!(
                            "\x1B[8;1HDecoded frame #{}: timestamp={:.2}s",
                            frames_decoded, frame.timestamp
                        );
                    }

                    // Push to decode queue - use the thread-specific clone
                    decoder_thread_queue.push(DecodedFrame {
                        frame: Arc::new(frame.clone()),
                        timestamp: frame.timestamp,
                    });
                }
                Ok(None) => {
                    // End of video
                    println!("\x1B[9;1HEnd of video reached");
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    println!("\x1B[9;1HError decoding frame: {}", e);
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }

        println!("\x1B[10;1HDecoder thread exiting");
    });

    // ========== PREPARE THREAD ==========
    let prepare_thread = thread::spawn(move || {
        println!("\x1B[10;1HPrepare thread starting...");
        std::io::stdout().flush().unwrap();

        let mut frames_prepared = 0;

        // Pre-allocate image paths
        let mut frame_paths = Vec::with_capacity(10);
        for i in 0..10 {
            frame_paths.push(temp_dir_clone.join(format!("frame_{}.png", i)));
        }

        // Improved processing loop
        while *running_prepare.lock().unwrap() {
            // Check if the display queue is full
            if prepare_thread_display_queue.is_full() {
                // Queue is full, wait a bit before checking again
                thread::sleep(Duration::from_millis(5));
                continue;
            }

            // Always check for frames to process - don't use loop conditions that might skip frames
            match prepare_thread_decode_queue.pop() {
                Some(decoded_frame) => {
                    frames_prepared += 1;

                    // Get a frame path to use (cycling through the pre-allocated paths)
                    let frame_path =
                        Arc::new(frame_paths[frames_prepared % frame_paths.len()].clone());

                    // Use optimized frame preparation for better performance
                    match prepare_frame_fast(
                        &decoded_frame.frame,
                        &frame_path,
                        terminal_width * scaling_factor,
                        terminal_height * scaling_factor,
                    ) {
                        Ok(()) => {
                            if frames_prepared % 30 == 0 {
                                println!(
                                    "\x1B[11;1HPrepared frame #{}: timestamp={:.2}s",
                                    frames_prepared, decoded_frame.timestamp
                                );
                            }

                            // Add clear success/failure feedback for display queue
                            if prepare_thread_display_queue.push(PreparedFrame {
                                image_path: frame_path,
                                timestamp: decoded_frame.timestamp,
                            }) {
                                // Successfully added to display queue
                                if frames_prepared % 30 == 0 {
                                    println!("\x1B[12;1HFrame added to display queue");
                                }
                            } else {
                                // Queue is full, skip frame
                                println!("\x1B[12;1HDisplay queue full, skipping frame");
                            }
                        }
                        Err(e) => {
                            println!("\x1B[11;1HError preparing frame: {}", e);
                        }
                    }
                }
                None => {
                    // No frames to prepare, sleep a very small amount
                    thread::sleep(Duration::from_micros(500));
                }
            }
        }

        println!("\x1B[12;1HPrepare thread exiting");
    });

    // Main thread handles display and user input
    println!("\x1B[13;1HDisplay thread starting...");
    std::io::stdout().flush()?;

    // Display variables
    let mut rendered_frames = 0;
    let mut last_fps_update = Instant::now();
    let mut actual_fps = 0.0;
    let mut _last_frame_time = Instant::now();

    // Main loop
    loop {
        // Check for events with error handling
        if let Ok(has_event) = event::poll(Duration::from_millis(10)) {
            if has_event {
                if let Ok(Event::Key(key)) = event::read() {
                    match key.code {
                        KeyCode::Char('q') => {
                            println!("\x1B[14;1HSending stop command");
                            // Set running to false for all threads
                            {
                                let mut running_state = running.lock().unwrap();
                                *running_state = false;
                            }
                            control_sender.send(PlayerControl::Stop)?;
                            break;
                        }
                        KeyCode::Char(' ') => {
                            // Toggle between play/pause
                            let current_paused = *paused.lock().unwrap();
                            if current_paused {
                                println!("\x1B[10;1HMain thread: resuming playback");
                                // Send play command to decoder thread
                                control_sender.send(PlayerControl::Play)?;
                            } else {
                                println!("\x1B[10;1HMain thread: pausing playback");
                                // Send pause command to decoder thread
                                control_sender.send(PlayerControl::Pause)?;
                            }
                            // Update local pause state immediately for UI responsiveness
                            let mut paused_lock = paused.lock().unwrap();
                            *paused_lock = !current_paused;
                        }
                        KeyCode::Left => {
                            let new_position = if current_position > 5.0 {
                                current_position - 5.0
                            } else {
                                0.0
                            };
                            control_sender.send(PlayerControl::Seek(new_position))?;
                            current_position = new_position;
                            // Clear the display queue on seek
                            main_thread_display_queue.clear();
                        }
                        KeyCode::Right => {
                            let new_position = if current_position + 5.0 < info.duration {
                                current_position + 5.0
                            } else {
                                info.duration - 1.0
                            };
                            control_sender.send(PlayerControl::Seek(new_position))?;
                            current_position = new_position;
                            // Clear the display queue on seek
                            main_thread_display_queue.clear();
                        }
                        _ => {}
                    }
                }
            }
        } else {
            // Failed to poll events, sleep a little and continue
            thread::sleep(Duration::from_millis(10));
        }

        // Improved frame display logic
        if !*paused.lock().unwrap() {
            // Try to get a frame regardless of timing - process frames as fast as possible
            if let Some(next_frame) = main_thread_display_queue.pop() {
                // Update our position
                current_position = next_frame.timestamp;

                // Display the frame - use display_x and display_y instead of config.x and config.y
                display_frame(
                    &next_frame.image_path,
                    display_x as usize,
                    display_y as usize + 1,
                )?;

                // Update rendering stats
                rendered_frames += 1;
                let now = Instant::now();
                if now.duration_since(last_fps_update) >= Duration::from_secs(1) {
                    actual_fps =
                        rendered_frames as f32 / now.duration_since(last_fps_update).as_secs_f32();
                    rendered_frames = 0;
                    last_fps_update = now;
                }

                // Update the status line with correct queue info
                print!("\x1B[1;1H");
                print!(
                    "Frame: {:.2}/{:.2}s | Target FPS: {:.1} | Actual FPS: {:.1} | Decode: {}/{} | Display: {}/{}",
                    current_position,
                    info.duration,
                    info.frame_rate,
                    actual_fps,
                    main_thread_decode_queue.len(),
                    decode_queue_size,
                    main_thread_display_queue.len(),
                    display_queue_size
                );

                // Update frame timing
                _last_frame_time = Instant::now();
            } else {
                // No frames available, sleep a tiny bit
                thread::sleep(Duration::from_micros(500));
            }
        } else {
            // When paused, sleep longer
            thread::sleep(Duration::from_millis(50));
        }
    }

    // Clean up
    println!("\x1B[15;1HCleaning up...");
    std::io::stdout().flush()?;

    // Wait for threads to finish
    if let Err(e) = decoder_thread.join() {
        println!("Error joining decoder thread: {:?}", e);
    }

    if let Err(e) = prepare_thread.join() {
        println!("Error joining prepare thread: {:?}", e);
    }

    // Clean up temp directory
    std::fs::remove_dir_all(&temp_dir).ok();

    terminal::disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;

    Ok(())
}

// New function for faster frame preparation with improved error handling
fn prepare_frame_fast(
    frame: &VideoFrame,
    output_path: &std::path::Path,
    target_width: usize,
    target_height: usize,
) -> Result<()> {
    // Safety check input dimensions
    if frame.width == 0 || frame.height == 0 {
        return Err(anyhow::anyhow!(
            "Invalid frame dimensions: {}x{}",
            frame.width,
            frame.height
        ));
    }

    // Cap maximum dimensions to avoid excessive memory usage
    let max_dimension = 2048;
    let capped_target_width = target_width.min(max_dimension);
    let capped_target_height = target_height.min(max_dimension);

    log::debug!(
        "Preparing frame: original={}x{}, target={}x{}",
        frame.width,
        frame.height,
        capped_target_width,
        capped_target_height
    );

    // Use the frame's resize method directly for maximum efficiency
    let resized_frame = if frame
        .needs_resize(capped_target_width as u32, capped_target_height as u32)
    {
        // For significant upscaling, limit the max size to improve performance
        let scaling_factor = (capped_target_width as f32 / frame.width as f32)
            .max(capped_target_height as f32 / frame.height as f32);

        log::trace!("Scaling factor: {:.2}", scaling_factor);

        if scaling_factor > 2.0 {
            // Use a lower target size for better performance
            let actual_width = (frame.width as f32 * 2.0).min(capped_target_width as f32) as u32;
            let actual_height = (frame.height as f32 * 2.0).min(capped_target_height as f32) as u32;
            log::debug!(
                "Using limited dimensions: {}x{}",
                actual_width,
                actual_height
            );
            frame.resize(actual_width, actual_height, true)
        } else {
            // Normal resize
            frame.resize(
                capped_target_width as u32,
                capped_target_height as u32,
                true,
            )
        }
    } else {
        // No resize needed
        log::debug!("No resize needed");
        frame.clone()
    };

    // Create parent directory if it doesn't exist
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // Try saving with different formats if PNG fails
    let save_result = resized_frame.image.to_rgba8().save(output_path);
    if let Err(e) = save_result {
        log::warn!("Failed to save as PNG: {:?}, trying JPEG", e);

        // Try JPEG as fallback
        let mut jpeg_data = std::io::Cursor::new(Vec::new());
        let img = resized_frame.image.to_rgba8();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_data, 90);
        encoder.encode_image(&img)?;

        // Save JPEG data to file
        let jpeg_path = output_path.with_extension("jpg");
        std::fs::write(&jpeg_path, jpeg_data.into_inner())?;

        // Rename to original path
        std::fs::rename(jpeg_path, output_path)?;
    }

    log::debug!("Frame saved to {:?}", output_path);
    Ok(())
}

// The unused prepare_frame function has been removed in favor of prepare_frame_fast

// Function for displaying a frame from an image file
fn display_frame(image_path: &std::path::Path, x: usize, y: usize) -> Result<()> {
    // Verify file exists and is accessible
    if !image_path.exists() {
        return Err(anyhow::anyhow!("Image file does not exist: {:?}", image_path));
    }
    
    // Try to get file size to verify it's a valid file
    match std::fs::metadata(image_path) {
        Ok(metadata) => {
            let size = metadata.len();
            if size == 0 {
                return Err(anyhow::anyhow!("Image file is empty: {:?}", image_path));
            }
            log::trace!("Image size: {} bytes", size);
        },
        Err(e) => {
            log::warn!("Unable to read file metadata: {:?}", e);
            // Continue anyway
        }
    }
    
    // Load the image with error handling
    let img = match image::open(image_path) {
        Ok(img) => img,
        Err(e) => {
            log::error!("Failed to open image: {:?}", e);
            // Try to read file with more specific approach
            let file_data = std::fs::read(image_path)?;
            match image::load_from_memory(&file_data) {
                Ok(img) => img,
                Err(e2) => {
                    log::error!("Also failed with memory approach: {:?}", e2);
                    return Err(anyhow::anyhow!("Could not load image: {:?}", e));
                }
            }
        }
    };
    
    // Check image dimensions
    if img.width() == 0 || img.height() == 0 {
        return Err(anyhow::anyhow!("Invalid image dimensions: {}x{}", img.width(), img.height()));
    }
    
    log::debug!("Loaded image: {}x{}", img.width(), img.height());
    
    // Create a video frame from the image
    let frame = VideoFrame::new(
        img,
        0.0, // timestamp doesn't matter here
        0.04, // standard frame duration at 25fps
    );
    
    // First try Kitty if that's likely to work
    let kitty_available = std::env::var("KITTY_WINDOW_ID").is_ok();
    
    // Determine best method automatically
    let method = if kitty_available {
        RenderMethod::Kitty
    } else {
        // Try to auto-detect best method
        core::render::TerminalRenderer::detect_best_method()
    };
    
    log::debug!("Using render method: {:?}", method);
    
    // Create a renderer with specified position and adaptive resolution
    let config_x = x as u16;
    let config_y = y as u16;
    
    let config = RenderConfig {
        method,
        maintain_aspect: true,
        x: config_x,
        y: config_y,
        adaptive_resolution: true, // Enable adaptive resolution for better performance
        quality: 0.8,             // Start with decent quality for good balance
        enable_threading: true,   // Enable multi-threaded processing
        max_frame_dimension: Some(1024), // Limit max frame size for performance
        enable_gpu: false,        // Disable GPU acceleration for Kitty compatibility
        ..Default::default()
    };
    
    // Create renderer with fallback option
    let mut renderer = match core::render::TerminalRenderer::new(config) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("Failed to create renderer with {:?}: {:?}", method, e);
            // Try blocks as fallback
            let fallback_config = RenderConfig {
                method: RenderMethod::Blocks,
                maintain_aspect: true,
                x: config_x,
                y: config_y,
                adaptive_resolution: true,
                quality: 0.7,
                target_fps: 30.0,
                enable_threading: true,
                max_frame_dimension: Some(1024),
                enable_gpu: false,
                ..Default::default()
            };
            core::render::TerminalRenderer::new(fallback_config)?
        }
    };
    
    // Render the frame with proper error handling
    match renderer.render(&frame) {
        Ok(_) => {
            // Ensure output is flushed
            std::io::stdout().flush()?;
            Ok(())
        },
        Err(e) => {
            log::error!("Failed to render frame: {:?}", e);
            // Try to clean up terminal state
            print!("\x1B[0m");
            std::io::stdout().flush()?;
            Err(e)
        }
    }
}
