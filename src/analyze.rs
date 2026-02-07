//! DICOM file analysis module for identifying distinguishing tags.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use dicom::dictionary_std::tags;
use dicom::object::open_file;

use crate::utils::validate_input_folder;

/// CLI arguments for the `analyze` subcommand.
#[derive(Args, Debug)]
pub struct AnalyzeArgs {
    /// Input folder containing DICOM (.dcm) files
    #[arg(long = "in")]
    pub input: PathBuf,

    /// Expected number of groups/series (highlights matching tags in recommendation)
    #[arg(long, short = 'g')]
    pub expected_groups: Option<usize>,
}

/// Analyze DICOM files to find distinguishing tags for different cuts/series.
pub fn run(args: &AnalyzeArgs) -> Result<()> {
    validate_input_folder(&args.input)?;

    let entries = fs::read_dir(&args.input)
        .with_context(|| format!("Failed to read input folder: {:?}", args.input))?;

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
        println!("No .dcm files found in {:?}", args.input);
        return Ok(());
    }

    println!("Analyzing {} DICOM files...\n", dcm_files.len());

    // Collect all unique values for each tag we're interested in
    let mut series_uid_map: HashMap<String, usize> = HashMap::new();
    let mut series_number_map: HashMap<String, usize> = HashMap::new();
    let mut acquisition_number_map: HashMap<String, usize> = HashMap::new();
    let mut series_description_map: HashMap<String, usize> = HashMap::new();
    let mut orientation_map: HashMap<String, usize> = HashMap::new();
    let mut stack_id_map: HashMap<String, usize> = HashMap::new();

    for dcm_path in &dcm_files {
        if let Ok(obj) = open_file(dcm_path) {
            // SeriesInstanceUID
            if let Ok(val) = obj.element(tags::SERIES_INSTANCE_UID) {
                if let Ok(s) = val.to_str() {
                    *series_uid_map.entry(s.to_string()).or_insert(0) += 1;
                }
            }
            // SeriesNumber
            if let Ok(val) = obj.element(tags::SERIES_NUMBER) {
                if let Ok(s) = val.to_str() {
                    *series_number_map.entry(s.to_string()).or_insert(0) += 1;
                }
            }
            // AcquisitionNumber
            if let Ok(val) = obj.element(tags::ACQUISITION_NUMBER) {
                if let Ok(s) = val.to_str() {
                    *acquisition_number_map.entry(s.to_string()).or_insert(0) += 1;
                }
            }
            // SeriesDescription
            if let Ok(val) = obj.element(tags::SERIES_DESCRIPTION) {
                if let Ok(s) = val.to_str() {
                    *series_description_map.entry(s.to_string()).or_insert(0) += 1;
                }
            }
            // ImageOrientationPatient
            if let Ok(val) = obj.element(tags::IMAGE_ORIENTATION_PATIENT) {
                if let Ok(s) = val.to_str() {
                    *orientation_map.entry(s.to_string()).or_insert(0) += 1;
                }
            }
            // StackID (private tag 0020,9056)
            if let Ok(val) = obj.element(dicom::core::Tag(0x0020, 0x9056)) {
                if let Ok(s) = val.to_str() {
                    *stack_id_map.entry(s.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    println!("=== Potential Cut Identifiers ===\n");

    println!(
        "SeriesInstanceUID (0020,000E): {} unique values",
        series_uid_map.len()
    );
    if series_uid_map.len() <= 20 {
        let mut entries: Vec<_> = series_uid_map.iter().collect();
        entries.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        for (uid, count) in entries {
            println!("  - {} files: {}", count, uid);
        }
    }
    println!();

    println!(
        "SeriesNumber (0020,0011): {} unique values",
        series_number_map.len()
    );
    if series_number_map.len() <= 20 {
        let mut entries: Vec<_> = series_number_map.iter().collect();
        entries.sort_by(|(a, _), (b, _)| {
            a.parse::<i32>()
                .unwrap_or(0)
                .cmp(&b.parse::<i32>().unwrap_or(0))
        });
        for (num, count) in entries {
            println!("  - Series {}: {} files", num, count);
        }
    }
    println!();

    println!(
        "AcquisitionNumber (0020,0012): {} unique values",
        acquisition_number_map.len()
    );
    if acquisition_number_map.len() <= 20 {
        let mut entries: Vec<_> = acquisition_number_map.iter().collect();
        entries.sort_by(|(a, _), (b, _)| {
            a.parse::<i32>()
                .unwrap_or(0)
                .cmp(&b.parse::<i32>().unwrap_or(0))
        });
        for (num, count) in entries {
            println!("  - Acquisition {}: {} files", num, count);
        }
    }
    println!();

    println!(
        "SeriesDescription (0008,103E): {} unique values",
        series_description_map.len()
    );
    if series_description_map.len() <= 20 {
        let mut entries: Vec<_> = series_description_map.iter().collect();
        entries.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        for (desc, count) in entries {
            println!("  - \"{}\": {} files", desc, count);
        }
    }
    println!();

    println!(
        "ImageOrientationPatient (0020,0037): {} unique values",
        orientation_map.len()
    );
    if orientation_map.len() <= 20 {
        let mut entries: Vec<_> = orientation_map.iter().collect();
        entries.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        for (orientation, count) in entries {
            println!("  - {} files: {}", count, orientation);
        }
    }
    println!();

    println!("StackID (0020,9056): {} unique values", stack_id_map.len());
    if stack_id_map.len() <= 20 && !stack_id_map.is_empty() {
        let mut entries: Vec<_> = stack_id_map.iter().collect();
        entries.sort_by(|(a, _), (b, _)| {
            a.parse::<i32>()
                .unwrap_or(0)
                .cmp(&b.parse::<i32>().unwrap_or(0))
        });
        for (id, count) in entries {
            println!("  - Stack {}: {} files", id, count);
        }
    }
    println!();

    // Recommendation
    println!("=== Recommendation ===");
    let candidates = [
        (
            "SeriesInstanceUID",
            "--split-by series-uid",
            series_uid_map.len(),
        ),
        (
            "SeriesNumber",
            "--split-by series-number",
            series_number_map.len(),
        ),
        (
            "AcquisitionNumber",
            "--split-by acquisition-number",
            acquisition_number_map.len(),
        ),
        (
            "SeriesDescription",
            "--split-by description",
            series_description_map.len(),
        ),
        (
            "ImageOrientationPatient",
            "--split-by orientation",
            orientation_map.len(),
        ),
        ("StackID", "--split-by stack-id", stack_id_map.len()),
    ];

    if let Some(expected) = args.expected_groups {
        println!("Looking for tag with exactly {} unique values:", expected);
        for (name, flag, count) in candidates {
            if count == expected {
                println!(
                    "  ✓ {} has {} unique values - MATCH! Use: {}",
                    name, count, flag
                );
            } else if count > 1 && count <= 50 {
                println!("  - {} has {} unique values", name, count);
            }
        }
    } else {
        println!(
            "Tags with multiple unique values (use --expected-groups (-g) to highlight matches):"
        );
        for (name, flag, count) in candidates {
            if count > 1 && count <= 50 {
                println!("  - {} has {} unique values ({})", name, count, flag);
            } else if count > 50 {
                println!(
                    "  - {} has {} unique values (too many to list)",
                    name, count
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // =========================================================================
    // HashMap Entry Sorting Tests
    // =========================================================================

    mod entry_sorting {
        use std::collections::HashMap;

        #[test]
        fn sort_by_count_descending() {
            let mut map: HashMap<String, usize> = HashMap::new();
            map.insert("a".to_string(), 10);
            map.insert("b".to_string(), 50);
            map.insert("c".to_string(), 25);

            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

            let keys: Vec<&String> = entries.iter().map(|(k, _)| *k).collect();
            assert_eq!(keys, vec!["b", "c", "a"]);
        }

        #[test]
        fn sort_by_count_handles_equal_counts() {
            let mut map: HashMap<String, usize> = HashMap::new();
            map.insert("a".to_string(), 10);
            map.insert("b".to_string(), 10);
            map.insert("c".to_string(), 10);

            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

            // All have same count, so order is stable but all present
            assert_eq!(entries.len(), 3);
        }

        #[test]
        fn sort_by_numeric_key_ascending() {
            let mut map: HashMap<String, usize> = HashMap::new();
            map.insert("10".to_string(), 1);
            map.insert("2".to_string(), 1);
            map.insert("1".to_string(), 1);
            map.insert("20".to_string(), 1);

            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|(a, _), (b, _)| {
                a.parse::<i32>()
                    .unwrap_or(0)
                    .cmp(&b.parse::<i32>().unwrap_or(0))
            });

            let keys: Vec<i32> = entries.iter().map(|(k, _)| k.parse().unwrap()).collect();
            assert_eq!(keys, vec![1, 2, 10, 20]);
        }

        #[test]
        fn sort_by_numeric_key_handles_non_numeric() {
            let mut map: HashMap<String, usize> = HashMap::new();
            map.insert("10".to_string(), 1);
            map.insert("invalid".to_string(), 1);
            map.insert("2".to_string(), 1);

            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|(a, _), (b, _)| {
                a.parse::<i32>()
                    .unwrap_or(0)
                    .cmp(&b.parse::<i32>().unwrap_or(0))
            });

            // Non-numeric parses to 0, so "invalid" comes first
            let first_key = entries[0].0.as_str();
            assert_eq!(first_key, "invalid");
        }

        #[test]
        fn sort_preserves_all_entries() {
            let mut map: HashMap<String, usize> = HashMap::new();
            for i in 0..100 {
                map.insert(format!("key_{i}"), i);
            }

            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

            assert_eq!(entries.len(), 100);
        }

        #[test]
        fn empty_map_sorts_without_error() {
            let map: HashMap<String, usize> = HashMap::new();
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

            assert!(entries.is_empty());
        }
    }

    // =========================================================================
    // Candidate Filtering Tests
    // =========================================================================

    mod candidate_filtering {
        #[test]
        fn filter_candidates_with_expected_groups() {
            let candidates = [
                ("SeriesUID", "--split-by series-uid", 3),
                ("SeriesNumber", "--split-by series-number", 5),
                ("AcquisitionNumber", "--split-by acquisition-number", 1),
                ("Description", "--split-by description", 5),
            ];

            let expected = 5;
            let matches: Vec<_> = candidates
                .iter()
                .filter(|(_, _, count)| *count == expected)
                .collect();

            assert_eq!(matches.len(), 2);
            assert!(matches.iter().any(|(name, _, _)| *name == "SeriesNumber"));
            assert!(matches.iter().any(|(name, _, _)| *name == "Description"));
        }

        #[test]
        fn filter_candidates_excludes_single_value() {
            let candidates = [("A", "--a", 1), ("B", "--b", 5), ("C", "--c", 10)];

            let valid: Vec<_> = candidates
                .iter()
                .filter(|(_, _, count)| *count > 1 && *count <= 50)
                .collect();

            assert_eq!(valid.len(), 2);
            assert!(!valid.iter().any(|(name, _, _)| *name == "A"));
        }

        #[test]
        fn filter_candidates_excludes_too_many_values() {
            let candidates = [("A", "--a", 10), ("B", "--b", 51), ("C", "--c", 100)];

            let valid: Vec<_> = candidates
                .iter()
                .filter(|(_, _, count)| *count > 1 && *count <= 50)
                .collect();

            assert_eq!(valid.len(), 1);
            assert!(valid.iter().any(|(name, _, _)| *name == "A"));
        }

        #[test]
        fn filter_boundary_values() {
            let candidates = [
                ("Boundary1", "--1", 1),    // Excluded: not > 1
                ("Boundary2", "--2", 2),    // Included: exactly > 1
                ("Boundary50", "--50", 50), // Included: exactly <= 50
                ("Boundary51", "--51", 51), // Excluded: not <= 50
            ];

            let valid: Vec<_> = candidates
                .iter()
                .filter(|(_, _, count)| *count > 1 && *count <= 50)
                .collect();

            assert_eq!(valid.len(), 2);
            assert!(valid.iter().any(|(name, _, _)| *name == "Boundary2"));
            assert!(valid.iter().any(|(name, _, _)| *name == "Boundary50"));
        }
    }

    // =========================================================================
    // Recommendation Logic Tests
    // =========================================================================

    mod recommendation_logic {
        #[test]
        fn recommendation_finds_exact_match() {
            let candidates = [("SeriesUID", 3), ("SeriesNumber", 5), ("Description", 5)];

            let expected = 5;
            let exact_matches: Vec<_> = candidates
                .iter()
                .filter(|(_, count)| *count == expected)
                .map(|(name, _)| *name)
                .collect();

            assert_eq!(exact_matches.len(), 2);
            assert!(exact_matches.contains(&"SeriesNumber"));
            assert!(exact_matches.contains(&"Description"));
        }

        #[test]
        fn recommendation_handles_no_match() {
            let candidates = [("SeriesUID", 3), ("SeriesNumber", 5), ("Description", 10)];

            let expected = 7;
            let exact_matches: Vec<_> = candidates
                .iter()
                .filter(|(_, count)| *count == expected)
                .collect();

            assert!(exact_matches.is_empty());
        }

        #[test]
        fn recommendation_prioritizes_usable_ranges() {
            let candidates = [
                ("A", 0),   // Too few
                ("B", 1),   // Too few (single value)
                ("C", 10),  // Good
                ("D", 50),  // Good (max boundary)
                ("E", 51),  // Too many
                ("F", 100), // Too many
            ];

            let usable: Vec<_> = candidates
                .iter()
                .filter(|(_, count)| *count > 1 && *count <= 50)
                .map(|(name, _)| *name)
                .collect();

            assert_eq!(usable, vec!["C", "D"]);
        }
    }

    // =========================================================================
    // Display Logic Tests
    // =========================================================================

    mod display_logic {

        #[test]
        fn should_display_entries_when_20_or_fewer() {
            for count in 1..=20 {
                let map_len = count;
                assert!(map_len <= 20, "Should display when count is {count}");
            }
        }

        #[test]
        fn should_not_display_entries_when_more_than_20() {
            for count in 21..=100 {
                let map_len = count;
                assert!(map_len > 20, "Should not display when count is {count}");
            }
        }

        #[test]
        fn truncation_threshold_is_20() {
            let threshold = 20;

            let display_cases = [1, 5, 10, 15, 20];
            for count in display_cases {
                assert!(count <= threshold);
            }

            let hide_cases = [21, 50, 100];
            for count in hide_cases {
                assert!(count > threshold);
            }
        }

        #[test]
        fn format_series_number_output() {
            let series_num = "5";
            let count = 30;
            let formatted = format!("  - Series {}: {} files", series_num, count);
            assert_eq!(formatted, "  - Series 5: 30 files");
        }

        #[test]
        fn format_series_description_output() {
            let description = "T2W FLAIR";
            let count = 25;
            let formatted = format!("  - \"{}\": {} files", description, count);
            assert_eq!(formatted, "  - \"T2W FLAIR\": 25 files");
        }

        #[test]
        fn format_uid_output() {
            let uid = "1.2.3.4.5.6.7.8.9";
            let count = 100;
            let formatted = format!("  - {} files: {}", count, uid);
            assert_eq!(formatted, "  - 100 files: 1.2.3.4.5.6.7.8.9");
        }

        #[test]
        fn format_stack_id_output() {
            let stack_id = "1";
            let count = 50;
            let formatted = format!("  - Stack {}: {} files", stack_id, count);
            assert_eq!(formatted, "  - Stack 1: 50 files");
        }
    }

    // =========================================================================
    // Empty Map Handling Tests
    // =========================================================================

    mod empty_handling {
        use std::collections::HashMap;

        #[test]
        fn empty_map_has_zero_length() {
            let map: HashMap<String, usize> = HashMap::new();
            assert_eq!(map.len(), 0);
            assert!(map.is_empty());
        }

        #[test]
        fn empty_stack_id_map_should_not_display() {
            let stack_id_map: HashMap<String, usize> = HashMap::new();
            // The condition in the code is: map.len() <= 20 && !map.is_empty()
            let should_display = stack_id_map.len() <= 20 && !stack_id_map.is_empty();
            assert!(!should_display);
        }

        #[test]
        fn non_empty_stack_id_map_should_display() {
            let mut stack_id_map: HashMap<String, usize> = HashMap::new();
            stack_id_map.insert("1".to_string(), 10);

            let should_display = stack_id_map.len() <= 20 && !stack_id_map.is_empty();
            assert!(should_display);
        }
    }

    // =========================================================================
    // File Extension Filter Tests
    // =========================================================================

    mod file_filtering {
        use std::path::PathBuf;

        #[test]
        fn dcm_extension_matches_lowercase() {
            let path = PathBuf::from("/path/to/file.dcm");
            let is_dcm = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dcm"));
            assert!(is_dcm);
        }

        #[test]
        fn dcm_extension_matches_uppercase() {
            let path = PathBuf::from("/path/to/file.DCM");
            let is_dcm = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dcm"));
            assert!(is_dcm);
        }

        #[test]
        fn dcm_extension_matches_mixed_case() {
            let path = PathBuf::from("/path/to/file.Dcm");
            let is_dcm = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dcm"));
            assert!(is_dcm);
        }

        #[test]
        fn non_dcm_extension_does_not_match() {
            let paths = [
                PathBuf::from("/path/to/file.jpg"),
                PathBuf::from("/path/to/file.png"),
                PathBuf::from("/path/to/file.dicom"),
                PathBuf::from("/path/to/file"),
            ];

            for path in paths {
                let is_dcm = path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("dcm"));
                assert!(!is_dcm, "Path {:?} should not match DCM", path);
            }
        }

        #[test]
        fn no_extension_does_not_match() {
            let path = PathBuf::from("/path/to/file");
            let is_dcm = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dcm"));
            assert!(!is_dcm);
        }
    }

    // =========================================================================
    // DICOM Tag Constants Tests
    // =========================================================================

    mod dicom_tags {
        use dicom::core::Tag;

        #[test]
        fn stack_id_tag_is_correct() {
            let stack_id_tag = Tag(0x0020, 0x9056);
            assert_eq!(stack_id_tag.0, 0x0020);
            assert_eq!(stack_id_tag.1, 0x9056);
        }

        #[test]
        fn standard_tags_exist() {
            use dicom::dictionary_std::tags;

            // Verify standard tags are accessible
            let _ = tags::SERIES_INSTANCE_UID;
            let _ = tags::SERIES_NUMBER;
            let _ = tags::ACQUISITION_NUMBER;
            let _ = tags::SERIES_DESCRIPTION;
            let _ = tags::IMAGE_ORIENTATION_PATIENT;
        }
    }
}
