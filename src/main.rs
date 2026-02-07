//! # DCM Toolbox
//!
//! A command-line tool to convert DICOM (.dcm) files to JPEG images, MP4 videos,
//! or STL 3D models.
//!
//! ## Features
//!
//! - Convert DICOM files to JPEG images, MP4 video, or STL 3D models
//! - Analyze DICOM metadata to identify optimal splitting strategies
//! - Split output by series/groups based on configurable DICOM tags
//! - Automatic Otsu thresholding for STL isosurface extraction
//! - Configurable Gaussian smoothing for 3D model generation
//!
//! ## Usage
//!
//! ```bash
//! dcm-toolbox convert --in <input> --out <output> --split-by <tag> jpeg
//! dcm-toolbox convert --in <input> --out <output> video --fps 10
//! dcm-toolbox convert --in <input> --out <output> stl --smooth 1.0
//! dcm-toolbox analyze --in <input_folder>
//! ```
//!
//! The `<output>` folder will contain subfolders for each series/group.

mod analyze;
mod convert;
mod utils;

use anyhow::Result;
use clap::{Parser, Subcommand};

use convert::{ConvertFormat, ConvertShared};

#[derive(Parser, Debug)]
#[command(name = "dcm-toolbox")]
#[command(about = "Convert DICOM medical images to JPG, video, or 3D model format")]
struct CliArgs {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Convert DICOM files to JPG images, MP4 video, or STL 3D model
    Convert {
        #[command(flatten)]
        shared: ConvertShared,

        #[command(subcommand)]
        format: ConvertFormat,
    },
    /// Analyze DICOM files to find distinguishing tags for different cuts/series
    Analyze {
        #[command(flatten)]
        args: analyze::AnalyzeArgs,
    },
}

fn main() -> Result<()> {
    let args = CliArgs::parse();

    match args.command {
        Commands::Convert { shared, format } => convert::run(&shared, &format),
        Commands::Analyze { args } => analyze::run(&args),
    }
}
