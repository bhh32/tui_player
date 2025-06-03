use anyhow::{Result, anyhow};
use core::{render::RenderMethod};

use crate::app::App;

/// Command handler for the application
pub struct CommandHandler;

impl CommandHandler {
    /// Parse and execute a command
    pub fn execute(app: &mut App, command_str: &str) -> Result<()> {
        let parts: Vec<&str> = command_str.trim().splitn(2, ' ').collect();
        let cmd = parts[0].to_lowercase();
        let args = parts.get(1).map(|s| s.trim());
        
        match cmd.as_str() {
            "seek" | "s" => {
                if let Some(args) = args {
                    if let Ok(position) = args.parse::<f64>() {
                        if let Some(player) = &mut app.player {
                            player.seek(position)?;
                        }
                    } else {
                        return Err(anyhow!("Invalid position: {}", args));
                    }
                } else {
                    return Err(anyhow!("Seek command requires a position argument"));
                }
            },
            "play" | "p" => {
                if let Some(player) = &mut app.player {
                    if player.is_paused() {
                        player.toggle_pause();
                    }
                }
            },
            "pause" => {
                if let Some(player) = &mut app.player {
                    if !player.is_paused() {
                        player.toggle_pause();
                    }
                }
            },
            "toggle" | "t" => {
                if let Some(player) = &mut app.player {
                    player.toggle_pause();
                }
            },
            "volume" | "vol" | "v" => {
                if let Some(args) = args {
                    if let Ok(volume) = args.parse::<u8>() {
                        // TODO: Implement volume control in the MediaPlayer trait
                        app.set_status(format!("Volume set to {}", volume), ratatui::style::Color::Yellow);
                    } else {
                        return Err(anyhow!("Invalid volume: {}", args));
                    }
                } else {
                    return Err(anyhow!("Volume command requires a level argument (0-100)"));
                }
            },
            "open" | "o" => {
                if let Some(path) = args {
                    app.open_media(path)?;
                } else {
                    app.view = crate::app::AppView::FileBrowser;
                    app.refresh_file_list()?;
                }
            },
            "menu" | "home" | "main" => {
                // Return to main menu from any view
                if app.view == crate::app::AppView::Player {
                    // Stop playback if in player view
                    if let Some(player) = &mut app.player {
                        let _ = player.stop();
                    }
                    app.player = None;
                    app.media_info = None;
                }
                app.view = crate::app::AppView::MainMenu;
                app.set_status("Main Menu", ratatui::style::Color::Blue);
            },
            "youtube" | "yt" | "y" => {
                if let Some(query) = args {
                    // If it looks like a YouTube URL or ID, open it directly
                    if query.contains("youtube.com") || query.contains("youtu.be") || query.len() == 11 {
                        app.open_media(query)?;
                    } else {
                        // Otherwise, set it as a search query and perform a search
                        app.view = crate::app::AppView::YoutubeSearch;
                        app.input = query.to_string();
                        app.input_cursor = query.len();
                        
                        // Generate some mock search results
                        app.youtube_search.results.clear();
                        app.youtube_search.searching = true;
                        app.set_status(format!("Searching for '{}'...", query), ratatui::style::Color::Yellow);
                        
                        // Use the app's internal search mechanism (real YouTube search)
                        app.youtube_search.query = query.to_string();
                        app.youtube_search.searching = true;
                        
                        // This will trigger the actual search in the background
                        // Results will be populated by the app's update method
                        app.set_status(format!("Searching YouTube for '{}'...", query), 
                                     ratatui::style::Color::Yellow);
                        
                        // Add a loading placeholder
                        app.youtube_search.results.push(crate::app::YoutubeResult {
                            id: "loading".to_string(),
                            title: "Searching YouTube...".to_string(),
                            duration: "".to_string(),
                            thumbnail: None,
                            downloaded_thumbnail: None,
                            channel: "Please wait".to_string(),
                        });
                        
                        app.youtube_search.selected = Some(0);
                    }
                } else {
                    app.view = crate::app::AppView::YoutubeSearch;
                }
            },
            "quality" | "q" => {
                if let Some(args) = args {
                    if let Ok(quality) = args.parse::<u8>() {
                        if quality <= 5 {
                            app.youtube_config.quality = quality;
                            app.set_status(format!("YouTube quality set to {}", quality), ratatui::style::Color::Green);
                        } else {
                            return Err(anyhow!("Quality must be between 0 and 5"));
                        }
                    } else {
                        return Err(anyhow!("Invalid quality: {}", args));
                    }
                } else {
                    return Err(anyhow!("Quality command requires a value argument (0-5)"));
                }
            },
            "renderer" | "render" | "r" => {
                if let Some(args) = args {
                    let render_method = match args {
                        "auto" => RenderMethod::Auto,
                        "blocks" => RenderMethod::Blocks,
                        "kitty" => RenderMethod::Kitty,
                        "sixel" => RenderMethod::Sixel,
                        "iterm" => RenderMethod::ITerm,
                        _ => return Err(anyhow!("Unknown render method: {}", args)),
                    };
                    
                    app.render_config.method = render_method;
                    app.set_status(format!("Renderer set to {:?}", render_method), ratatui::style::Color::Green);
                } else {
                    return Err(anyhow!("Renderer command requires a method argument"));
                }
            },
            "restart" => {
                if let Some(player) = &mut app.player {
                    player.seek(0.0)?;
                }
            },
            "gpu" => {
                if let Some(args) = args {
                    let enable = match args {
                        "on" | "true" | "1" | "enable" | "enabled" => true,
                        "off" | "false" | "0" | "disable" | "disabled" => false,
                        _ => return Err(anyhow!("Invalid GPU setting: {}", args)),
                    };
                    
                    app.render_config.enable_gpu = enable;
                    app.set_status(
                        format!("GPU acceleration {}", if enable { "enabled" } else { "disabled" }), 
                        ratatui::style::Color::Green
                    );
                } else {
                    // Toggle current setting
                    app.render_config.enable_gpu = !app.render_config.enable_gpu;
                    app.set_status(
                        format!("GPU acceleration {}", if app.render_config.enable_gpu { "enabled" } else { "disabled" }), 
                        ratatui::style::Color::Green
                    );
                }
            },
            "settings" | "config" => {
                app.show_settings = true;
            },
            "help" | "h" | "?" => {
                app.show_help = true;
            },
            "quit" | "exit" => {
                if app.view == crate::app::AppView::Player {
                    // Stop playback and return to main menu
                    if let Some(player) = &mut app.player {
                        let _ = player.stop();
                    }
                    app.player = None;
                    app.media_info = None;
                    app.view = crate::app::AppView::MainMenu;
                    app.set_status("Returned to main menu", ratatui::style::Color::Blue);
                } else {
                    // Exit application
                    app.should_quit = true;
                }
            },
            "" => {
                // Empty command, do nothing
            },
            _ => {
                return Err(anyhow!("Unknown command: {}", cmd));
            }
        }
        
        Ok(())
    }
}

/// Handle a command string entered by the user
pub fn handle_command(app: &mut App, command: &str) -> Result<()> {
    let result = CommandHandler::execute(app, command);
    
    // If command was successful and it's a search, ensure we have the right view
    if result.is_ok() {
        if command.starts_with("youtube") || command.starts_with("yt") || command.starts_with("y") {
            // Make sure we're in the right view
            if app.view == crate::app::AppView::YoutubeSearch {
                // Ensure the YouTube search view is properly set up
                if app.youtube_search.results.is_empty() && !app.youtube_search.searching {
                    app.set_status("Enter search term or YouTube URL", ratatui::style::Color::Yellow);
                }
            }
        } else if command == "help" || command == "h" || command == "?" {
            // Make the help overlay visible regardless of view
            app.show_help = true;
        }
    }
    
    result
}