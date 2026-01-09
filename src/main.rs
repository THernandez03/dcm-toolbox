//! # DCM to JPG Converter
//!
//! A command-line tool to convert DICOM (.dcm) files to JPEG images or MP4 videos.
//!
//! ## Features
//!
//! - Convert single DICOM files or entire directories
//! - Output as JPEG images in a folder or MP4 video
//! - Configurable video frame rate
//! - Force overwrite existing files
//! - Separate different series/cuts into individual folders
//!
//! ## Usage
//!
//! ```bash
//! dcm-converter convert --in <input_folder> --out <output_folder> --split-by <split_by>
//! dcm-converter analyze --in <input_folder>
//! ```
//!
//! The `<output_folder>` will contain subfolders for each series/cut.

mod analyze;
mod convert;
mod utils;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "dcm-converter")]
#[command(about = "Convert DICOM medical images to JPG or video format")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum SplitBy {
    /// Split by SeriesNumber tag (0020,0011)
    SeriesNumber,
    /// Split by SeriesInstanceUID tag (0020,000E)
    SeriesUid,
    /// Split by AcquisitionNumber tag (0020,0012)
    AcquisitionNumber,
    /// Split by SeriesDescription tag (0008,103E)
    Description,
    /// Split by ImageOrientationPatient tag (0020,0037)
    Orientation,
    /// Split by StackID tag (0020,9056)
    StackId,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Convert DICOM files to JPG images or MP4 video
    Convert {
        /// Input folder containing DICOM (.dcm) files
        #[arg(long = "in")]
        input: PathBuf,

        /// Output folder for converted files (JPGs or video files)
        #[arg(long = "out")]
        output: PathBuf,

        /// Generate video output (MP4) instead of JPG images
        #[arg(long)]
        video: bool,

        /// Frames per second for video output
        #[arg(long, default_value_t = 10)]
        fps: u32,

        /// Force clean the output folder without asking for confirmation
        #[arg(long, short = 'f')]
        force: bool,

        /// Split files by series/cut identifier into separate folders
        #[arg(long, short = 's', value_enum, default_value_t = SplitBy::SeriesNumber)]
        split_by: SplitBy,
    },
    /// Analyze DICOM files to find distinguishing tags for different cuts/series
    Analyze {
        /// Input folder containing DICOM (.dcm) files
        #[arg(long = "in")]
        input: PathBuf,

        /// Expected number of groups/series (highlights matching tags in recommendation)
        #[arg(long, short = 'g')]
        expected_groups: Option<usize>,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Convert {
            input,
            output,
            video,
            fps,
            force,
            split_by,
        } => convert::run(&input, &output, video, fps, force, split_by),
        Commands::Analyze {
            input,
            expected_groups,
        } => analyze::run(&input, expected_groups),
    }
}
