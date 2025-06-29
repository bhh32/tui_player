use clap::Parser;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

#[derive(Debug, Parser)]
#[command(author, about, long_about = None)]
pub struct Cli {
    #[arg(
        short,
        long,
        required = false,
        help = "Video file to open TUI Player with."
    )]
    video: Option<String>,
}

pub fn open_tui(video: Option<String>) {
    match video {
        Some(video) => {
            let _video_path = PathBuf::from_str(&video);

            // TODO: Write helper function to start TUI video player
        }
        None => {
            let _open_path = match dirs::home_dir() {
                Some(home) => {
                    let home = home;
                    let videos_dir = home.join("/Videos");
                    videos_dir
                }
                None => {
                    let cwd = Command::new("pwd")
                        .output()
                        .expect("Couldn't get a directory to start TUI Player in");

                    let cwd_string = match String::from_utf8(cwd.stdout) {
                        Ok(cur_dir) => cur_dir.trim().to_string(),
                        Err(_) => ".".to_string(),
                    };

                    PathBuf::from(cwd_string)
                }
            };

            // TODO: Write helper function to start TUI video player
        }
    }
}
