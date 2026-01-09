# AGENTS.md — AI Context for DCM Toolbox

This document provides context for AI coding assistants (GitHub Copilot, Claude, Cursor, etc.) to understand the project structure, conventions, and guidelines for making changes.

## Project Overview

**DCM Toolbox** is a Rust CLI tool for converting DICOM medical image files to JPEG images or MP4 videos. It focuses on batch processing with intelligent series splitting based on DICOM metadata tags.

### Core Purpose

1. **Convert** DICOM files to JPEG or MP4
2. **Analyze** DICOM metadata to identify optimal splitting strategies
3. **Organize** output by series/groups based on configurable DICOM tags

## Architecture

```
src/
├── main.rs      # CLI entry point, argument parsing (clap)
├── convert.rs   # DICOM → JPEG/MP4 conversion logic
├── analyze.rs   # DICOM metadata analysis and recommendations
└── utils.rs     # Shared utilities (validation, sanitization, prompts)
```

### Module Responsibilities

| Module       | Purpose                                                                                       |
| ------------ | --------------------------------------------------------------------------------------------- |
| `main.rs`    | Defines CLI structure with `clap`. Parses args and dispatches to subcommands.                 |
| `convert.rs` | Handles file grouping by tags, sorting by Z-position, JPEG export, and ffmpeg video encoding. |
| `analyze.rs` | Reads DICOM tags across files, counts unique values, and recommends the best split strategy.  |
| `utils.rs`   | Input validation, filename sanitization, folder cleanup prompts, and file operations.         |

## Key Dependencies

| Crate             | Purpose                                       |
| ----------------- | --------------------------------------------- |
| `clap`            | CLI argument parsing with derive macros       |
| `dicom`           | DICOM file parsing and tag access             |
| `dicom-pixeldata` | Pixel data decoding from DICOM                |
| `image`           | Image manipulation and format conversion      |
| `anyhow`          | Error handling with context                   |
| `tempfile`        | Temporary directories for video frame staging |

### External Dependency

- **ffmpeg** — Required for MP4 video encoding. Called via `std::process::Command`.

## DICOM Tags Used

The tool works with these standard DICOM tags:

| Tag           | Name                    | Usage                           |
| ------------- | ----------------------- | ------------------------------- |
| `(0020,000E)` | SeriesInstanceUID       | Unique series identifier        |
| `(0020,0011)` | SeriesNumber            | Numeric series identifier       |
| `(0020,0012)` | AcquisitionNumber       | Acquisition grouping            |
| `(0008,103E)` | SeriesDescription       | Human-readable description      |
| `(0020,0037)` | ImageOrientationPatient | Orientation-based splitting     |
| `(0020,9056)` | StackID                 | Stack-based grouping            |
| `(0020,0032)` | ImagePositionPatient    | Z-coordinate for slice ordering |

## Code Conventions

### Error Handling

- Use `anyhow::Result` for all fallible functions
- Always add context with `.with_context(|| ...)`
- Prefer descriptive error messages that include relevant paths/values

```rust
// ✅ Good
fs::read_dir(input).with_context(|| format!("Failed to read input folder: {input:?}"))?;

// ❌ Bad
fs::read_dir(input)?;
```

### File Operations

- Validate input paths before processing (`validate_input_folder`)
- Sanitize user-derived strings before using as filenames (`sanitize_filename`)
- Always handle the case of empty directories gracefully

### CLI Patterns

- Use `clap` derive macros for argument definitions
- Default values should be sensible for typical medical imaging use cases
- Provide both long (`--option`) and short (`-o`) flags for common options

### Output Conventions

- Use `✓` for successful operations
- Use `✗` for failed operations
- Progress output: `"Processing {current}/{total}: {filename}"`
- Group output in labeled sections with `===` headers

## Testing

### Unit Tests

Located in each module under `#[cfg(test)] mod tests`.

```bash
cargo test
```

### Integration Tests

Located in `tests/integration_tests.rs`. Require sample DICOM files.

### Test Patterns

- Use `tempfile::TempDir` for temporary test directories
- Clean up test artifacts automatically via RAII
- Test both success and failure paths

### Local Testing Folders

The `in/` and `out/` folders at the project root are **gitignored** and intended for local manual testing. Place DICOM files in `in/` and use `--out ./out` to test conversions without cluttering the repository.

## Linting & Quality

### Clippy Configuration

The project uses strict Clippy settings (`clippy.toml` + `Cargo.toml`):

```toml
[lints.rust]
warnings = "deny"

[lints.clippy]
all = "deny"
```

Run before committing:

```bash
cargo clippy
cargo fmt
```

### MSRV

Minimum Supported Rust Version: **1.92.0** (see `clippy.toml`)

## Common Tasks

### Adding a New Split-By Option

1. Add variant to `SplitBy` enum in `main.rs`
2. Add corresponding DICOM tag lookup in `convert.rs` → `run()` function
3. Add tag analysis in `analyze.rs` → `run()` function
4. Update CLI help text with tag reference `(XXXX,XXXX)`

### Adding a New Output Format

1. Create conversion function in `convert.rs` (follow `convert_to_jpgs` pattern)
2. Add CLI flag in `main.rs` under `Commands::Convert`
3. Branch in `convert.rs` → `run()` based on flag
4. Handle temporary files if needed (use `tempfile` crate)

### Modifying Video Encoding

Video encoding uses ffmpeg with these settings:

- Codec: H.264 (`libx264`)
- Quality: CRF 18 (high quality)
- Preset: `slow` (better compression)
- Pixel format: `yuv420p` (compatibility)

Modify in `convert_to_video()` function. Test with various DICOM sources.

## File Flow

```
Input (.dcm files)
       │
       ▼
┌──────────────────┐
│  Read DICOM      │  dicom::object::open_file()
│  Parse Tags      │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│  Group by Tag    │  HashMap<String, Vec<PathBuf>>
│  (split-by)      │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│  Sort by Z-pos   │  ImagePositionPatient[2]
│  (slice order)   │
└────────┬─────────┘
         │
    ┌────┴────┐
    │         │
    ▼         ▼
┌───────┐ ┌───────┐
│ JPEG  │ │ Video │
│ Export│ │ (MP4) │
└───────┘ └───────┘
            │
            ▼
      ┌───────────┐
      │  ffmpeg   │
      │  encode   │
      └───────────┘
```

## Important Considerations

### Medical Imaging Context

- DICOM files may contain PHI (Protected Health Information)
- Test with anonymized/synthetic data when possible
- Maintain slice ordering accuracy (critical for medical review)

### Performance

- Large studies may have hundreds of files
- Files are processed sequentially (room for parallelization)
- Video encoding is CPU-intensive (ffmpeg handles this)

### Cross-Platform

- Path handling must work on Windows, macOS, Linux
- ffmpeg availability varies by platform
- Filename sanitization removes platform-specific invalid characters

## Debugging Tips

### DICOM Tag Issues

```bash
# Analyze files to see available tags
dcm-toolbox analyze --in ./problem-files

# Check if a specific tag exists
cargo run -- analyze --in ./files --expected-groups 3
```

### Conversion Failures

Common causes:

- Missing pixel data in DICOM file
- Unsupported transfer syntax
- Corrupt DICOM file

Check error message for specific file and failure reason.

### Video Issues

- Ensure ffmpeg is installed: `which ffmpeg`
- Check ffmpeg stderr output in error messages
- Verify frame dimensions are consistent (auto-resized to first frame)

## References

- [DICOM Standard](https://www.dicomstandard.org/)
- [dicom-rs Documentation](https://docs.rs/dicom/latest/dicom/)
- [FFmpeg Documentation](https://ffmpeg.org/documentation.html)
