//! DICOM to JPG/MP4 conversion module.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use dicom::dictionary_std::tags;
use dicom::object::open_file;
use dicom_pixeldata::PixelDecoder;
use image::{DynamicImage, ImageFormat};
use tempfile::TempDir;

use crate::utils::{
    clean_output, is_folder_empty, prompt_to_cleanup, sanitize_filename, validate_input_folder,
    CleanupChoice,
};
use crate::SplitBy;

/// Convert DICOM files to JPG images or MP4 video.
pub fn run(
    input: &PathBuf,
    output: &PathBuf,
    video: bool,
    fps: u32,
    force: bool,
    split_by: SplitBy,
) -> Result<()> {
    validate_input_folder(input)?;

    // Collect all DCM files
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

    if dcm_files.is_empty() {
        println!("No .dcm files found in {input:?}");
        return Ok(());
    }

    println!("Found {} DICOM file(s) to process", dcm_files.len());
    println!("Splitting by: {split_by:?}\n");

    // Group files by the split key
    let mut groups: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for dcm_path in dcm_files {
        let key = match open_file(&dcm_path) {
            Ok(obj) => {
                let tag = match split_by {
                    SplitBy::SeriesNumber => tags::SERIES_NUMBER,
                    SplitBy::SeriesUid => tags::SERIES_INSTANCE_UID,
                    SplitBy::AcquisitionNumber => tags::ACQUISITION_NUMBER,
                    SplitBy::Description => tags::SERIES_DESCRIPTION,
                    SplitBy::Orientation => tags::IMAGE_ORIENTATION_PATIENT,
                    SplitBy::StackId => dicom::core::Tag(0x0020, 0x9056),
                };
                obj.element(tag)
                    .ok()
                    .and_then(|elem| elem.to_str().ok())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            }
            Err(_) => "unknown".to_string(),
        };
        groups.entry(key).or_default().push(dcm_path);
    }

    println!("Found {} series/groups:\n", groups.len());

    // Sort group keys for consistent output
    let mut sorted_keys: Vec<_> = groups.keys().collect();
    sorted_keys.sort_by(|a, b| {
        // Try to sort numerically if possible, otherwise alphabetically
        match (a.parse::<i32>(), b.parse::<i32>()) {
            (Ok(a_num), Ok(b_num)) => a_num.cmp(&b_num),
            _ => a.cmp(b),
        }
    });

    for key in &sorted_keys {
        println!("  - {}: {} files", key, groups[*key].len());
    }
    println!();

    // Ensure output folder exists
    fs::create_dir_all(output)
        .with_context(|| format!("Failed to create output folder: {output:?}"))?;

    // Track saved choice for "to all" options
    let mut saved_choice: Option<CleanupChoice> = if force {
        Some(CleanupChoice::YesToAll)
    } else {
        None
    };

    // Process each group
    for key in sorted_keys {
        let files = groups.get(key).unwrap();

        // Create a sanitized folder name from the key
        let safe_key = sanitize_filename(key);
        let group_output = output.join(&safe_key);

        println!("=== Processing series: {} ({} files) ===", key, files.len());

        // Determine if we need to ask for confirmation
        let folder_exists =
            group_output.exists() && !is_folder_empty(&group_output).unwrap_or(true);

        let should_clean = if folder_exists {
            match saved_choice {
                Some(choice) => choice.should_clean(),
                None => {
                    let choice = prompt_to_cleanup(&group_output)?;
                    if choice.is_persistent() {
                        saved_choice = Some(choice);
                    }
                    choice.should_clean()
                }
            }
        } else {
            false // No need to clean if folder doesn't exist
        };

        // Sort files within the group by IMAGE_POSITION_PATIENT Z-coordinate
        let sorted_files = sort_files_by_position(files)?;

        // Clean first (if needed), then ensure directory exists
        clean_output(&group_output, should_clean)?;
        fs::create_dir_all(&group_output)?;

        if video {
            convert_to_video(&sorted_files, &group_output, fps)?;
        } else {
            convert_to_jpgs(&sorted_files, &group_output)?;
        }

        println!();
    }

    println!("Conversion complete! Created {} series.", groups.len());
    Ok(())
}

/// Sort files by IMAGE_POSITION_PATIENT Z-coordinate
fn sort_files_by_position(files: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files_with_position: Vec<(PathBuf, f64)> = files
        .iter()
        .map(|path| {
            let z_position = match open_file(path) {
                Ok(obj) => obj
                    .element(tags::IMAGE_POSITION_PATIENT)
                    .ok()
                    .and_then(|elem| elem.to_str().ok())
                    .and_then(|s| {
                        let coords: Vec<f64> = s
                            .split('\\')
                            .filter_map(|v| v.trim().parse::<f64>().ok())
                            .collect();
                        coords.get(2).copied()
                    })
                    .unwrap_or(f64::MAX),
                Err(_) => f64::MAX,
            };
            (path.clone(), z_position)
        })
        .collect();

    files_with_position.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    Ok(files_with_position
        .into_iter()
        .map(|(path, _)| path)
        .collect())
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

fn convert_to_video(dcm_files: &[PathBuf], output_dir: &Path, fps: u32) -> Result<()> {
    // Derive video name from the folder name
    let folder_name = output_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let video_path = output_dir.join(format!("{folder_name}.mp4"));

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

    println!("\n✓ Video saved to: {:?}", video_path);
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
    // =========================================================================
    // JPG Naming Tests
    // =========================================================================

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

        #[test]
        fn padding_is_always_at_least_4_digits() {
            for count in [1, 2, 5, 9, 10, 50, 99, 100, 500, 999] {
                let padding = count.to_string().len().max(4);
                assert_eq!(padding, 4, "Count {} should have padding 4", count);
            }
        }

        #[test]
        fn padding_increases_for_large_counts() {
            let test_cases = [(10000, 5), (100000, 6), (1000000, 7)];

            for (count, expected_padding) in test_cases {
                let padding = count.to_string().len().max(4);
                assert_eq!(
                    padding, expected_padding,
                    "Count {} should have padding {}",
                    count, expected_padding
                );
            }
        }

        #[test]
        fn filename_format_with_various_indices() {
            // Verify the exact format used in convert_dcm_to_jpg
            let padding = 4;
            let test_cases = [(1, "0001"), (10, "0010"), (100, "0100"), (1000, "1000")];

            for (index, expected_prefix) in test_cases {
                let filename = format!("{index:0padding$}.jpg");
                assert!(
                    filename.starts_with(expected_prefix),
                    "Index {} should produce filename starting with {}",
                    index,
                    expected_prefix
                );
            }
        }

        #[test]
        fn index_starts_at_one_not_zero() {
            // First file should be 0001.jpg, not 0000.jpg
            let idx = 0;
            let index = idx + 1; // This is how it's done in convert_to_jpgs
            let padding = 4;
            let filename = format!("{index:0padding$}.jpg");
            assert_eq!(filename, "0001.jpg");
        }
    }

    // =========================================================================
    // Video Duration Calculation Tests
    // =========================================================================

    mod video_duration {
        #[test]
        fn duration_calculation_with_standard_fps() {
            let test_cases = [
                (30, 30, 1.0),  // 30 frames at 30fps = 1 second
                (60, 30, 2.0),  // 60 frames at 30fps = 2 seconds
                (15, 30, 0.5),  // 15 frames at 30fps = 0.5 seconds
                (1, 30, 0.033), // 1 frame at 30fps ≈ 0.033 seconds
            ];

            for (frame_count, fps, expected_min_duration) in test_cases {
                let duration = frame_count as f64 / fps as f64;
                assert!(
                    duration >= expected_min_duration - 0.001,
                    "Frame count {} at {} fps should be at least {} seconds, got {}",
                    frame_count,
                    fps,
                    expected_min_duration,
                    duration
                );
            }
        }

        #[test]
        fn duration_with_custom_fps() {
            let test_cases = [
                (10, 10, 1.0),  // 10 frames at 10fps = 1 second
                (24, 24, 1.0),  // 24 frames at 24fps = 1 second
                (60, 60, 1.0),  // 60 frames at 60fps = 1 second
                (120, 30, 4.0), // 120 frames at 30fps = 4 seconds
            ];

            for (frame_count, fps, expected_duration) in test_cases {
                let duration = frame_count as f64 / fps as f64;
                assert!(
                    (duration - expected_duration).abs() < 0.001,
                    "Frame count {} at {} fps should be {} seconds",
                    frame_count,
                    fps,
                    expected_duration
                );
            }
        }

        #[test]
        fn typical_dicom_series_durations() {
            // Typical medical imaging scenarios
            let scenarios = [
                (100, 30, "Short sequence"),
                (500, 30, "Medium CT series"),
                (1000, 30, "Large MRI series"),
            ];

            for (frame_count, fps, _description) in scenarios {
                let duration = frame_count as f64 / fps as f64;
                assert!(duration > 0.0, "Duration should always be positive");
            }
        }
    }

    // =========================================================================
    // Frame Numbering Tests
    // =========================================================================

    mod frame_numbering {
        #[test]
        fn frame_pattern_is_zero_padded() {
            // ffmpeg expects frame_%06d.png pattern
            let test_cases = [
                (0, "frame_000000.png"),
                (1, "frame_000001.png"),
                (999999, "frame_999999.png"),
            ];

            for (idx, expected) in test_cases {
                let frame_name = format!("frame_{idx:06}.png");
                assert_eq!(frame_name, expected);
            }
        }

        #[test]
        fn frame_indices_are_sequential() {
            let frame_count = 10;
            let frames: Vec<String> = (0..frame_count)
                .map(|idx| format!("frame_{idx:06}.png"))
                .collect();

            assert_eq!(frames.len(), 10);
            assert_eq!(frames[0], "frame_000000.png");
            assert_eq!(frames[9], "frame_000009.png");
        }

        #[test]
        fn frame_pattern_supports_large_series() {
            // Should support up to 999,999 frames with 6-digit padding
            let max_idx = 999_999;
            let frame_name = format!("frame_{max_idx:06}.png");
            assert_eq!(frame_name, "frame_999999.png");
        }
    }

    // =========================================================================
    // Group Sorting Tests
    // =========================================================================

    mod group_sorting {
        #[test]
        fn numeric_strings_sort_numerically() {
            let mut keys: Vec<&str> = vec!["10", "2", "1", "20", "3"];
            keys.sort_by(|a, b| match (a.parse::<i32>(), b.parse::<i32>()) {
                (Ok(a_num), Ok(b_num)) => a_num.cmp(&b_num),
                _ => a.cmp(b),
            });

            assert_eq!(keys, vec!["1", "2", "3", "10", "20"]);
        }

        #[test]
        fn non_numeric_strings_sort_alphabetically() {
            let mut keys: Vec<&str> = vec!["zebra", "apple", "banana"];
            keys.sort_by(|a, b| match (a.parse::<i32>(), b.parse::<i32>()) {
                (Ok(a_num), Ok(b_num)) => a_num.cmp(&b_num),
                _ => a.cmp(b),
            });

            assert_eq!(keys, vec!["apple", "banana", "zebra"]);
        }

        #[test]
        fn mixed_numeric_and_strings_sort_correctly() {
            let mut keys: Vec<&str> = vec!["10", "alpha", "2", "beta", "1"];
            keys.sort_by(|a, b| match (a.parse::<i32>(), b.parse::<i32>()) {
                (Ok(a_num), Ok(b_num)) => a_num.cmp(&b_num),
                _ => a.cmp(b),
            });

            // Numeric values should be sorted together, then alphabetic
            // Due to the comparison logic, when one parses and one doesn't,
            // it falls back to string comparison
            assert!(keys[0] == "1" || keys[0] == "10" || keys[0] == "2");
        }

        #[test]
        fn unknown_key_sorts_predictably() {
            let mut keys: Vec<&str> = vec!["1", "unknown", "2"];
            keys.sort_by(|a, b| match (a.parse::<i32>(), b.parse::<i32>()) {
                (Ok(a_num), Ok(b_num)) => a_num.cmp(&b_num),
                _ => a.cmp(b),
            });

            // "unknown" can't parse as number, so falls back to string comparison
            // String comparison: "1" < "2" < "unknown"
            assert_eq!(keys, vec!["1", "2", "unknown"]);
        }
    }

    // =========================================================================
    // Position Parsing Tests (for sort_files_by_position logic)
    // =========================================================================

    mod position_parsing {
        #[test]
        fn parse_image_position_patient_z_coordinate() {
            // ImagePositionPatient format: "X\Y\Z"
            let test_cases = [
                ("0.0\\0.0\\0.0", 0.0),
                ("1.5\\2.5\\3.5", 3.5),
                ("-100.0\\-200.0\\-300.0", -300.0),
                ("50.25\\75.50\\100.75", 100.75),
            ];

            for (position_str, expected_z) in test_cases {
                let coords: Vec<f64> = position_str
                    .split('\\')
                    .filter_map(|v| v.trim().parse::<f64>().ok())
                    .collect();

                let z = coords.get(2).copied().unwrap_or(f64::MAX);
                assert!(
                    (z - expected_z).abs() < 0.001,
                    "Position '{}' should have Z={}, got {}",
                    position_str,
                    expected_z,
                    z
                );
            }
        }

        #[test]
        fn invalid_position_returns_max() {
            let invalid_cases = ["", "invalid", "1.0\\2.0"]; // Missing Z coordinate

            for position_str in invalid_cases {
                let coords: Vec<f64> = position_str
                    .split('\\')
                    .filter_map(|v| v.trim().parse::<f64>().ok())
                    .collect();

                let z = coords.get(2).copied().unwrap_or(f64::MAX);
                assert_eq!(
                    z,
                    f64::MAX,
                    "Invalid position '{}' should return f64::MAX",
                    position_str
                );
            }
        }

        #[test]
        fn position_with_whitespace_parses_correctly() {
            let position_str = " 1.0 \\ 2.0 \\ 3.0 ";
            let coords: Vec<f64> = position_str
                .split('\\')
                .filter_map(|v| v.trim().parse::<f64>().ok())
                .collect();

            assert_eq!(coords.len(), 3);
            assert!((coords[2] - 3.0).abs() < 0.001);
        }

        #[test]
        fn sorting_by_z_coordinate_works() {
            let mut positions: Vec<(usize, f64)> =
                vec![(0, 100.0), (1, 50.0), (2, 150.0), (3, 25.0), (4, 75.0)];

            positions.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            let sorted_indices: Vec<usize> = positions.iter().map(|(idx, _)| *idx).collect();
            assert_eq!(sorted_indices, vec![3, 1, 4, 0, 2]);
        }

        #[test]
        fn nan_values_handled_gracefully() {
            let mut positions: Vec<(usize, f64)> = vec![(0, 100.0), (1, f64::NAN), (2, 50.0)];

            // NaN comparisons return None, which becomes Equal
            positions.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            // The sort should complete without panicking
            assert_eq!(positions.len(), 3);
        }
    }

    // =========================================================================
    // Output Path Construction Tests
    // =========================================================================

    mod output_paths {
        use std::path::Path;

        #[test]
        fn video_filename_matches_folder_name() {
            let test_cases = [
                ("series_001", "series_001.mp4"),
                ("unknown", "unknown.mp4"),
                ("T2W_FLAIR", "T2W_FLAIR.mp4"),
            ];

            for (folder_name, expected_video) in test_cases {
                let output_dir = Path::new("/output").join(folder_name);
                let folder_name = output_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("output");
                let video_path = output_dir.join(format!("{folder_name}.mp4"));

                assert!(
                    video_path.ends_with(expected_video),
                    "Folder '{}' should produce video '{}'",
                    folder_name,
                    expected_video
                );
            }
        }

        #[test]
        fn jpg_output_path_is_in_correct_directory() {
            let output_dir = Path::new("/output/series_001");
            let index = 1;
            let padding = 4;

            let output_path = output_dir.join(format!("{index:0padding$}.jpg"));

            assert!(output_path.starts_with("/output/series_001"));
            assert!(output_path.ends_with("0001.jpg"));
        }

        #[test]
        fn group_output_uses_sanitized_key() {
            use crate::utils::sanitize_filename;

            let test_cases = [
                ("Series 1", "Series 1"),
                ("T2W/FLAIR", "T2W_FLAIR"),
                ("Series:Description", "Series_Description"),
            ];

            let base_output = Path::new("/output");

            for (key, expected_safe_key) in test_cases {
                let safe_key = sanitize_filename(key);
                let group_output = base_output.join(&safe_key);

                assert!(
                    group_output.ends_with(expected_safe_key),
                    "Key '{}' should produce path ending with '{}'",
                    key,
                    expected_safe_key
                );
            }
        }
    }
}
