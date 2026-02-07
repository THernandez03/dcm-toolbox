# 💼 DCM Toolbox

A fast, command-line tool for converting DICOM (.dcm) medical image files to JPEG images, MP4 videos, or STL 3D models, written in Rust.

## Features

- **Batch Conversion** — Convert entire directories of DICOM files at once
- **Multiple Output Formats** — Export as JPEG images, MP4 video, or STL 3D models
- **STL 3D Models** — Generate 3D surface meshes via Marching Cubes with automatic Otsu thresholding and optional Gaussian smoothing
- **Smart Series Splitting** — Automatically organize output by series, acquisition, orientation, and more
- **DICOM Analysis** — Analyze DICOM metadata to identify the best tag for splitting your files
- **Configurable** — Control video frame rate, STL iso-level/smoothing, output folder structure, and more
- **Safe Defaults** — Prompts before overwriting existing files (with force mode available)

## Installation

### From Source

Ensure you have [Rust](https://www.rust-lang.org/tools/install) installed (1.70+), then:

```bash
git clone https://github.com/THernandez03/dcm-tool.git
cd dcm-tool
cargo build --release
```

The binary will be available at `target/release/dcm-toolbox`.

### Prerequisites

JPEG and STL output require no external dependencies. For video output (MP4), you need `ffmpeg` installed and available in your PATH:

```bash
# Ubuntu/Debian
sudo apt install ffmpeg

# macOS
brew install ffmpeg

# Windows (via Chocolatey)
choco install ffmpeg
```

## Usage

### Convert DICOM to JPEG

Convert all `.dcm` files in a folder to JPEG images:

```bash
dcm-toolbox convert --in ./dicom-folder --out ./output-folder jpeg
```

Output files are organized into subfolders by series number.

### Convert DICOM to Video

Generate an MP4 video from DICOM files:

```bash
dcm-toolbox convert --in ./dicom-folder --out ./output-folder video
```

Adjust the frame rate (default: 10 fps):

```bash
dcm-toolbox convert --in ./dicom-folder --out ./output-folder video --fps 24
```

### Convert DICOM to STL (3D Model)

Generate a 3D surface mesh as a binary STL file:

```bash
dcm-toolbox convert --in ./dicom-folder --out ./output-folder stl
```

By default, the iso-level is computed automatically using Otsu's method and Gaussian smoothing (sigma=1.0) is applied. Override these defaults:

```bash
# Set a specific iso-level threshold
dcm-toolbox convert --in ./in --out ./out stl --iso-level 128

# Disable smoothing for raw output
dcm-toolbox convert --in ./in --out ./out stl --smooth 0

# Full control
dcm-toolbox convert --in ./in --out ./out stl --iso-level 200 --smooth 2.0
```

> **Note:** At least 5 DICOM slices are required for 3D reconstruction.

### Split by Different Tags

By default, files are split by `SeriesNumber`. You can choose a different tag:

```bash
# Split by Series Instance UID
dcm-toolbox convert --in ./in --out ./out --split-by series-uid jpeg

# Split by Acquisition Number
dcm-toolbox convert --in ./in --out ./out --split-by acquisition-number jpeg

# Split by Series Description
dcm-toolbox convert --in ./in --out ./out --split-by description jpeg

# Split by Image Orientation
dcm-toolbox convert --in ./in --out ./out --split-by orientation jpeg

# Split by Stack ID
dcm-toolbox convert --in ./in --out ./out --split-by stack-id jpeg
```

Split-by works with all output formats (jpeg, video, stl). Each group produces its own output file(s).

### Analyze DICOM Files

Not sure which tag to use for splitting? Use the `analyze` command to inspect your DICOM files:

```bash
dcm-toolbox analyze --in ./dicom-folder
```

If you know how many groups/series you expect, the tool will highlight matching tags:

```bash
dcm-toolbox analyze --in ./dicom-folder --expected-groups 4
```

### Force Overwrite

Skip confirmation prompts and always clean output folders:

```bash
dcm-toolbox convert --in ./in --out ./out --force jpeg
```

## Command Reference

### `convert`

Convert DICOM files to JPEG images, MP4 video, or STL 3D models.

```
dcm-toolbox convert [SHARED_OPTIONS] <FORMAT> [FORMAT_OPTIONS]
```

**Shared Options** (apply to all formats):

| Option             | Short | Description                          | Default         |
| ------------------ | ----- | ------------------------------------ | --------------- |
| `--in <PATH>`      |       | Input folder containing .dcm files   | Required        |
| `--out <PATH>`     |       | Output folder for converted files    | Required        |
| `--split-by <TAG>` | `-s`  | Tag to split files by                | `series-number` |
| `--force`          | `-f`  | Force overwrite without confirmation | `false`         |

**Formats:**

| Subcommand | Description                             |
| ---------- | --------------------------------------- |
| `jpeg`     | Convert to JPEG images (default format) |
| `video`    | Generate MP4 video                      |
| `stl`      | Generate STL 3D model                   |

**`video` options:**

| Option      | Description                 | Default |
| ----------- | --------------------------- | ------- |
| `--fps <N>` | Frames per second for video | `10`    |

**`stl` options:**

| Option             | Description                             | Default     |
| ------------------ | --------------------------------------- | ----------- |
| `--iso-level <N>`  | ISO surface level for Marching Cubes    | Auto (Otsu) |
| `--smooth <SIGMA>` | Gaussian smoothing sigma (0 to disable) | `1.0`       |

**Split-by options:**

- `series-number` — SeriesNumber tag (0020,0011)
- `series-uid` — SeriesInstanceUID tag (0020,000E)
- `acquisition-number` — AcquisitionNumber tag (0020,0012)
- `description` — SeriesDescription tag (0008,103E)
- `orientation` — ImageOrientationPatient tag (0020,0037)
- `stack-id` — StackID tag (0020,9056)

### `analyze`

Analyze DICOM files to find the best tag for splitting.

| Option                  | Short | Description                        | Default  |
| ----------------------- | ----- | ---------------------------------- | -------- |
| `--in <PATH>`           |       | Input folder containing .dcm files | Required |
| `--expected-groups <N>` | `-g`  | Expected number of series/groups   | None     |

## Examples

### Basic Conversion

```bash
# Convert CT scan slices to JPEGs
dcm-toolbox convert --in ~/scans/ct-chest --out ~/exports/ct-chest jpeg

# Convert MRI series to video at 15 fps
dcm-toolbox convert --in ~/scans/mri-brain --out ~/exports/mri-brain video --fps 15

# Generate a 3D model from CT slices
dcm-toolbox convert --in ~/scans/ct-chest --out ~/exports/ct-chest stl
```

### Workflow with Analysis

```bash
# First, analyze the DICOM files
dcm-toolbox analyze --in ~/scans/mixed-series

# Output shows SeriesDescription has 3 unique values
# Use that for splitting
dcm-toolbox convert --in ~/scans/mixed-series --out ~/exports --split-by description jpeg
```

## Output Structure

When converting, the tool creates a subfolder for each unique value of the split tag:

```
output-folder/
├── series_001/
│   ├── 0001.jpg
│   ├── 0002.jpg
│   └── ...
├── series_002/
│   └── video.mp4      # convert ... video
└── series_003/
    └── series_003.stl  # convert ... stl
```

Files within each series are sorted by their ImagePositionPatient Z-coordinate for correct slice ordering.

## Project Structure

```
src/
├── main.rs           # CLI entry point and argument parsing (clap)
├── analyze.rs        # DICOM metadata analysis and tag recommendations
├── convert.rs        # Shared conversion pipeline (grouping, sorting, CLI types)
├── convert/
│   ├── jpeg.rs       # DICOM → JPEG image conversion
│   ├── video.rs      # DICOM → MP4 video conversion (via ffmpeg)
│   └── stl.rs        # DICOM → STL 3D model (Marching Cubes)
└── utils.rs          # Shared utilities (validation, sanitization, prompts)
```

Each command (`analyze`, `convert`) maps to its own module. Each output format (`jpeg`, `video`, `stl`) lives in its own submodule under `convert/`. Adding a new format means creating a new file under `convert/` and wiring it into `convert.rs`.

## License

This project is open source. See the repository for license details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request
