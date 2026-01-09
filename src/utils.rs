//! Utility functions for path validation, file operations, and sanitization.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};

/// User's choice when prompted about overwriting existing folders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CleanupChoice {
    /// Clean folder before writing, apply to all remaining folders
    YesToAll,
    /// Clean folder before writing, ask again for next folder
    Yes,
    /// Don't clean, just overwrite matching files, ask again for next folder
    No,
    /// Don't clean, just overwrite matching files, apply to all remaining folders
    NoToAll,
}

impl CleanupChoice {
    /// Whether to clean the folder before writing.
    pub fn should_clean(&self) -> bool {
        matches!(self, Self::YesToAll | Self::Yes)
    }

    /// Whether this choice should be saved for remaining folders.
    pub fn is_persistent(&self) -> bool {
        matches!(self, Self::YesToAll | Self::NoToAll)
    }
}

/// Prompt the user for overwrite confirmation.
pub fn prompt_to_cleanup(folder_path: &PathBuf) -> Result<CleanupChoice> {
    println!("Folder already exists: {folder_path:?}");
    print!("Cleanup? [Y]es / Yes to [A]ll / [N]o / No to A[l]l: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let choice = match input.trim().to_lowercase().as_str() {
        "y" | "yes" => CleanupChoice::Yes,
        "a" | "yes to all" | "all" => CleanupChoice::YesToAll,
        "n" | "no" => CleanupChoice::No,
        "l" | "no to all" => CleanupChoice::NoToAll,
        _ => {
            println!("Invalid choice, defaulting to 'No'");
            CleanupChoice::No
        }
    };

    Ok(choice)
}

/// Validate that the input folder exists and is a directory.
pub fn validate_input_folder(input: &PathBuf) -> Result<()> {
    if !input.exists() {
        anyhow::bail!("Input folder does not exist: {input:?}");
    }
    if !input.is_dir() {
        anyhow::bail!("Input path is not a directory: {input:?}");
    }
    Ok(())
}

/// Sanitize a string for use as a filename/folder name.
/// Replaces invalid characters with underscores.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_ascii_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Clean existing output folder if requested.
/// When `should_clean` is true, removes all contents.
/// When `should_clean` is false, the folder is left as-is (files will be overwritten).
pub fn clean_output(path: &PathBuf, should_clean: bool) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if path.is_file() {
        if should_clean {
            fs::remove_file(path)
                .with_context(|| format!("Failed to remove existing file: {path:?}"))?;
            println!("Removed existing file: {path:?}");
        }
        // If not cleaning, the file will be overwritten naturally
    } else if path.is_dir() && !is_folder_empty(path)? && should_clean {
        fs::remove_dir_all(path)
            .with_context(|| format!("Failed to clean output folder: {path:?}"))?;
        println!("Cleaned output folder: {path:?}");
    }

    Ok(())
}

/// Check if a folder is empty.
pub fn is_folder_empty(path: &PathBuf) -> Result<bool> {
    let mut entries =
        fs::read_dir(path).with_context(|| format!("Failed to read directory: {path:?}"))?;
    Ok(entries.next().is_none())
}

// =============================================================================
// Unit Tests for utils module
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // =========================================================================
    // CleanupChoice Tests
    // =========================================================================

    mod cleanup_choice_tests {
        use super::*;

        #[test]
        fn yes_to_all_should_clean_and_is_persistent() {
            let choice = CleanupChoice::YesToAll;
            assert!(choice.should_clean());
            assert!(choice.is_persistent());
        }

        #[test]
        fn yes_should_clean_but_not_persistent() {
            let choice = CleanupChoice::Yes;
            assert!(choice.should_clean());
            assert!(!choice.is_persistent());
        }

        #[test]
        fn no_should_not_clean_and_not_persistent() {
            let choice = CleanupChoice::No;
            assert!(!choice.should_clean());
            assert!(!choice.is_persistent());
        }

        #[test]
        fn no_to_all_should_not_clean_but_is_persistent() {
            let choice = CleanupChoice::NoToAll;
            assert!(!choice.should_clean());
            assert!(choice.is_persistent());
        }

        #[test]
        fn all_variants_are_copy() {
            let original = CleanupChoice::Yes;
            let copied = original;
            assert_eq!(original, copied);
        }

        #[test]
        fn all_variants_are_clone() {
            let choices = [
                CleanupChoice::YesToAll,
                CleanupChoice::Yes,
                CleanupChoice::No,
                CleanupChoice::NoToAll,
            ];
            for choice in choices {
                assert_eq!(choice, choice.clone());
            }
        }

        #[test]
        fn debug_format_is_readable() {
            assert!(format!("{:?}", CleanupChoice::YesToAll).contains("YesToAll"));
            assert!(format!("{:?}", CleanupChoice::Yes).contains("Yes"));
            assert!(format!("{:?}", CleanupChoice::No).contains("No"));
            assert!(format!("{:?}", CleanupChoice::NoToAll).contains("NoToAll"));
        }

        #[test]
        fn equality_works_correctly() {
            assert_eq!(CleanupChoice::Yes, CleanupChoice::Yes);
            assert_ne!(CleanupChoice::Yes, CleanupChoice::No);
            assert_ne!(CleanupChoice::YesToAll, CleanupChoice::NoToAll);
        }
    }

    // =========================================================================
    // validate_input_folder Tests
    // =========================================================================

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

        #[test]
        fn empty_folder_is_valid() {
            let temp_dir = TempDir::new().unwrap();
            let empty_folder = temp_dir.path().join("empty");
            fs::create_dir(&empty_folder).unwrap();

            let result = validate_input_folder(&empty_folder);
            assert!(result.is_ok());
        }

        #[test]
        fn folder_with_files_is_valid() {
            let temp_dir = TempDir::new().unwrap();
            fs::write(temp_dir.path().join("file.txt"), "content").unwrap();

            let result = validate_input_folder(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
        }

        #[test]
        fn symlink_to_folder_is_valid() {
            let temp_dir = TempDir::new().unwrap();
            let target = temp_dir.path().join("target");
            fs::create_dir(&target).unwrap();

            #[cfg(unix)]
            {
                let link = temp_dir.path().join("link");
                std::os::unix::fs::symlink(&target, &link).unwrap();
                let result = validate_input_folder(&link);
                assert!(result.is_ok());
            }
        }

        #[test]
        fn relative_path_works() {
            // Create a temp dir and ensure current dir check works
            let temp_dir = TempDir::new().unwrap();
            let abs_path = temp_dir.path().to_path_buf();
            let result = validate_input_folder(&abs_path);
            assert!(result.is_ok());
        }
    }

    // =========================================================================
    // sanitize_filename Tests
    // =========================================================================

    mod sanitize_filename_tests {
        use super::*;

        #[test]
        fn replaces_forward_slash() {
            assert_eq!(sanitize_filename("a/b"), "a_b");
        }

        #[test]
        fn replaces_backslash() {
            assert_eq!(sanitize_filename("a\\b"), "a_b");
        }

        #[test]
        fn replaces_colon() {
            assert_eq!(sanitize_filename("a:b"), "a_b");
        }

        #[test]
        fn replaces_asterisk() {
            assert_eq!(sanitize_filename("a*b"), "a_b");
        }

        #[test]
        fn replaces_question_mark() {
            assert_eq!(sanitize_filename("a?b"), "a_b");
        }

        #[test]
        fn replaces_double_quote() {
            assert_eq!(sanitize_filename("a\"b"), "a_b");
        }

        #[test]
        fn replaces_less_than() {
            assert_eq!(sanitize_filename("a<b"), "a_b");
        }

        #[test]
        fn replaces_greater_than() {
            assert_eq!(sanitize_filename("a>b"), "a_b");
        }

        #[test]
        fn replaces_pipe() {
            assert_eq!(sanitize_filename("a|b"), "a_b");
        }

        #[test]
        fn replaces_all_invalid_characters() {
            assert_eq!(
                sanitize_filename("a/b\\c:d*e?f\"g<h>i|j"),
                "a_b_c_d_e_f_g_h_i_j"
            );
        }

        #[test]
        fn replaces_control_characters() {
            assert_eq!(sanitize_filename("a\x00b\x1Fc"), "a_b_c");
        }

        #[test]
        fn replaces_tab_character() {
            assert_eq!(sanitize_filename("a\tb"), "a_b");
        }

        #[test]
        fn replaces_newline_character() {
            assert_eq!(sanitize_filename("a\nb"), "a_b");
        }

        #[test]
        fn replaces_carriage_return() {
            assert_eq!(sanitize_filename("a\rb"), "a_b");
        }

        #[test]
        fn trims_leading_whitespace() {
            assert_eq!(sanitize_filename("  test"), "test");
        }

        #[test]
        fn trims_trailing_whitespace() {
            assert_eq!(sanitize_filename("test  "), "test");
        }

        #[test]
        fn trims_both_ends() {
            assert_eq!(sanitize_filename("  test  "), "test");
        }

        #[test]
        fn preserves_internal_spaces() {
            assert_eq!(sanitize_filename("hello world"), "hello world");
        }

        #[test]
        fn preserves_valid_characters() {
            assert_eq!(
                sanitize_filename("valid_filename-123.txt"),
                "valid_filename-123.txt"
            );
        }

        #[test]
        fn preserves_dots() {
            assert_eq!(sanitize_filename("file.name.ext"), "file.name.ext");
        }

        #[test]
        fn preserves_underscores() {
            assert_eq!(sanitize_filename("a_b_c"), "a_b_c");
        }

        #[test]
        fn preserves_hyphens() {
            assert_eq!(sanitize_filename("a-b-c"), "a-b-c");
        }

        #[test]
        fn preserves_numbers() {
            assert_eq!(sanitize_filename("12345"), "12345");
        }

        #[test]
        fn preserves_uppercase() {
            assert_eq!(sanitize_filename("UPPER"), "UPPER");
        }

        #[test]
        fn preserves_mixed_case() {
            assert_eq!(sanitize_filename("MixedCase"), "MixedCase");
        }

        #[test]
        fn empty_string_stays_empty() {
            assert_eq!(sanitize_filename(""), "");
        }

        #[test]
        fn whitespace_only_becomes_empty() {
            assert_eq!(sanitize_filename("   "), "");
        }

        #[test]
        fn handles_unicode_characters() {
            assert_eq!(sanitize_filename("日本語"), "日本語");
        }

        #[test]
        fn handles_emoji() {
            assert_eq!(sanitize_filename("test🎉emoji"), "test🎉emoji");
        }

        #[test]
        fn handles_accented_characters() {
            assert_eq!(sanitize_filename("café"), "café");
        }

        #[test]
        fn handles_consecutive_invalid_chars() {
            assert_eq!(sanitize_filename("a//\\\\b"), "a____b");
        }

        #[test]
        fn realistic_series_description() {
            assert_eq!(
                sanitize_filename("Series 1: T2W FLAIR/DARK-FLUID"),
                "Series 1_ T2W FLAIR_DARK-FLUID"
            );
        }

        #[test]
        fn realistic_patient_name() {
            assert_eq!(sanitize_filename("Doe^John"), "Doe^John");
        }
    }

    // =========================================================================
    // clean_output Tests
    // =========================================================================

    mod clean_output_tests {
        use super::*;

        #[test]
        fn allows_nonexistent_path() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("nonexistent");

            let result = clean_output(&output_path, false);
            assert!(result.is_ok());
        }

        #[test]
        fn allows_nonexistent_path_with_clean_true() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("nonexistent");

            let result = clean_output(&output_path, true);
            assert!(result.is_ok());
        }

        #[test]
        fn cleans_non_empty_folder_when_should_clean_is_true() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("output");
            fs::create_dir(&output_path).unwrap();
            fs::write(output_path.join("test.jpg"), "fake jpg").unwrap();

            let result = clean_output(&output_path, true);
            assert!(result.is_ok());
            assert!(!output_path.exists());
        }

        #[test]
        fn leaves_non_empty_folder_when_should_clean_is_false() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("output");
            fs::create_dir(&output_path).unwrap();
            fs::write(output_path.join("test.jpg"), "fake jpg").unwrap();

            let result = clean_output(&output_path, false);
            assert!(result.is_ok());
            assert!(output_path.exists());
            assert!(output_path.join("test.jpg").exists());
        }

        #[test]
        fn allows_empty_folder_without_cleaning() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("empty_folder");
            fs::create_dir(&output_path).unwrap();

            let result = clean_output(&output_path, false);
            assert!(result.is_ok());
            assert!(output_path.exists());
        }

        #[test]
        fn does_not_clean_empty_folder_even_with_clean_true() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("empty_folder");
            fs::create_dir(&output_path).unwrap();

            let result = clean_output(&output_path, true);
            assert!(result.is_ok());
            // Empty folder is not removed (only non-empty)
            assert!(output_path.exists());
        }

        #[test]
        fn removes_existing_file_when_should_clean_is_true() {
            let temp_dir = TempDir::new().unwrap();
            let file_path = temp_dir.path().join("existing.mp4");
            fs::write(&file_path, "fake video").unwrap();

            let result = clean_output(&file_path, true);
            assert!(result.is_ok());
            assert!(!file_path.exists());
        }

        #[test]
        fn leaves_existing_file_when_should_clean_is_false() {
            let temp_dir = TempDir::new().unwrap();
            let file_path = temp_dir.path().join("existing.mp4");
            fs::write(&file_path, "fake video").unwrap();

            let result = clean_output(&file_path, false);
            assert!(result.is_ok());
            assert!(file_path.exists());
        }

        #[test]
        fn cleans_folder_with_nested_structure() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("output");
            let nested = output_path.join("nested").join("deep");
            fs::create_dir_all(&nested).unwrap();
            fs::write(nested.join("file.txt"), "content").unwrap();

            let result = clean_output(&output_path, true);
            assert!(result.is_ok());
            assert!(!output_path.exists());
        }

        #[test]
        fn cleans_folder_with_multiple_files() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("output");
            fs::create_dir(&output_path).unwrap();

            for i in 0..10 {
                fs::write(output_path.join(format!("file{i}.jpg")), "content").unwrap();
            }

            let result = clean_output(&output_path, true);
            assert!(result.is_ok());
            assert!(!output_path.exists());
        }

        #[test]
        fn leaves_folder_content_intact_when_not_cleaning() {
            let temp_dir = TempDir::new().unwrap();
            let output_path = temp_dir.path().join("output");
            fs::create_dir(&output_path).unwrap();

            let files: Vec<_> = (0..5)
                .map(|i| {
                    let path = output_path.join(format!("file{i}.jpg"));
                    fs::write(&path, format!("content{i}")).unwrap();
                    path
                })
                .collect();

            let result = clean_output(&output_path, false);
            assert!(result.is_ok());

            for file in files {
                assert!(file.exists());
            }
        }
    }

    // =========================================================================
    // is_folder_empty Tests
    // =========================================================================

    mod is_folder_empty_tests {
        use super::*;

        #[test]
        fn returns_true_for_empty_folder() {
            let temp_dir = TempDir::new().unwrap();
            let result = is_folder_empty(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
            assert!(result.unwrap());
        }

        #[test]
        fn returns_false_for_non_empty_folder() {
            let temp_dir = TempDir::new().unwrap();
            fs::write(temp_dir.path().join("file.txt"), "content").unwrap();

            let result = is_folder_empty(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
            assert!(!result.unwrap());
        }

        #[test]
        fn returns_false_for_folder_with_subfolder() {
            let temp_dir = TempDir::new().unwrap();
            fs::create_dir(temp_dir.path().join("subfolder")).unwrap();

            let result = is_folder_empty(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
            assert!(!result.unwrap());
        }

        #[test]
        fn returns_false_for_folder_with_hidden_file() {
            let temp_dir = TempDir::new().unwrap();
            fs::write(temp_dir.path().join(".hidden"), "content").unwrap();

            let result = is_folder_empty(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
            assert!(!result.unwrap());
        }

        #[test]
        fn fails_for_nonexistent_path() {
            let temp_dir = TempDir::new().unwrap();
            let nonexistent = temp_dir.path().join("nonexistent");

            let result = is_folder_empty(&nonexistent);
            assert!(result.is_err());
        }

        #[test]
        fn fails_for_file_path() {
            let temp_dir = TempDir::new().unwrap();
            let file_path = temp_dir.path().join("file.txt");
            fs::write(&file_path, "content").unwrap();

            let result = is_folder_empty(&file_path);
            assert!(result.is_err());
        }

        #[test]
        fn returns_false_for_folder_with_many_files() {
            let temp_dir = TempDir::new().unwrap();
            for i in 0..100 {
                fs::write(temp_dir.path().join(format!("file{i}.txt")), "x").unwrap();
            }

            let result = is_folder_empty(&temp_dir.path().to_path_buf());
            assert!(result.is_ok());
            assert!(!result.unwrap());
        }
    }
}
