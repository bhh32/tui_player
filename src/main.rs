pub mod commands;
pub mod player;
pub mod renderer;
pub mod test_integration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use commands::cli::Cli;

#[derive(Parser)]
#[command(name = "tui_player")]
#[command(about = "A full video player for the Terminal")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    // Video file to play
    #[arg(short, long)]
    video: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Test the video renderer with a file
    Test {
        /// Video file path to test with
        #[arg(short, long)]
        file: Option<String>,
    },
    /// Run component tests
    TestComponents,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();

    let args = Args::parse();

    match args.command {
        Some(Commands::Test { file }) => {
            // Test mode
            let video_path = if let Some(path) = file {
                path
            } else {
                // Try to create/find test video
                match test_integration::create_test_video().await {
                    Ok(path) => path,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        return Ok(());
                    }
                }
            };

            test_integration::test_video_file(&video_path).await?;
        }
        Some(Commands::TestComponents) => {
            // Component test mode
            test_integration::test_renderer_components().await?;
        }
        None => {
            // Regular mode
            if let Some(video_path) = args.video {
                println!("🎬 Playing video: {video_path}");

                test_integration::test_video_file(&video_path).await?;
            } else {
                println!("TUI Video Player");
                println!("Usage:");
                println!("  tui_player --video <path>   Play a video");
                println!("  tui_player --file <path>    Test with a video file");
                println!("  tui_player test_components  Test components only");
            }
        }
    }

    Ok(())
}
