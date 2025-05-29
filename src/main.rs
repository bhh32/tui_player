use std::{io, time::{Duration, Instant}, fs::OpenOptions, io::Write};
use anyhow::{Result, Context};
use ratatui::{
    backend::CrosstermBackend, 
    Terminal,
    style::Color,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

// Debug logger to file for development
fn debug_log(message: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("tui_player_debug.log") 
    {
        let datetime = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let thread_id = std::thread::current().id();
        
        // Get backtrace info for important messages
        let backtrace = if message.contains("ERROR") || message.contains("PANIC") {
            format!("\n    at {}", std::backtrace::Backtrace::capture())
        } else {
            String::new()
        };
        
        let _ = writeln!(file, "[{} {:?}] {}{}", datetime, thread_id, message, backtrace);
    }
}

mod app;
mod commands;
mod events; // Contains event utility functions
mod ui;

use app::App;

fn main() -> Result<()> {
    // Setup logger
    env_logger::init();
    
    // Initialize debug log
    debug_log("Application starting");
    
    // Set up clean terminal restoration on panic
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Try to restore terminal to a usable state
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        
        // Log the panic
        debug_log(&format!("PANIC: {}", panic_info));
        
        // Call the original hook
        orig_hook(panic_info);
    }));
    
    // Setup terminal with better error handling
    match enable_raw_mode() {
        Ok(_) => debug_log("Raw mode enabled"),
        Err(e) => {
            debug_log(&format!("Failed to enable raw mode: {}", e));
            return Err(anyhow::anyhow!("Failed to enable raw mode: {}", e));
        }
    }
    
    let mut stdout = io::stdout();
    if let Err(e) = execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture
    ) {
        let _ = disable_raw_mode();
        debug_log(&format!("Failed to setup terminal: {}", e));
        return Err(anyhow::anyhow!("Failed to setup terminal: {}", e));
    }
    
    debug_log("Terminal setup complete");
    
    // Create backend and terminal
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(term) => term,
        Err(e) => {
            let _ = disable_raw_mode();
            debug_log(&format!("Failed to create terminal: {}", e));
            return Err(anyhow::anyhow!("Failed to create terminal: {}", e));
        }
    };
    
    // Create app state and initialize
    let mut app = App::new();
    debug_log("App initialized");
    
    // If a command line argument is provided, try to open it
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        debug_log(&format!("Attempting to open media: {}", &args[1]));
        // Try to open the specified file or URL
        if let Err(e) = app.open_media(&args[1]) {
            let error_msg = format!("Error opening media: {}", e);
            debug_log(&error_msg);
            eprintln!("{}", error_msg);
            app.set_status(format!("Error: {}", e), Color::Red);
        } else {
            debug_log("Media opened successfully");
            app.set_status("Media loaded successfully", Color::Green);
        }
    }
    
    // Create render timer
    let mut last_tick = Instant::now();
    
    // Configure for smoother animation with faster redraw
    let tick_rate = Duration::from_millis(33); // ~30 FPS for UI (reduces flicker)
    let video_update_rate = Duration::from_millis(16); // ~60 FPS for video updates
    let mut last_video_update = Instant::now();
    let mut frame_count = 0;
    let mut last_fps_update = Instant::now();

    debug_log("Entering main loop");
    
    // Main loop
    while !app.should_quit {
        // Use controlled rendering to prevent flickering
        // Only render if enough time has passed since last render
        let now = Instant::now();
        let time_since_render = now.duration_since(last_tick);
        
        if time_since_render >= tick_rate {
            // Draw the UI without clearing first to prevent flickering
            let draw_start = Instant::now();
            if let Err(e) = terminal.draw(|f| {
                match ui::draw_ui(f, &mut app) {
                    Ok(_) => {},
                    Err(e) => {
                        debug_log(&format!("ERROR: UI draw function error: {}", e));
                    }
                }
            }) {
                debug_log(&format!("ERROR: Terminal draw error: {}", e));
                // Don't propagate terminal draw errors to avoid crashing
                // on non-critical display issues
            }
            let draw_time = draw_start.elapsed();
            if draw_time > Duration::from_millis(50) {
                debug_log(&format!("SLOW RENDER: UI draw took {}ms", draw_time.as_millis()));
            }
            
            last_tick = now;
        }
        
        // Count frames for FPS calculation
        frame_count += 1;
        if last_fps_update.elapsed() >= Duration::from_secs(5) {
            let fps = frame_count as f64 / last_fps_update.elapsed().as_secs_f64();
            let video_fps = if let Some(player) = &app.player {
                player.get_media_info().map_or(0.0, |info| info.frame_rate)
            } else {
                0.0
            };
            debug_log(&format!("PERFORMANCE: UI refresh rate: {:.2} FPS | Video: {:.2} FPS | View: {:?} | Render method: {:?}", 
                    fps, 
                    video_fps,
                    app.view,
                    app.render_config.method));
            frame_count = 0;
            last_fps_update = Instant::now();
        }
        
        // Handle input events with a reasonable timeout
        let timeout = Duration::from_millis(10);
            
        if crossterm::event::poll(timeout)? {
            match event::read() {
                Ok(Event::Key(key)) => {
                    debug_log(&format!("EVENT: Key {:?} with modifiers {:?}", key.code, key.modifiers));
                    
                    // Check for global quit key (Ctrl+C or Ctrl+Q)
                    if (key.code == event::KeyCode::Char('c') || key.code == event::KeyCode::Char('q')) 
                        && key.modifiers.contains(event::KeyModifiers::CONTROL) {
                        debug_log("ACTION: Quit key pressed, exiting application");
                        app.should_quit = true;
                        break;
                    }
                    
                    // Check for command mode (press ':')
                    if key.code == event::KeyCode::Char(':') && !app.is_command_mode() {
                        debug_log("Entering command mode");
                        app.enter_command_mode();
                    } else if app.is_command_mode() && key.code == event::KeyCode::Enter {
                        // Execute command when Enter is pressed in command mode
                        let cmd = app.get_command_buffer().to_string();
                        debug_log(&format!("Executing command: {}", cmd));
                        app.exit_command_mode();
                        
                        if let Err(e) = commands::handle_command(&mut app, &cmd) {
                            debug_log(&format!("Command error: {}", e));
                            app.set_status(format!("Error: {}", e), Color::Red);
                        } else {
                            debug_log("Command executed successfully");
                        }
                    } else {
                        debug_log(&format!("Handling key event: {:?}", key));
                        if let Err(e) = app.handle_key_event(key) {
                            debug_log(&format!("Key handler error: {}", e));
                            app.set_status(format!("Key error: {}", e), Color::Red);
                        }
                    }
                }
                Ok(Event::Mouse(mouse)) => {
                    let area = terminal.get_frame().area();
                    debug_log(&format!("EVENT: Mouse {:?} at col={} row={} in area {}x{}", 
                                     mouse.kind, mouse.column, mouse.row, area.width, area.height));
                    if let Err(e) = app.handle_mouse_event(mouse, area) {
                        debug_log(&format!("ERROR: Mouse handler error: {}", e));
                        app.set_status(format!("Mouse error: {}", e), Color::Red);
                    }
                }
                Ok(Event::Resize(w, h)) => {
                    debug_log(&format!("Resize event: {}x{}", w, h));
                    // Force a redraw on resize
                    last_tick = Instant::now() - tick_rate;
                }
                Ok(_) => {
                    debug_log("Unknown event type");
                }
                Err(e) => {
                    debug_log(&format!("Error reading event: {}", e));
                }
            }
        }
        
        // Update video state more frequently than UI for smooth playback
        let video_elapsed = last_video_update.elapsed();
        if video_elapsed >= video_update_rate {
            if app.view == app::AppView::Player && app.player.is_some() {
                if let Some(player) = &mut app.player {
                    let video_update_start = Instant::now();
                    match player.update() {
                        Ok(_) => {
                            // Video frame updated successfully
                            let actual_update_time = video_update_start.elapsed();
                            if actual_update_time > Duration::from_millis(33) {
                                // Log if frame decode/render took longer than ~30fps would allow
                                debug_log(&format!("SLOW VIDEO: Update took {}ms (target: {}ms) | Position: {:.2}s", 
                                                 actual_update_time.as_millis(), 
                                                 video_update_rate.as_millis(),
                                                 player.get_position()));
                            }
                        },
                        Err(e) => {
                            debug_log(&format!("ERROR: Playback error: {} at position {:.2}s", 
                                             e, player.get_position()));
                            app.set_status(format!("Playback error: {}", e), Color::Red);
                        }
                    }
                }
            }
            last_video_update = Instant::now();
            
            // Force UI update after video frame to keep UI responsive
            if app.view == app::AppView::Player {
                // Reduce time to next UI update to keep controls responsive
                last_tick = last_tick.checked_sub(Duration::from_millis(5)).unwrap_or_else(Instant::now);
            }
        }
        
        // Update app state at a controlled rate
        if now.duration_since(last_tick) >= tick_rate {
            if let Err(e) = app.update() {
                debug_log(&format!("App update error: {}", e));
                app.set_status(format!("Error: {}", e), Color::Red);
            }
        }
        
        // Sleep just enough to avoid 100% CPU usage but maintain responsiveness
        // Dynamic sleep time based on next scheduled event
        let next_video_update = video_update_rate.checked_sub(last_video_update.elapsed()).unwrap_or_default();
        let next_ui_update = tick_rate.checked_sub(now.duration_since(last_tick)).unwrap_or_default();
        let sleep_time = std::cmp::min(next_video_update, next_ui_update);
        
        // Ensure we don't sleep for too long or too little
        let sleep_time = std::cmp::min(sleep_time, Duration::from_millis(10));
        if !sleep_time.is_zero() {
            std::thread::sleep(sleep_time);
        }
    }
    
    debug_log("Shutting down application");
    
    // Clean up any player resources first
    if let Some(player) = &mut app.player {
        if let Err(e) = player.stop() {
            debug_log(&format!("Error stopping player: {}", e));
        }
    }
    
    // Restore terminal state in a way that ensures cleanup even on panic
    debug_log("CLEANUP: Starting terminal cleanup sequence");
    let cleanup_result = (|| -> Result<()> {
        // Disable raw mode first
        debug_log("CLEANUP: Disabling raw mode");
        disable_raw_mode().context("Failed to disable raw mode")?;
        
        // Execute terminal cleanup commands
        debug_log("CLEANUP: Leaving alternate screen and disabling mouse capture");
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        ).context("Failed to leave alternate screen")?;
        
        // Show cursor
        debug_log("CLEANUP: Restoring cursor visibility");
        terminal.show_cursor().context("Failed to show cursor")?;
        
        Ok(())
    })();
    
    if let Err(e) = cleanup_result {
        debug_log(&format!("Error during cleanup: {}", e));
        eprintln!("Error during cleanup: {}", e);
    }
    
    debug_log("Application terminated successfully");
    Ok(())
}