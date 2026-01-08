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

/// Helper to run the CLI with given arguments
fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(binary_path())
        .args(args)
        .output()
        .expect("Failed to execute command")
}

mod cli_args {
    use super::*;

    #[test]
    fn missing_input_arg_shows_error() {
        let output = run_cli(&["--out", "/tmp/out"]);

        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--in"));
    }

    #[test]
    fn missing_output_arg_shows_error() {
        let output = run_cli(&["--in", "./example"]);

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
        assert!(stderr.contains("does not exist") || stderr.contains("Error"));
    }

    #[test]
    fn help_flag_shows_usage() {
        let output = run_cli(&["--help"]);

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("--in"));
        assert!(stdout.contains("--out"));
        assert!(stdout.contains("--fps"));
        assert!(stdout.contains("--force"));
    }
}

mod jpg_conversion {
    use super::*;

    #[test]
    fn converts_dcm_files_to_jpg() {
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

        // Check that output folder was created
        assert!(output_path.exists(), "Output folder should exist");

        // Check that JPG files were created
        let jpg_files: Vec<_> = fs::read_dir(&output_path)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "jpg")
                    .unwrap_or(false)
            })
            .collect();

        assert!(!jpg_files.is_empty(), "Should have created JPG files");
    }

    #[test]
    fn jpg_files_are_sequentially_named() {
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

        // Collect and sort filenames
        let mut filenames: Vec<String> = fs::read_dir(&output_path)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| name.ends_with(".jpg"))
            .collect();

        filenames.sort();

        // First file should be 0001.jpg (or similar pattern)
        assert!(
            filenames
                .first()
                .map(|s| s.starts_with("0"))
                .unwrap_or(false),
            "First JPG should start with 0: {:?}",
            filenames.first()
        );

        // Files should be sequential
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

    #[test]
    fn force_flag_cleans_non_empty_folder() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        // Create output folder with a dummy file
        fs::create_dir_all(&output_path).unwrap();
        fs::write(output_path.join("old_file.txt"), "old content").unwrap();

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--force",
        ]);

        assert!(output.status.success());

        // Old file should be gone
        assert!(
            !output_path.join("old_file.txt").exists(),
            "Old files should be cleaned"
        );
    }

    #[test]
    fn non_empty_folder_without_force_fails() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        // Create output folder with a dummy file
        fs::create_dir_all(&output_path).unwrap();
        fs::write(output_path.join("existing_file.txt"), "content").unwrap();

        // Run without --force, simulating 'no' response via stdin (empty/closed stdin)
        let output = Command::new(binary_path())
            .args([
                "--in",
                example.to_str().unwrap(),
                "--out",
                output_path.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::null()) // No input, defaults to 'no'
            .output()
            .expect("Failed to execute command");

        // Should fail because user didn't confirm
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("cancelled") || stderr.contains("not empty"),
            "Should indicate operation was cancelled: {}",
            stderr
        );
    }

    #[test]
    fn empty_folder_works_without_force() {
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
    }

    #[test]
    fn short_force_flag_works() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output");

        // Create non-empty folder
        fs::create_dir_all(&output_path).unwrap();
        fs::write(output_path.join("old.txt"), "content").unwrap();

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "-f", // Short flag
        ]);

        assert!(output.status.success());
        assert!(!output_path.join("old.txt").exists());
    }
}

mod video_conversion {
    use super::*;

    #[test]
    fn detects_video_output_by_extension() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        // Check if ffmpeg is available
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("Skipping test: ffmpeg not available");
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("scan.mp4");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
        ]);

        assert!(output.status.success(), "CLI failed: {:?}", output);

        // Check that video file was created
        assert!(output_path.exists(), "Video file should exist");

        // Check file size is reasonable (> 1KB)
        let metadata = fs::metadata(&output_path).unwrap();
        assert!(
            metadata.len() > 1024,
            "Video file should have content: {} bytes",
            metadata.len()
        );
    }

    #[test]
    fn fps_parameter_is_accepted() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("scan.mp4");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--fps",
            "30",
        ]);

        assert!(output.status.success());
        assert!(output_path.exists());
    }
}

mod output_detection {
    use super::*;

    #[test]
    fn folder_path_triggers_jpg_mode() {
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
        ]);

        assert!(output.status.success());

        // Should be a folder with JPGs, not a single file
        assert!(output_path.is_dir(), "Output should be a directory");

        let files: Vec<_> = fs::read_dir(&output_path)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();

        assert!(!files.is_empty(), "Should contain files");
    }

    #[test]
    fn file_path_with_extension_triggers_video_mode() {
        let example = example_folder();
        if !example.exists() {
            return;
        }

        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output.mp4");

        let output = run_cli(&[
            "--in",
            example.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
        ]);

        assert!(output.status.success());

        // Should be a file, not a directory
        assert!(output_path.is_file(), "Output should be a file");
    }
}

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
            "Should indicate no DCM files found"
        );
    }
}
