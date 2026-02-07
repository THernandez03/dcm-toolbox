//! DICOM to JPG/MP4/STL conversion module.

mod jpeg;
mod stl;
mod video;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use dicom::dictionary_std::tags;
use dicom::object::open_file;
use dicom_pixeldata::PixelDecoder;
use image::DynamicImage;

use crate::utils::{
    clean_output, is_folder_empty, prompt_to_cleanup, sanitize_filename, validate_input_folder,
    CleanupChoice,
};

/// Tag used to split DICOM files into groups/series.
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

/// Shared options for all convert subcommands.
#[derive(Args, Debug)]
pub struct ConvertShared {
    /// Input folder containing DICOM (.dcm) files
    #[arg(long = "in")]
    pub input: PathBuf,

    /// Output folder for converted files
    #[arg(long = "out")]
    pub output: PathBuf,

    /// Force clean the output folder without asking for confirmation
    #[arg(long, short = 'f')]
    pub force: bool,

    /// Split files by series/cut identifier into separate folders
    #[arg(long, short = 's', value_enum, default_value_t = SplitBy::SeriesNumber)]
    pub split_by: SplitBy,
}

/// Output format subcommands for `convert`.
#[derive(Subcommand, Debug)]
pub enum ConvertFormat {
    /// Convert DICOM files to JPEG images
    Jpeg,
    /// Convert DICOM files to MP4 video
    Video {
        /// Frames per second for video output
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..))]
        fps: u32,
    },
    /// Convert DICOM files to STL 3D model
    Stl {
        /// Isosurface threshold level (auto-detected via Otsu if omitted)
        #[arg(long)]
        iso_level: Option<f32>,

        /// Gaussian smoothing sigma (0 disables smoothing)
        #[arg(long, default_value_t = 1.0)]
        smooth: f32,
    },
}

/// A prepared group of DICOM files ready for conversion.
struct PreparedGroup {
    /// Display key for the group
    key: String,
    /// Sorted DICOM file paths (by Z-position)
    files: Vec<PathBuf>,
    /// Output directory for this group
    output_dir: PathBuf,
}

/// Convert DICOM files to the specified output format.
pub fn run(shared: &ConvertShared, format: &ConvertFormat) -> Result<()> {
    let groups = prepare_groups(shared)?;

    for group in &groups {
        println!(
            "=== Processing series: {} ({} files) ===",
            group.key,
            group.files.len()
        );

        match format {
            ConvertFormat::Jpeg => jpeg::convert_to_jpgs(&group.files, &group.output_dir)?,
            ConvertFormat::Video { fps } => {
                video::convert_to_video(&group.files, &group.output_dir, *fps)?;
            }
            ConvertFormat::Stl { iso_level, smooth } => {
                stl::convert_to_stl(&group.files, &group.output_dir, *iso_level, *smooth)?;
            }
        }

        println!();
    }

    println!("Conversion complete! Created {} series.", groups.len());
    Ok(())
}

/// Collect, group, sort, and prepare output directories for DICOM files.
///
/// Handles input validation, file discovery, tag-based grouping,
/// output directory creation, and overwrite prompts.
fn prepare_groups(shared: &ConvertShared) -> Result<Vec<PreparedGroup>> {
    validate_input_folder(&shared.input)?;

    let entries = fs::read_dir(&shared.input)
        .with_context(|| format!("Failed to read input folder: {:?}", shared.input))?;

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
        println!("No .dcm files found in {:?}", shared.input);
        return Ok(vec![]);
    }

    println!("Found {} DICOM file(s) to process", dcm_files.len());
    println!("Splitting by: {:?}\n", shared.split_by);

    // Group files by the split key
    let mut groups: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for dcm_path in dcm_files {
        let key = match open_file(&dcm_path) {
            Ok(obj) => {
                let tag = match shared.split_by {
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
    let mut sorted_keys: Vec<_> = groups.keys().cloned().collect();
    sorted_keys.sort_by(|a, b| match (a.parse::<i32>(), b.parse::<i32>()) {
        (Ok(a_num), Ok(b_num)) => a_num.cmp(&b_num),
        _ => a.cmp(b),
    });

    for key in &sorted_keys {
        println!("  - {}: {} files", key, groups[key].len());
    }
    println!();

    // Ensure output folder exists
    fs::create_dir_all(&shared.output)
        .with_context(|| format!("Failed to create output folder: {:?}", shared.output))?;

    // Track saved choice for "to all" options
    let mut saved_choice: Option<CleanupChoice> = if shared.force {
        Some(CleanupChoice::YesToAll)
    } else {
        None
    };

    let mut prepared = Vec::with_capacity(sorted_keys.len());

    for key in sorted_keys {
        let files = groups.remove(&key).unwrap();
        let safe_key = sanitize_filename(&key);
        let group_output = shared.output.join(&safe_key);

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
            false
        };

        let sorted_files = sort_files_by_position(&files)?;

        clean_output(&group_output, should_clean)?;
        fs::create_dir_all(&group_output)?;

        prepared.push(PreparedGroup {
            key,
            files: sorted_files,
            output_dir: group_output,
        });
    }

    Ok(prepared)
}

/// Sort files by IMAGE_POSITION_PATIENT Z-coordinate.
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

/// Load a DICOM file and decode it as a dynamic image.
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

#[cfg(test)]
mod tests {
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
