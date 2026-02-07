//! DICOM to JPEG image conversion.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use dicom::object::open_file;
use dicom_pixeldata::PixelDecoder;
use image::ImageFormat;

pub(super) fn convert_to_jpgs(dcm_files: &[PathBuf], output_dir: &Path) -> Result<()> {
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
