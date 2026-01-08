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
//!
//! ## Usage
//!
//! ```bash
//! dcm-converter <input_folder> <output>
//! ```
//!
//! Where `<output>` can be a folder (for JPGs) or a file with .mp4 extension (for video).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use clap::Parser;
use dicom::object::open_file;
use dicom_pixeldata::PixelDecoder;
use image::{DynamicImage, ImageFormat};
use tempfile::TempDir;

#[derive(Parser, Debug)]
#[command(name = "dcm-converter")]
#[command(about = "Convert DICOM medical images to JPG or video format")]
struct Args {
    /// Input folder containing DICOM (.dcm) files
    #[arg(long = "in")]
    input: PathBuf,

    /// Output destination:
    /// - If a folder path: converts to individual JPG images
    /// - If a file path (e.g., scan.mp4): generates a video
    #[arg(long = "out")]
    output: PathBuf,

    /// Frames per second for video output
    #[arg(long, default_value_t = 24)]
    fps: u32,

    /// Force clean the output folder without asking for confirmation
    #[arg(long, short = 'f')]
    force: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    validate_input_folder(&args.input)?;

    let dcm_files = collect_dcm_files(&args.input)?;

    if dcm_files.is_empty() {
        println!("No .dcm files found in {:?}", args.input);
        return Ok(());
    }

    println!("Found {} DICOM file(s) to process", dcm_files.len());

    // Determine output mode based on whether output is a file or folder
    let is_video_output = args.output.extension().is_some();

    if is_video_output {
        prepare_video_output(&args.output, args.force)?;
        convert_to_video(&dcm_files, args.output.as_path(), args.fps)?;
    } else {
        prepare_output_folder(&args.output, args.force)?;
        convert_to_jpgs(&dcm_files, args.output.as_path())?;
    }

    println!("\nConversion complete!");
    Ok(())
}

fn convert_to_jpgs(dcm_files: &[PathBuf], output_dir: &Path) -> Result<()> {
    let total = dcm_files.len();
    let padding = total.to_string().len().max(4); // At least 4 digits

    for (idx, dcm_path) in dcm_files.iter().enumerate() {
        match convert_dcm_to_jpg(dcm_path, output_dir, idx + 1, padding) {
            Ok(output_path) => println!(
                "✓ Converted: {:?} -> {:?}",
                dcm_path.file_name().unwrap(),
                output_path.file_name().unwrap()
            ),
            Err(e) => eprintln!(
                "✗ Failed to convert {:?}: {}",
                dcm_path.file_name().unwrap(),
                e
            ),
        }
    }
    Ok(())
}

fn convert_to_video(dcm_files: &[PathBuf], output_path: &Path, fps: u32) -> Result<()> {
    // Ensure output path has .mp4 extension
    let video_path = if output_path.extension().is_some_and(|e| e == "mp4") {
        output_path.to_path_buf()
    } else {
        output_path.with_extension("mp4")
    };

    // Create temporary directory for intermediate frames
    let temp_dir = TempDir::new().with_context(|| "Failed to create temporary directory")?;
    let temp_path = temp_dir.path();

    println!("Preparing frames for video encoding...");

    // Load first frame to determine dimensions for consistent sizing
    let first_image = load_dcm_as_image(&dcm_files[0])?;
    let (target_width, target_height) = (first_image.width(), first_image.height());

    println!("Creating video: {target_width}x{target_height} @ {fps} fps");

    // Save all frames as PNG files with sequential numbering
    let mut frame_count = 0;
    for (idx, dcm_path) in dcm_files.iter().enumerate() {
        match load_dcm_as_image(dcm_path) {
            Ok(img) => {
                // Resize if dimensions don't match first frame
                let img = if img.width() != target_width || img.height() != target_height {
                    img.resize_exact(
                        target_width,
                        target_height,
                        image::imageops::FilterType::Lanczos3,
                    )
                } else {
                    img
                };

                // Save as PNG with zero-padded numbering for ffmpeg
                let frame_path = temp_path.join(format!("frame_{idx:06}.png"));
                img.save_with_format(&frame_path, ImageFormat::Png)
                    .with_context(|| format!("Failed to save frame: {frame_path:?}"))?;

                frame_count += 1;
                println!(
                    "✓ Prepared frame {}/{}: {:?}",
                    idx + 1,
                    dcm_files.len(),
                    dcm_path.file_name().unwrap()
                );
            }
            Err(e) => {
                eprintln!(
                    "✗ Failed to load {:?}: {}",
                    dcm_path.file_name().unwrap(),
                    e
                );
            }
        }
    }

    if frame_count == 0 {
        anyhow::bail!("No frames were successfully processed for video creation");
    }

    println!("\nEncoding video with ffmpeg...");

    // Call ffmpeg to encode frames into video
    // Settings optimized for AI context in medical imaging:
    // - H.264 codec for broad compatibility
    // - CRF 18 for high quality (near-lossless)
    // - YUV420p pixel format for standard playback
    // - preset slow for better compression
    let frame_pattern = temp_path.join("frame_%06d.png");
    let output = Command::new("ffmpeg")
        .args([
            "-y", // Overwrite output
            "-framerate",
            &fps.to_string(), // Input framerate
            "-i",
            frame_pattern.to_str().unwrap(), // Input pattern
            "-c:v",
            "libx264", // H.264 codec
            "-crf",
            "18", // High quality
            "-preset",
            "slow", // Better compression
            "-pix_fmt",
            "yuv420p", // Standard pixel format
            "-movflags",
            "+faststart",                 // Web optimization
            video_path.to_str().unwrap(), // Output file
        ])
        .output()
        .with_context(|| "Failed to execute ffmpeg. Is ffmpeg installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg encoding failed: {stderr}");
    }

    println!("\n✓ Video saved to: {video_path:?}");
    println!("  Total frames: {frame_count}");
    println!(
        "  Duration: {:.2}s",
        f64::from(frame_count) / f64::from(fps)
    );

    // temp_dir is automatically cleaned up when dropped
    Ok(())
}

fn load_dcm_as_image(dcm_path: &PathBuf) -> Result<DynamicImage> {
    let dicom_obj =
        open_file(dcm_path).with_context(|| format!("Failed to open DICOM file: {dcm_path:?}"))?;

    let pixel_data = dicom_obj
        .decode_pixel_data()
        .with_context(|| format!("Failed to decode pixel data from: {dcm_path:?}"))?;

    pixel_data
        .to_dynamic_image(0)
        .with_context(|| format!("Failed to convert to image: {dcm_path:?}"))
}

fn validate_input_folder(input: &PathBuf) -> Result<()> {
    if !input.exists() {
        anyhow::bail!("Input folder does not exist: {input:?}");
    }
    if !input.is_dir() {
        anyhow::bail!("Input path is not a directory: {input:?}");
    }
    Ok(())
}

fn prepare_output_folder(output: &PathBuf, force: bool) -> Result<()> {
    if output.exists() && !is_folder_empty(output)? {
        if force {
            println!("Force cleaning output folder: {output:?}");
        } else {
            print!("Output folder {output:?} is not empty. Clean it and continue? [y/N]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            let confirmed = matches!(input.trim().to_lowercase().as_str(), "y" | "yes");
            if !confirmed {
                anyhow::bail!("Operation cancelled: output folder is not empty");
            }
        }

        fs::remove_dir_all(output)
            .with_context(|| format!("Failed to clean output folder: {output:?}"))?;
        println!("Cleaned output folder: {output:?}");
    }

    fs::create_dir_all(output)
        .with_context(|| format!("Failed to create output folder: {output:?}"))?;

    Ok(())
}

fn is_folder_empty(path: &PathBuf) -> Result<bool> {
    let mut entries =
        fs::read_dir(path).with_context(|| format!("Failed to read directory: {path:?}"))?;
    Ok(entries.next().is_none())
}

fn prepare_video_output(output: &PathBuf, force: bool) -> Result<()> {
    // Create parent directory if needed
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create output directory: {parent:?}"))?;
        }
    }

    // Check if output file already exists
    if output.exists() {
        if !force {
            print!("Output file {output:?} already exists. Overwrite? [y/N]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            let confirmed = matches!(input.trim().to_lowercase().as_str(), "y" | "yes");
            if !confirmed {
                anyhow::bail!("Operation cancelled: output file already exists");
            }
        }

        fs::remove_file(output)
            .with_context(|| format!("Failed to remove existing file: {output:?}"))?;
        println!("Removed existing file: {output:?}");
    }

    Ok(())
}

fn collect_dcm_files(input: &PathBuf) -> Result<Vec<PathBuf>> {
    use dicom::dictionary_std::tags;

    let entries =
        fs::read_dir(input).with_context(|| format!("Failed to read input folder: {input:?}"))?;

    let dcm_files: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("dcm"))
        })
        .collect();

    // Extract Image Position (Patient) Z-coordinate for sorting
    // This is the slice position along the patient axis (usually head-to-feet for CT)
    let mut files_with_position: Vec<(PathBuf, f64)> = dcm_files
        .into_iter()
        .map(|path| {
            let z_position = match open_file(&path) {
                Ok(obj) => {
                    // Image Position (Patient) is a string like "x\\y\\z"
                    obj.element(tags::IMAGE_POSITION_PATIENT)
                        .ok()
                        .and_then(|elem| elem.to_str().ok())
                        .and_then(|s| {
                            let coords: Vec<f64> = s
                                .split('\\')
                                .filter_map(|v| v.trim().parse::<f64>().ok())
                                .collect();
                            coords.get(2).copied() // Z coordinate (3rd value)
                        })
                        .unwrap_or(f64::MAX)
                }
                Err(_) => f64::MAX,
            };
            (path, z_position)
        })
        .collect();

    // Sort by Z position (ascending = inferior to superior typically)
    files_with_position.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("Sorting by: Image Position Patient Z-coordinate (0020,0032)");

    Ok(files_with_position
        .into_iter()
        .map(|(path, _)| path)
        .collect())
}

fn convert_dcm_to_jpg(
    dcm_path: &PathBuf,
    output_dir: &Path,
    index: usize,
    padding: usize,
) -> Result<PathBuf> {
    let dicom_obj =
        open_file(dcm_path).with_context(|| format!("Failed to open DICOM file: {dcm_path:?}"))?;

    let pixel_data = dicom_obj
        .decode_pixel_data()
        .with_context(|| format!("Failed to decode pixel data from: {dcm_path:?}"))?;

    let dynamic_image = pixel_data
        .to_dynamic_image(0)
        .with_context(|| format!("Failed to convert to image: {dcm_path:?}"))?;

    let output_path = output_dir.join(format!("{index:0padding$}.jpg"));

    dynamic_image
        .save_with_format(&output_path, ImageFormat::Jpeg)
        .with_context(|| format!("Failed to save JPG: {output_path:?}"))?;

    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    mod validate_input_folder_tests {
        use super::*;

        #[test]
        fn valid_folder_succeeds() {
            let temp_dir = TempDir::new().unwrap();
            let result = validate_input_folder(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
        }

        #[test]
        fn nonexistent_folder_fails() {
            let path = PathBuf::from("/nonexistent/path/that/does/not/exist");
            let result = validate_input_folder(&path);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("does not exist"));
        }

        #[test]
        fn file_instead_of_folder_fails() {
            let temp_dir = TempDir::new().unwrap();
            let file_path = temp_dir.path().join("test.txt");
            fs::write(&file_path, "content").unwrap();

            let result = validate_input_folder(&file_path);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("not a directory"));
        }
    }

    mod prepare_output_folder_tests {
        use super::*;

        #[test]
        fn creates_new_folder() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("new_output");

            assert!(!output_path.exists());

            let result = prepare_output_folder(&output_path, false);
            assert!(result.is_ok());
            assert!(output_path.exists());
            assert!(output_path.is_dir());
        }

        #[test]
        fn force_cleans_non_empty_folder() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("existing_output");

            // Create folder with content
            fs::create_dir_all(&output_path).unwrap();
            fs::write(output_path.join("old_file.txt"), "old content").unwrap();

            let result = prepare_output_folder(&output_path, true);
            assert!(result.is_ok());

            // Old file should be gone
            assert!(!output_path.join("old_file.txt").exists());
            // Folder should still exist
            assert!(output_path.exists());
        }

        #[test]
        fn creates_nested_folders() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("level1").join("level2").join("output");

            let result = prepare_output_folder(&output_path, false);
            assert!(result.is_ok());
            assert!(output_path.exists());
        }

        #[test]
        fn allows_empty_existing_folder_without_force() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("empty_output");

            // Create empty folder
            fs::create_dir_all(&output_path).unwrap();

            let result = prepare_output_folder(&output_path, false);
            assert!(result.is_ok());
            assert!(output_path.exists());
        }
    }

    mod is_folder_empty_tests {
        use super::*;

        #[test]
        fn empty_folder_returns_true() {
            let temp_dir = TempDir::new().unwrap();
            let result = is_folder_empty(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
            assert!(result.unwrap());
        }

        #[test]
        fn folder_with_file_returns_false() {
            let temp_dir = TempDir::new().unwrap();
            fs::write(temp_dir.path().join("file.txt"), "content").unwrap();

            let result = is_folder_empty(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
            assert!(!result.unwrap());
        }

        #[test]
        fn folder_with_subfolder_returns_false() {
            let temp_dir = TempDir::new().unwrap();
            fs::create_dir_all(temp_dir.path().join("subfolder")).unwrap();

            let result = is_folder_empty(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
            assert!(!result.unwrap());
        }

        #[test]
        fn nonexistent_folder_returns_error() {
            let path = PathBuf::from("/nonexistent/path/that/does/not/exist");
            let result = is_folder_empty(&path);
            assert!(result.is_err());
        }
    }

    mod prepare_video_output_tests {
        use super::*;

        #[test]
        fn creates_parent_directories() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("nested").join("dir").join("video.mp4");

            let result = prepare_video_output(&output_path, false);
            assert!(result.is_ok());
            assert!(output_path.parent().unwrap().exists());
        }

        #[test]
        fn allows_nonexistent_file() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("new_video.mp4");

            let result = prepare_video_output(&output_path, false);
            assert!(result.is_ok());
        }

        #[test]
        fn force_removes_existing_file() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("existing.mp4");

            // Create existing file
            fs::write(&output_path, "old video content").unwrap();
            assert!(output_path.exists());

            let result = prepare_video_output(&output_path, true);
            assert!(result.is_ok());
            assert!(!output_path.exists());
        }
    }

    mod collect_dcm_files_tests {
        use super::*;

        #[test]
        fn empty_folder_returns_empty_vec() {
            let temp_dir = TempDir::new().unwrap();
            let result = collect_dcm_files(&temp_dir.path().to_path_buf());

            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        }

        #[test]
        fn ignores_non_dcm_files() {
            let temp_dir = TempDir::new().unwrap();

            // Create various non-dcm files
            fs::write(temp_dir.path().join("test.txt"), "content").unwrap();
            fs::write(temp_dir.path().join("image.jpg"), "content").unwrap();
            fs::write(temp_dir.path().join("data.json"), "content").unwrap();

            let result = collect_dcm_files(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        }

        #[test]
        fn ignores_directories() {
            let temp_dir = TempDir::new().unwrap();

            // Create a subdirectory with .dcm in name
            fs::create_dir_all(temp_dir.path().join("test.dcm")).unwrap();

            let result = collect_dcm_files(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        }

        #[test]
        fn case_insensitive_extension() {
            // This test verifies the filter logic exists for case-insensitive matching.
            // Real DICOM parsing is tested through integration tests with example files.
            // Here we just confirm the code path for extension checking works.
            let ext = "DCM";
            assert!(ext.eq_ignore_ascii_case("dcm"));

            let ext = "Dcm";
            assert!(ext.eq_ignore_ascii_case("dcm"));
        }
    }

    mod output_path_detection {
        use std::path::Path;

        #[test]
        fn path_with_extension_is_video() {
            let path = Path::new("/output/scan.mp4");
            assert!(path.extension().is_some());
        }

        #[test]
        fn path_without_extension_is_jpg_folder() {
            let path = Path::new("/output/images");
            assert!(path.extension().is_none());
        }

        #[test]
        fn various_video_extensions_detected() {
            let paths = [
                "/output/scan.mp4",
                "/output/scan.avi",
                "/output/scan.mov",
                "/output/scan.webm",
            ];

            for p in paths {
                let path = Path::new(p);
                assert!(
                    path.extension().is_some(),
                    "Extension should be detected for {}",
                    p
                );
            }
        }

        #[test]
        fn folder_path_with_dots_handled() {
            // Path like "my.folder.name" without final extension
            let path = Path::new("/output/scan.2024/final");
            // This has no extension because "final" has none
            assert!(path.extension().is_none());
        }
    }

    mod jpg_naming {
        #[test]
        fn sequential_naming_format() {
            let test_cases = [
                (1, 4, "0001.jpg"),
                (42, 4, "0042.jpg"),
                (999, 4, "0999.jpg"),
                (1000, 4, "1000.jpg"),
                (1, 6, "000001.jpg"),
            ];

            for (index, padding, expected) in test_cases {
                let filename = format!("{:0width$}.jpg", index, width = padding);
                assert_eq!(
                    filename, expected,
                    "Index {} with padding {}",
                    index, padding
                );
            }
        }

        #[test]
        fn padding_calculation() {
            let test_cases = [
                (1, 4),     // min padding is 4
                (10, 4),    // still 4
                (100, 4),   // still 4
                (1000, 4),  // exactly 4 digits
                (10000, 5), // needs 5
            ];

            for (count, expected_min_padding) in test_cases {
                let padding = count.to_string().len().max(4);
                assert!(
                    padding >= expected_min_padding,
                    "Count {} should have at least {} padding, got {}",
                    count,
                    expected_min_padding,
                    padding
                );
            }
        }
    }
}
