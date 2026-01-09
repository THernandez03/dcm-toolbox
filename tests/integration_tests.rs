//! Integration tests for dcm-converter CLI.
//!
//! These tests verify the end-to-end behavior of the CLI tool.
//!
//! ## Output Structure
//!
//! The converter creates subfolders per series/group:
//! - JPG mode: `{output}/{series}/{0001.jpg, 0002.jpg, ...}`
//! - Video mode: `{output}/{series}/{series}.mp4`

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

/// Helper to get the path to the test binary
fn binary_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    path.pop(); // Remove deps
    path.push("dcm-converter");
    path
}

/// Helper to get the example folder path
fn example_folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("example")
}

/// Helper to run the CLI with given arguments (prepends 'convert' subcommand)
fn run_cli(args: &[&str]) -> std::process::Output {
    let mut full_args = vec!["convert"];
    full_args.extend(args);
    Command::new(binary_path())
        .args(&full_args)
        .output()
        .expect("Failed to execute command")
}

/// Check if ffmpeg is available
fn ffmpeg_available() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

/// Count files with given extension in a directory (recursively)
fn count_files_with_extension(dir: &PathBuf, ext: &str) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                count += count_files_with_extension(&path, ext);
            } else if path.extension().is_some_and(|e| e == ext) {
                count += 1;
            }
        }
    }
    count
}

/// Get all subdirectories in a directory
fn get_subdirs(dir: &PathBuf) -> Vec<PathBuf> {
    if !dir.exists() {
        return vec![];
    }
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

// =============================================================================
// CLI Arguments Tests
// =============================================================================

mod cli_args {
    use super::*;

    #[test]
    fn missing_input_arg_shows_error() {
        let output = Command::new(binary_path())
            .args(["convert", "--out", "/tmp/out"])
            .output()
            .expect("Failed to execute command");

        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--in"));
    }

    #[test]
    fn missing_output_arg_shows_error() {
        let output = Command::new(binary_path())
            .args(["convert", "--in", "./example"])
            .output()
            .expect("Failed to execute command");

        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--out"));
    }

    #[test]
    fn nonexistent_input_folder_fails() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        let output = run_cli(&[
            "--in",
            "/nonexistent/folder/that/does/not/exist",
            "--out",
            output_path.to_str().unwrap(),
        ]);

        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("does not exist") || stderr.contains("Error"),
            "Expected error about nonexistent folder: {}",
            stderr
        );
    }

    #[test]
    fn help_flag_shows_usage() {
        let output = Command::new(binary_path())
            .args(["convert", "--help"])
            .output()
            .expect("Failed to execute command");

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("--in"), "Should show --in option");
        assert!(stdout.contains("--out"), "Should show --out option");
        assert!(stdout.contains("--video"), "Should show --video option");
        assert!(stdout.contains("--fps"), "Should show --fps option");
        assert!(stdout.contains("--force"), "Should show --force option");
        assert!(
            stdout.contains("--split-by"),
            "Should show --split-by option"
        );
    }

    #[test]
    fn short_flags_shown_in_help() {
        let output = Command::new(binary_path())
            .args(["convert", "--help"])
            .output()
            .expect("Failed to execute command");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("-i"), "Should show -i short flag");
        assert!(stdout.contains("-o"), "Should show -o short flag");
        assert!(stdout.contains("-v"), "Should show -v short flag");
        assert!(stdout.contains("-f"), "Should show -f short flag");
    }
}

// =============================================================================
// JPG Conversion Tests
// =============================================================================

mod jpg_conversion {
    use super::*;

    #[test]
    fn converts_dcm_files_to_jpg_in_series_subfolders() {
        let example = example_folder();
        if !example.exists() {
            eprintln!("Skipping test: example folder not found");
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--force",
        ]);

        assert!(output.status.success(), "CLI failed: {:?}", output);
        assert!(output_path.exists(), "Output folder should exist");

        // Should have series subfolders (not JPGs directly in output)
        let subdirs = get_subdirs(&output_path);
        assert!(
            !subdirs.is_empty(),
            "Should have at least one series subfolder"
        );

        // JPG files should exist inside series subfolders
        let jpg_count = count_files_with_extension(&output_path, "jpg");
        assert!(jpg_count > 0, "Should have created JPG files in subfolders");
    }

    #[test]
    fn jpg_files_are_sequentially_named_within_series() {
        let example = example_folder();
        if !example.exists() {
            eprintln!("Skipping test: example folder not found");
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--force",
        ]);

        assert!(output.status.success());

        // Check each series subfolder
        for subdir in get_subdirs(&output_path) {
            let mut filenames: Vec<String> = fs::read_dir(&subdir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| name.ends_with(".jpg"))
                .collect();

            if filenames.is_empty() {
                continue;
            }

            filenames.sort();

            // Files should be sequential starting from 0001
            for (i, name) in filenames.iter().enumerate() {
                let expected_num = i + 1;
                let file_num: usize = name.trim_end_matches(".jpg").parse().unwrap_or(0);
                assert_eq!(
                    file_num, expected_num,
                    "File {} should be numbered {}",
                    name, expected_num
                );
            }
        }
    }

    #[test]
    fn force_flag_cleans_existing_series_folders() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        // First run to create series folders
        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--force",
        ]);
        assert!(output.status.success());

        // Add a dummy file to one of the series folders
        let subdirs = get_subdirs(&output_path);
        if let Some(first_subdir) = subdirs.first() {
            fs::write(first_subdir.join("old_file.txt"), "old content").unwrap();
        }

        // Run again with force
        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--force",
        ]);

        assert!(output.status.success());

        // Old file should be gone from all series folders
        for subdir in get_subdirs(&output_path) {
            assert!(
                !subdir.join("old_file.txt").exists(),
                "Old files should be cleaned from {:?}",
                subdir
            );
        }
    }

    #[test]
    fn empty_series_folder_works_without_force() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        // Create empty output folder
        fs::create_dir_all(&output_path).unwrap();

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
        ]);

        assert!(output.status.success());

        // Should have series subfolders with JPGs
        let subdirs = get_subdirs(&output_path);
        assert!(!subdirs.is_empty(), "Should have series subfolders");
    }

    #[test]
    fn short_force_flag_works() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        // First run to create folders
        run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--force",
        ]);

        // Add dummy file to a series folder
        let subdirs = get_subdirs(&output_path);
        if let Some(first_subdir) = subdirs.first() {
            fs::write(first_subdir.join("old.txt"), "content").unwrap();
        }

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "-f", // Short flag
        ]);

        assert!(output.status.success());

        // Verify old files are cleaned
        for subdir in get_subdirs(&output_path) {
            assert!(
                !subdir.join("old.txt").exists(),
                "Old file should be cleaned"
            );
        }
    }

    #[test]
    fn creates_multiple_series_folders_when_data_has_multiple_series() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--force",
        ]);

        assert!(output.status.success());

        let subdirs = get_subdirs(&output_path);
        // This test verifies the structure; actual count depends on test data
        assert!(
            !subdirs.is_empty(),
            "Should create at least one series subfolder"
        );

        // Verify each subfolder has JPGs
        for subdir in &subdirs {
            let jpg_count: usize = fs::read_dir(subdir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "jpg"))
                .count();

            assert!(
                jpg_count > 0,
                "Series folder {:?} should have JPG files",
                subdir
            );
        }
    }
}

// =============================================================================
// Video Conversion Tests
// =============================================================================

mod video_conversion {
    use super::*;

    #[test]
    fn video_flag_creates_video_in_series_subfolders() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        if !ffmpeg_available() {
            eprintln!("Skipping test: ffmpeg not available");
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("video_output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--video",
            "--force",
        ]);

        assert!(output.status.success(), "CLI failed: {:?}", output);
        assert!(output_path.exists(), "Output folder should exist");

        // Videos should be in series subfolders, named after the folder
        let subdirs = get_subdirs(&output_path);
        assert!(
            !subdirs.is_empty(),
            "Should have at least one series subfolder"
        );

        for subdir in &subdirs {
            let folder_name = subdir.file_name().unwrap().to_str().unwrap();
            let video_file = subdir.join(format!("{folder_name}.mp4"));

            assert!(
                video_file.exists(),
                "Video file should exist at {:?}",
                video_file
            );

            // Check file size is reasonable (> 1KB)
            let metadata = fs::metadata(&video_file).unwrap();
            assert!(
                metadata.len() > 1024,
                "Video file should have content: {} bytes",
                metadata.len()
            );
        }
    }

    #[test]
    fn fps_parameter_is_accepted() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        if !ffmpeg_available() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("video_output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--video",
            "--fps",
            "30",
            "--force",
        ]);

        assert!(output.status.success());

        // Verify video files were created in series subfolders
        let mp4_count = count_files_with_extension(&output_path, "mp4");
        assert!(mp4_count > 0, "Should have created at least one MP4 file");
    }

    #[test]
    fn video_files_named_after_series_folder() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        if !ffmpeg_available() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("video_output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--video",
            "--force",
        ]);

        assert!(output.status.success());

        // Each series folder should have a video named {folder}.mp4
        for subdir in get_subdirs(&output_path) {
            let folder_name = subdir.file_name().unwrap().to_str().unwrap();
            let expected_video = subdir.join(format!("{folder_name}.mp4"));

            assert!(
                expected_video.exists(),
                "Video should be named {folder_name}.mp4 in {:?}",
                subdir
            );
        }
    }

    #[test]
    fn video_mode_cleans_existing_series_with_force() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        if !ffmpeg_available() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("video_output");

        // First run
        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--video",
            "--force",
        ]);
        assert!(output.status.success());

        // Add dummy file to a series folder
        let subdirs = get_subdirs(&output_path);
        if let Some(first_subdir) = subdirs.first() {
            fs::write(first_subdir.join("old_video.txt"), "old").unwrap();
        }

        // Run again with force
        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--video",
            "--force",
        ]);

        assert!(output.status.success());

        // Old files should be cleaned
        for subdir in get_subdirs(&output_path) {
            assert!(
                !subdir.join("old_video.txt").exists(),
                "Old files should be cleaned"
            );
        }
    }
}

// =============================================================================
// Output Mode Tests
// =============================================================================

mod output_mode {
    use super::*;

    #[test]
    fn default_output_is_jpg_mode_in_series_folders() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("my_output_folder");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--force",
        ]);

        assert!(output.status.success());
        assert!(output_path.is_dir(), "Output should be a directory");

        // Should have series subfolders with JPGs inside
        let subdirs = get_subdirs(&output_path);
        assert!(!subdirs.is_empty(), "Should have series subfolders");

        // No JPGs directly in output root
        let root_jpgs: usize = fs::read_dir(&output_path)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "jpg"))
            .count();
        assert_eq!(root_jpgs, 0, "JPGs should not be in root output folder");

        // JPGs should be inside series subfolders
        let total_jpgs = count_files_with_extension(&output_path, "jpg");
        assert!(total_jpgs > 0, "Should have JPG files in series folders");
    }

    #[test]
    fn video_flag_creates_videos_in_series_folders() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        if !ffmpeg_available() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("video_folder");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--video",
            "--force",
        ]);

        assert!(output.status.success());
        assert!(output_path.is_dir(), "Output should be a directory");

        // Should have series subfolders
        let subdirs = get_subdirs(&output_path);
        assert!(!subdirs.is_empty(), "Should have series subfolders");

        // Each series folder should have its own video
        for subdir in &subdirs {
            let folder_name = subdir.file_name().unwrap().to_str().unwrap();
            let video_file = subdir.join(format!("{folder_name}.mp4"));
            assert!(
                video_file.is_file(),
                "Should have {folder_name}.mp4 in {:?}",
                subdir
            );
        }
    }

    #[test]
    fn no_mp4_files_in_jpg_mode() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--force",
        ]);

        assert!(output.status.success());

        let mp4_count = count_files_with_extension(&output_path, "mp4");
        assert_eq!(mp4_count, 0, "JPG mode should not create MP4 files");
    }

    #[test]
    fn no_jpg_files_in_video_mode() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        if !ffmpeg_available() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--video",
            "--force",
        ]);

        assert!(output.status.success());

        // Note: Video mode uses temp dir for JPGs, then converts
        // After conversion, output should only have MP4s
        let jpg_count = count_files_with_extension(&output_path, "jpg");
        assert_eq!(jpg_count, 0, "Video mode should not leave JPG files");
    }
}

// =============================================================================
// Empty Input Tests
// =============================================================================

mod empty_input {
    use super::*;

    #[test]
    fn empty_folder_shows_message() {
        let temp_input = TempDir::new().unwrap();
        let temp_output = TempDir::new().unwrap();
        let output_path = temp_output.path().join("output");

        let output = run_cli(&[
            "--in",
            temp_input.path().to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
        ]);

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("No .dcm files"),
            "Should indicate no DCM files found: {}",
            stdout
        );
    }

    #[test]
    fn empty_folder_does_not_create_output() {
        let temp_input = TempDir::new().unwrap();
        let temp_output = TempDir::new().unwrap();
        let output_path = temp_output.path().join("output");

        let output = run_cli(&[
            "--in",
            temp_input.path().to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
        ]);

        assert!(output.status.success());

        // Output folder may exist but should have no series subfolders
        if output_path.exists() {
            let subdirs = get_subdirs(&output_path);
            assert!(subdirs.is_empty(), "Empty input should not create series folders");
        }
    }
}

// =============================================================================
// Split-By Tests
// =============================================================================

mod split_by {
    use super::*;

    #[test]
    fn split_by_series_number_creates_numbered_folders() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--split-by",
            "series-number",
            "--force",
        ]);

        assert!(output.status.success());

        let subdirs = get_subdirs(&output_path);
        assert!(!subdirs.is_empty(), "Should have series subfolders");

        // Folder names should be numeric (series numbers)
        for subdir in &subdirs {
            let name = subdir.file_name().unwrap().to_str().unwrap();
            // Either numeric or "unknown"
            assert!(
                name.parse::<i32>().is_ok() || name == "unknown",
                "Folder name should be numeric or 'unknown': {}",
                name
            );
        }
    }

    #[test]
    fn split_by_flag_shown_in_output() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--split-by",
            "description",
            "--force",
        ]);

        assert!(output.status.success());

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Splitting by") || stdout.contains("Description"),
            "Should indicate split-by mode: {}",
            stdout
        );
    }

    #[test]
    fn default_split_by_is_series_number() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--force",
        ]);

        assert!(output.status.success());

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("SeriesNumber") || stdout.contains("series-number"),
            "Default should be series-number: {}",
            stdout
        );
    }
}
