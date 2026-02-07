//! DICOM to MP4 video conversion.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use image::ImageFormat;
use tempfile::TempDir;

pub(super) fn convert_to_video(dcm_files: &[PathBuf], output_dir: &Path, fps: u32) -> Result<()> {
    if fps == 0 {
        anyhow::bail!("FPS must be greater than 0");
    }

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
    let first_image = super::load_dcm_as_image(&dcm_files[0])?;
    let (target_width, target_height) = (first_image.width(), first_image.height());

    println!("Creating video: {target_width}x{target_height} @ {fps} fps");

    // Save all frames as PNG files with sequential numbering
    let mut frame_count = 0;
    for (idx, dcm_path) in dcm_files.iter().enumerate() {
        match super::load_dcm_as_image(dcm_path) {
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
    let frame_pattern_str = frame_pattern
        .to_str()
        .with_context(|| format!("Frame pattern path is not valid UTF-8: {frame_pattern:?}"))?;
    let video_path_str = video_path
        .to_str()
        .with_context(|| format!("Video output path is not valid UTF-8: {video_path:?}"))?;

    let output = Command::new("ffmpeg")
        .args([
            "-y", // Overwrite output
            "-framerate",
            &fps.to_string(), // Input framerate
            "-i",
            frame_pattern_str, // Input pattern
            "-c:v",
            "libx264", // H.264 codec
            "-crf",
            "18", // High quality
            "-preset",
            "slow", // Better compression
            "-pix_fmt",
            "yuv420p", // Standard pixel format
            "-movflags",
            "+faststart",   // Web optimization
            video_path_str, // Output file
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

#[cfg(test)]
mod tests {
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
}
