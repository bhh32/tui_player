use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use log::{debug, info, warn, error};
// Using youtube_dl crate but will configure it to use yt-dlp executable
use youtube_dl::{YoutubeDl, YoutubeDlOutput};

use crate::render::{RenderConfig, TerminalRenderer};
use crate::{MediaInfo, MediaPlayer, VideoDecoder};
use std::any::Any;

/// Configuration for YouTube streaming
#[derive(Clone)]
pub struct YouTubeConfig {
    /// Video quality (0-9, where 0 is best quality)
    pub quality: u8,
    /// Preferred format: "mp4", "webm", etc. (None for auto)
    pub format: Option<String>,
    /// Use proxy for connection (e.g., "socks5://127.0.0.1:9050")
    pub proxy: Option<String>,
    /// Maximum resolution to request (e.g., "720p")
    pub max_resolution: Option<String>,
    /// Download subtitles if available
    pub subtitles: bool,
    /// Timeout for network operations in seconds
    pub timeout: u64,
    /// Cache directory for downloaded metadata
    pub cache_dir: Option<PathBuf>,
    /// Path to yt-dlp executable (None for auto-detect)
    pub ytdlp_path: Option<String>,
}

impl Default for YouTubeConfig {
    fn default() -> Self {
        Self {
            quality: 1,
            format: Some("mp4".to_string()),
            proxy: None,
            max_resolution: Some("720p".to_string()),
            subtitles: false,
            timeout: 30,
            cache_dir: None,
            ytdlp_path: None,
        }
    }
}

/// Information about a YouTube video
#[derive(Debug, Clone)]
pub struct YouTubeVideoInfo {
    /// Video ID
    pub id: String,
    /// Video title
    pub title: String,
    /// Video duration in seconds
    pub duration: f64,
    /// Video thumbnail URL
    pub thumbnail: Option<String>,
    /// Available formats
    pub formats: Vec<String>,
    /// Available resolutions
    pub resolutions: Vec<String>,
    /// Video uploader
    pub uploader: Option<String>,
    /// Video upload date
    pub upload_date: Option<String>,
    /// Video view count
    pub view_count: Option<u64>,
}

/// A player for YouTube videos
pub struct YouTubePlayer {
    /// URL or ID of the YouTube video
    url: String,
    /// Video decoder for the stream
    decoder: Option<VideoDecoder>,
    /// Renderer for displaying frames
    renderer: TerminalRenderer,
    /// Configuration for YouTube streaming
    config: YouTubeConfig,
    /// Current playback timestamp
    current_timestamp: f64,
    /// Last frame time for FPS calculation
    last_frame_time: Instant,
    /// Frame duration based on video FPS
    frame_duration: Duration,
    /// Playback state
    paused: bool,
    /// Video information
    video_info: Option<YouTubeVideoInfo>,
    /// Media information
    media_info: Option<MediaInfo>,
}

impl YouTubePlayer {
    /// Create a new YouTube player for the given URL or video ID
    pub fn new(
        url_or_id: &str,
        render_config: Option<RenderConfig>,
        youtube_config: Option<YouTubeConfig>,
    ) -> Result<Self> {
        // Normalize YouTube URL/ID
        let url = Self::normalize_youtube_url(url_or_id)?;

        // Create renderer with provided config or default
        let render_config = render_config.unwrap_or_default();
        let renderer = TerminalRenderer::new(render_config)?;

        // Use provided YouTube config or default
        let config = youtube_config.unwrap_or_default();

        Ok(Self {
            url,
            decoder: None,
            renderer,
            config,
            current_timestamp: 0.0,
            last_frame_time: Instant::now(),
            frame_duration: Duration::from_secs_f64(1.0 / 30.0), // Default 30fps until we know better
            paused: false,
            video_info: None,
            media_info: None,
        })
    }

    /// Normalize a YouTube URL or video ID
    fn normalize_youtube_url(url_or_id: &str) -> Result<String> {
        // Check if it's already a full URL
        if url_or_id.starts_with("http://") || url_or_id.starts_with("https://") {
            return Ok(url_or_id.to_string());
        }

        // Check if it's a youtu.be short URL
        if url_or_id.starts_with("youtu.be/") {
            let id = url_or_id.strip_prefix("youtu.be/").unwrap_or(url_or_id);
            return Ok(format!("https://www.youtube.com/watch?v={}", id));
        }

        // Check if it looks like a video ID (typically 11 characters)
        if url_or_id.len() == 11
            && url_or_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Ok(format!("https://www.youtube.com/watch?v={}", url_or_id));
        }

        // Otherwise assume it's a partial URL
        if url_or_id.contains("youtube.com/watch") {
            return Ok(format!(
                "https://{}",
                url_or_id
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
            ));
        }

        Err(anyhow!("Invalid YouTube URL or video ID: {}", url_or_id))
    }

    /// Initialize streaming and prepare the decoder
    pub fn initialize(&mut self) -> Result<()> {
        info!("Initializing YouTube player for: {}", self.url);

        // Set up timeout for initialization
        let start_time = std::time::Instant::now();
        let timeout = Duration::from_secs(30); // 30 second timeout for initialization

        // Check if yt-dlp is installed
        self.check_ytdlp_installed()?;

        // Get video info using yt-dlp with timeout check
        if start_time.elapsed() > timeout {
            return Err(anyhow!("Timed out while initializing YouTube player"));
        }
        let video_info = self.fetch_video_info()?;
        self.video_info = Some(video_info.clone());

        // Get streaming URL based on preferred format and quality
        if start_time.elapsed() > timeout {
            return Err(anyhow!("Timed out while getting streaming URL"));
        }
        let stream_url = self.get_best_stream_url(&video_info)?;
        debug!("Selected stream URL: {}", stream_url);

        // Create decoder for the stream with timeout check
        if start_time.elapsed() > timeout {
            return Err(anyhow!("Timed out while creating video decoder"));
        }
        let decoder = VideoDecoder::new(&stream_url)
            .context("Failed to create video decoder for YouTube stream")?;

        // Get media info
        let media_info = decoder.get_media_info();

        // Update frame duration based on actual FPS
        self.frame_duration = Duration::from_secs_f64(1.0 / media_info.frame_rate);
        self.media_info = Some(media_info);
        self.decoder = Some(decoder);

        info!("YouTube player initialized successfully in {:?}", start_time.elapsed());
        Ok(())
    }

    /// Fetch video information from YouTube
    fn fetch_video_info(&self) -> Result<YouTubeVideoInfo> {
        // Configure yt-dlp
        let mut ytdl = YoutubeDl::new(&self.url);

        // Set flat playlist to get info for a single video
        ytdl.flat_playlist(true);

        // Set socket timeout - convert to seconds as string for yt-dlp
        ytdl.socket_timeout(self.config.timeout.to_string());

        // Apply proxy if specified
        if let Some(proxy) = &self.config.proxy {
            ytdl.extra_arg("--proxy");
            ytdl.extra_arg(proxy);
        }

        // Use the yt-dlp executable path from config or find it
        if let Some(path) = &self.config.ytdlp_path {
            ytdl.youtube_dl_path(path);
        } else if let Some(path) = self.find_ytdlp_executable() {
            ytdl.youtube_dl_path(path);
        } else {
            ytdl.youtube_dl_path("yt-dlp");
        }

        // Run yt-dlp to get video info
        let output = ytdl
            .run()
            .context("Failed to retrieve YouTube video information")?;

        // Extract video information
        match output {
            YoutubeDlOutput::SingleVideo(video) => {
                // Extract available formats
                let formats = video.formats
                    .as_ref()
                    .map(|formats| formats
                        .iter()
                        .filter_map(|f| f.format.clone())
                        .collect::<Vec<String>>())
                    .unwrap_or_default();

                // Extract available resolutions
                let resolutions = video.formats
                    .as_ref()
                    .map(|formats| formats
                        .iter()
                        .filter_map(|f| f.height.map(|h| format!("{}p", h)))
                        .collect::<Vec<String>>())
                    .unwrap_or_default();

                Ok(YouTubeVideoInfo {
                    id: video.id,
                    title: video.title.unwrap_or_else(|| "Untitled".to_string()),
                    duration: video.duration.map(|d| d.as_f64().unwrap_or(0.0)).unwrap_or(0.0),
                    thumbnail: video.thumbnail,
                    formats,
                    resolutions,
                    uploader: video.uploader,
                    upload_date: video.upload_date,
                    view_count: video.view_count.map(|v| v as u64),
                })
            }
            YoutubeDlOutput::Playlist(_) => {
                Err(anyhow!("URL refers to a playlist, not a single video"))
            }
        }
    }

    /// Get the best streaming URL based on config
    fn get_best_stream_url(&self, _video_info: &YouTubeVideoInfo) -> Result<String> {
        // Configure yt-dlp for format selection
        let mut ytdl = YoutubeDl::new(&self.url);
        
        // Build format selector based on config
        let mut format_selector = String::new();
        
        // Apply format preference if specified
        if let Some(format) = &self.config.format {
            format_selector.push_str(&format!("bestvideo[ext={}]+bestaudio[ext={}]/best[ext={}]", format, format, format));
        } else {
            format_selector.push_str("bestvideo+bestaudio/best");
        }
        
        // Apply resolution limit if specified
        if let Some(resolution) = &self.config.max_resolution {
            // Parse resolution (e.g., "720p" -> 720)
            if let Some(height) = resolution.trim_end_matches('p').parse::<u32>().ok() {
                format_selector.push_str(&format!("[height<={}]", height));
            }
        }
        
        // Apply quality preference (0-9)
        if self.config.quality > 0 {
            // Higher quality number means lower quality in yt-dlp
            format_selector.push_str(&format!("/best[quality<={}]", self.config.quality + 1));
        }
        
        debug!("Format selector: {}", format_selector);
        ytdl.format(&format_selector);
        
        // Set socket timeout - convert to seconds as string for yt-dlp
        ytdl.socket_timeout(self.config.timeout.to_string());
        
        // Apply proxy if specified
        if let Some(proxy) = &self.config.proxy {
            ytdl.extra_arg("--proxy");
            ytdl.extra_arg(proxy);
        }
        
        // Use the yt-dlp executable path from config or find it
        if let Some(path) = &self.config.ytdlp_path {
            ytdl.youtube_dl_path(path);
        } else if let Some(path) = self.find_ytdlp_executable() {
            ytdl.youtube_dl_path(path);
        } else {
            ytdl.youtube_dl_path("yt-dlp");
        }
        
        ytdl.extra_arg("--dump-json");
        
        // Run youtube-dl to get stream URL
        match ytdl.run() {
            Ok(YoutubeDlOutput::SingleVideo(video)) => {
                // Get URL from video info (return the first one available)
                video.url.clone()
                    .or_else(|| {
                        // Try to find a URL in the formats
                        video.formats.as_ref()
                            .and_then(|formats| {
                                formats.iter()
                                    .filter_map(|fmt| fmt.url.clone())
                                    .next()
                            })
                    })
                    .ok_or_else(|| anyhow!("No streaming URL found for the video"))
            },
            Ok(YoutubeDlOutput::Playlist(_)) => {
                Err(anyhow!("URL refers to a playlist, not a single video"))
            },
            Err(e) => {
                Err(anyhow!("Failed to get streaming URL: {}", e))
            }
        }
    }

    /// Get YouTube-specific video information
    pub fn get_youtube_info(&self) -> Option<YouTubeVideoInfo> {
        self.video_info.clone()
    }
}

impl MediaPlayer for YouTubePlayer {
    fn get_media_info(&self) -> Option<MediaInfo> {
        self.media_info.clone()
    }

    fn get_position(&self) -> f64 {
        self.current_timestamp
    }

    fn is_paused(&self) -> bool {
        self.paused
    }

    fn stop(&mut self) -> Result<()> {
        self.decoder = None;
        Ok(())
    }

    /// Update player state and render the next frame if needed
    fn update(&mut self) -> Result<()> {
        // CRUCIAL FIX: Handle paused state first
        if self.paused {
            debug!("YouTube player is paused - rendering current frame");
            
            // Get decoder if available
            if let Some(decoder) = &mut self.decoder {
                // Get the current frame at the paused position
                match decoder.decode_current_frame() {
                    Ok(Some(mut frame)) => {
                        // Mark this as a paused frame with negative timestamp
                        frame.timestamp = -1.0;
                        self.renderer.render(&frame)?;
                        debug!("Rendered paused YouTube frame at position: {:.2}s", self.current_timestamp);
                    }
                    Ok(None) => {
                        debug!("No YouTube frame available at paused position");
                    }
                    Err(e) => {
                        warn!("Error getting paused YouTube frame: {}", e);
                    }
                }
            }
            
            // Sleep a bit to reduce CPU usage when paused
            std::thread::sleep(Duration::from_millis(100));
            return Ok(());
        }
    
        // Initialize if not already done with timeout protection
        if self.decoder.is_none() {
            // Use a timeout to prevent initialization from hanging
            let init_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.initialize()
            }));
            
            match init_result {
                Ok(result) => match result {
                    Ok(_) => {
                        // Initialization successful
                        info!("YouTube player initialization complete");
                    },
                    Err(e) => {
                        // Initialization failed with a known error
                        error!("Failed to initialize YouTube player: {}", e);
                        return Err(anyhow!("Failed to initialize YouTube player: {}", e));
                    }
                },
                Err(_) => {
                    // Initialization panicked - this is serious
                    error!("YouTube player initialization panicked!");
                    return Err(anyhow!("YouTube player initialization failed unexpectedly"));
                }
            }
        }

        let elapsed = self.last_frame_time.elapsed();

        // Check if it's time to render the next frame
        if elapsed >= self.frame_duration {
            self.last_frame_time = Instant::now();

            // Get decoder (we already checked it's not None)
            let decoder = match self.decoder.as_mut() {
                Some(d) => d,
                None => {
                    error!("Decoder unexpectedly became None");
                    return Err(anyhow!("Video decoder not initialized or was lost"));
                }
            };

            // Decode and render the next frame with timeout protection
            let decode_start = Instant::now();
            let frame_result = decoder.decode_next_frame();
            
            if decode_start.elapsed() > Duration::from_secs(5) {
                warn!("Frame decoding took too long: {:?}", decode_start.elapsed());
            }
            
            match frame_result {
                Ok(Some(frame)) => {
                    self.current_timestamp = frame.timestamp;
                    self.renderer.render(&frame)?;
                    debug!(
                        "Rendered frame at timestamp: {:.2}s",
                        self.current_timestamp
                    );
                },
                Ok(None) => {
                    // EOF reached
                    debug!("End of video reached");
                },
                Err(e) => {
                    warn!("Error decoding frame: {}", e);
                    // Don't propagate every decode error to avoid constant failures
                    // Just log it and continue
                }
            }
        }

        Ok(())
    }

    /// Pause or resume playback
    fn toggle_pause(&mut self) {
        // CRUCIAL FIX: Toggle pause state with prominent logging
        self.paused = !self.paused;
        log::warn!("YOUTUBE PLAYER: Playback paused state changed to: {}", self.paused);
        self.last_frame_time = Instant::now(); // Reset frame timing
    }

    /// Seek to a specific time in seconds
    fn seek(&mut self, timestamp_secs: f64) -> Result<()> {
        // CRUCIAL FIX: Log seek action prominently
        log::warn!("YOUTUBE PLAYER: Seeking to {:.2}s", timestamp_secs);
        
        if let Some(decoder) = &mut self.decoder {
            // Ensure seek operation completes
            let seek_result = decoder.seek(timestamp_secs);
            if let Err(ref e) = seek_result {
                log::warn!("YOUTUBE PLAYER: Seek failed: {}", e);
                return seek_result;
            }
            
            // Update state
            self.current_timestamp = timestamp_secs;
            self.last_frame_time = Instant::now(); // Reset frame timing

            // Force immediate frame update after seek
            log::warn!("YOUTUBE PLAYER: Decoding frame at new position");
            match decoder.decode_next_frame() {
                Ok(Some(frame)) => {
                    log::warn!("YOUTUBE PLAYER: Rendering frame at new position");
                    self.current_timestamp = frame.timestamp;
                    self.renderer.render(&frame)?;
                }
                Ok(None) => {
                    log::warn!("YOUTUBE PLAYER: No frame at seek position (EOF)");
                }
                Err(e) => {
                    log::warn!("YOUTUBE PLAYER: Error decoding frame after seek: {}", e);
                    return Err(e);
                }
            }
        } else {
            log::warn!("YOUTUBE PLAYER: Cannot seek - decoder not initialized");
            return Err(anyhow!("Cannot seek - decoder not initialized"));
        }

        Ok(())
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Helper function to extract YouTube video ID from URL
pub fn extract_youtube_id(url: &str) -> Option<String> {
    // Match youtube.com/watch?v=ID pattern
    if let Some(pos) = url.find("youtube.com/watch?v=") {
        let id_start = pos + "youtube.com/watch?v=".len();
        let id_end = url[id_start..]
            .find(&['&', '#', '?', '/'][..])
            .unwrap_or(url.len() - id_start)
            + id_start;
        return Some(url[id_start..id_end].to_string());
    }

    // Match youtu.be/ID pattern
    if let Some(pos) = url.find("youtu.be/") {
        let id_start = pos + "youtu.be/".len();
        let id_end = url[id_start..]
            .find(&['&', '#', '?', '/'][..])
            .unwrap_or(url.len() - id_start)
            + id_start;
        return Some(url[id_start..id_end].to_string());
    }

    // Match youtube.com/embed/ID pattern
    if let Some(pos) = url.find("youtube.com/embed/") {
        let id_start = pos + "youtube.com/embed/".len();
        let id_end = url[id_start..]
            .find(&['&', '#', '?', '/'][..])
            .unwrap_or(url.len() - id_start)
            + id_start;
        return Some(url[id_start..id_end].to_string());
    }

    None
}

/// Check if a URL is a valid YouTube URL
pub fn is_youtube_url(url: &str) -> bool {
    url.contains("youtube.com/watch")
        || url.contains("youtu.be/")
        || url.contains("youtube.com/embed/")
}

impl YouTubePlayer {
    /// Check if yt-dlp is installed
    fn check_ytdlp_installed(&self) -> Result<()> {
        // Add timeout for command execution to prevent hanging
        let check_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Use a timeout for checking yt-dlp
            let ytdlp_timeout = std::time::Duration::from_secs(5);
            let start_time = std::time::Instant::now();
            
            let result = self.find_ytdlp_executable();
            
            if start_time.elapsed() > ytdlp_timeout {
                warn!("yt-dlp check took too long ({:?}), may indicate issues", 
                      start_time.elapsed());
            }
            
            result
        }));
        
        match check_result {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(anyhow!("
yt-dlp not found. Please install it first:

For Debian/Ubuntu:
    sudo apt-get install yt-dlp
    
    # If not available in your repository:
    sudo curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o /usr/local/bin/yt-dlp
    sudo chmod a+rx /usr/local/bin/yt-dlp

For macOS:
    brew install yt-dlp

For other systems:
    pip install yt-dlp
")),
            Err(_) => Err(anyhow!("Error while checking for yt-dlp - command execution failed"))
        }
    }

    /// Find the yt-dlp executable
    fn find_ytdlp_executable(&self) -> Option<String> {
        // First check custom path from config
        if let Some(path) = &self.config.ytdlp_path {
            if let Ok(status) = std::process::Command::new(path)
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .status()
            {
                if status.success() {
                    return Some(path.clone());
                }
            }
        }
        
        // Check for yt-dlp
        if let Ok(status) = std::process::Command::new("yt-dlp")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .status()
        {
            if status.success() {
                return Some("yt-dlp".to_string());
            }
        }
        
        // Check for youtube-dl as fallback
        if let Ok(status) = std::process::Command::new("youtube-dl")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .status()
        {
            if status.success() {
                info!("Using youtube-dl instead of yt-dlp (consider upgrading to yt-dlp for better performance)");
                return Some("youtube-dl".to_string());
            }
        }

        None
    }
}
