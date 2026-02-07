//! DICOM to STL 3D model conversion module.
//!
//! Converts a group of DICOM slices into a 3D surface mesh (binary STL format)
//! using the Marching Cubes algorithm. Supports optional Gaussian smoothing
//! and automatic Otsu thresholding for isosurface extraction.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use dicom::dictionary_std::tags;
use dicom::object::open_file;
use dicom_pixeldata::PixelDecoder;
use lin_alg::f32::Vec3;
use mcubes::{MarchingCubes, MeshSide};

/// Minimum number of slices required for meaningful 3D reconstruction.
const MIN_SLICES_FOR_3D: usize = 5;

/// Default slice thickness when metadata is unavailable (mm).
const DEFAULT_SLICE_THICKNESS: f32 = 1.0;

/// Default pixel spacing when metadata is unavailable (mm).
const DEFAULT_PIXEL_SPACING: f32 = 1.0;

/// Number of histogram bins for Otsu thresholding.
const HISTOGRAM_BINS: usize = 256;

/// Holds the 3D volumetric data built from stacked DICOM slices.
struct VolumeData {
    /// Flat array of voxel intensities (0.0–255.0), packed X-fastest.
    values: Vec<f32>,
    /// Number of columns (X dimension).
    cols: usize,
    /// Number of rows (Y dimension).
    rows: usize,
    /// Number of slices (Z dimension).
    slices: usize,
    /// Physical pixel spacing along X in mm.
    spacing_x: f32,
    /// Physical pixel spacing along Y in mm.
    spacing_y: f32,
    /// Physical slice spacing along Z in mm.
    spacing_z: f32,
}

/// Convert a group of sorted DICOM files into a binary STL 3D model.
pub fn convert_to_stl(
    dcm_files: &[PathBuf],
    output_dir: &Path,
    iso_level: Option<f32>,
    smooth_sigma: f32,
) -> Result<()> {
    if dcm_files.len() < MIN_SLICES_FOR_3D {
        anyhow::bail!(
            "Need at least {MIN_SLICES_FOR_3D} slices for 3D reconstruction, got {}",
            dcm_files.len()
        );
    }

    println!("  Building 3D volume from {} slices...", dcm_files.len());
    let volume = build_volume(dcm_files)?;
    println!(
        "  Volume: {}x{}x{} (spacing: {:.2}x{:.2}x{:.2} mm)",
        volume.cols,
        volume.rows,
        volume.slices,
        volume.spacing_x,
        volume.spacing_y,
        volume.spacing_z
    );

    // Apply Gaussian smoothing if sigma > 0
    let smoothed_values = if smooth_sigma > 0.0 {
        println!("  Applying Gaussian smoothing (sigma={smooth_sigma:.2})...");
        gaussian_smooth_3d(
            &volume.values,
            volume.cols,
            volume.rows,
            volume.slices,
            smooth_sigma,
        )
    } else {
        volume.values.clone()
    };

    // Determine iso level via Otsu or use user-provided value
    let threshold = iso_level.unwrap_or_else(|| {
        let t = otsu_threshold(&smoothed_values);
        println!("  Auto-detected Otsu threshold: {t:.2}");
        t
    });
    if iso_level.is_some() {
        println!("  Using user-specified iso-level: {threshold:.2}");
    }

    println!("  Running Marching Cubes...");
    let mc = MarchingCubes::new(
        (volume.cols, volume.rows, volume.slices),
        (
            volume.cols as f32 * volume.spacing_x,
            volume.rows as f32 * volume.spacing_y,
            volume.slices as f32 * volume.spacing_z,
        ),
        (volume.cols as f32, volume.rows as f32, volume.slices as f32),
        Vec3::new_zero(),
        smoothed_values,
        threshold,
    )?;
    let mesh = mc.generate(MeshSide::OutsideOnly);

    let vertex_count = mesh.vertices.len();
    let triangle_count = mesh.indices.len() / 3;

    if triangle_count == 0 {
        anyhow::bail!(
            "Marching Cubes produced no triangles. Try adjusting --iso-level (current: {threshold:.2})"
        );
    }

    println!("  Mesh: {vertex_count} vertices, {triangle_count} triangles");

    // Write binary STL
    let stl_name = output_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let stl_path = output_dir.join(format!("{stl_name}.stl"));
    write_stl_file(&mesh, &stl_path)?;

    println!("✓ STL saved to: {stl_path:?}");
    Ok(())
}

/// Build a 3D volume from sorted DICOM slices.
///
/// Each slice is converted to 8-bit grayscale. Pixel spacing and slice
/// thickness are extracted from DICOM metadata when available.
fn build_volume(dcm_files: &[PathBuf]) -> Result<VolumeData> {
    // Read metadata from the first file to establish dimensions
    let first_obj = open_file(&dcm_files[0])
        .with_context(|| format!("Failed to open first DICOM file: {:?}", dcm_files[0]))?;

    let rows = first_obj
        .element(tags::ROWS)
        .ok()
        .and_then(|e| e.to_int::<u32>().ok())
        .unwrap_or(0) as usize;
    let cols = first_obj
        .element(tags::COLUMNS)
        .ok()
        .and_then(|e| e.to_int::<u32>().ok())
        .unwrap_or(0) as usize;

    if rows == 0 || cols == 0 {
        anyhow::bail!("Invalid image dimensions: {cols}x{rows}");
    }

    // Extract pixel spacing (Y\X format in DICOM)
    let (spacing_y, spacing_x) = first_obj
        .element(tags::PIXEL_SPACING)
        .ok()
        .and_then(|e| e.to_str().ok())
        .and_then(|s| {
            let parts: Vec<f32> = s
                .split('\\')
                .filter_map(|v| v.trim().parse::<f32>().ok())
                .collect();
            if parts.len() >= 2 {
                Some((parts[0], parts[1]))
            } else {
                None
            }
        })
        .unwrap_or((DEFAULT_PIXEL_SPACING, DEFAULT_PIXEL_SPACING));

    // Compute Z spacing from first two slice positions, or fall back to SliceThickness
    let spacing_z = compute_slice_spacing(dcm_files).unwrap_or_else(|| {
        first_obj
            .element(tags::SLICE_THICKNESS)
            .ok()
            .and_then(|e| e.to_str().ok())
            .and_then(|s| s.trim().parse::<f32>().ok())
            .unwrap_or(DEFAULT_SLICE_THICKNESS)
    });

    let num_slices = dcm_files.len();
    let slice_size = cols * rows;
    let mut values = vec![0.0_f32; slice_size * num_slices];

    for (z, dcm_path) in dcm_files.iter().enumerate() {
        let dicom_obj = open_file(dcm_path)
            .with_context(|| format!("Failed to open DICOM file: {dcm_path:?}"))?;

        let pixel_data = dicom_obj
            .decode_pixel_data()
            .with_context(|| format!("Failed to decode pixel data: {dcm_path:?}"))?;

        let img = pixel_data
            .to_dynamic_image(0)
            .with_context(|| format!("Failed to convert to image: {dcm_path:?}"))?;

        let gray = img.to_luma8();

        // Ensure consistent dimensions
        if gray.width() as usize != cols || gray.height() as usize != rows {
            anyhow::bail!(
                "Inconsistent slice dimensions: expected {cols}x{rows}, got {}x{} in {:?}",
                gray.width(),
                gray.height(),
                dcm_path
            );
        }

        // Pack into the flat volume array
        // mcubes indexes as: values[x + y * cols + z * cols * rows]
        // (X varies fastest, Z varies slowest)
        for y in 0..rows {
            for x in 0..cols {
                let pixel_val = f32::from(gray.get_pixel(x as u32, y as u32).0[0]);
                let idx = x + y * cols + z * cols * rows;
                values[idx] = pixel_val;
            }
        }

        println!(
            "  ✓ Loaded slice {}/{}: {:?}",
            z + 1,
            num_slices,
            dcm_path.file_name().unwrap()
        );
    }

    Ok(VolumeData {
        values,
        cols,
        rows,
        slices: num_slices,
        spacing_x,
        spacing_y,
        spacing_z,
    })
}

/// Compute the Z spacing between slices from ImagePositionPatient tags.
fn compute_slice_spacing(dcm_files: &[PathBuf]) -> Option<f32> {
    if dcm_files.len() < 2 {
        return None;
    }

    let z_pos = |path: &PathBuf| -> Option<f64> {
        let obj = open_file(path).ok()?;
        let s = obj
            .element(tags::IMAGE_POSITION_PATIENT)
            .ok()?
            .to_str()
            .ok()?;
        let coords: Vec<f64> = s
            .split('\\')
            .filter_map(|v| v.trim().parse::<f64>().ok())
            .collect();
        coords.get(2).copied()
    };

    let z0 = z_pos(&dcm_files[0])?;
    let z1 = z_pos(&dcm_files[1])?;
    let spacing = (z1 - z0).abs();

    if spacing > 0.0 {
        Some(spacing as f32)
    } else {
        None
    }
}

/// Compute the optimal threshold using Otsu's method.
///
/// Maximizes inter-class variance on a 256-bin histogram to find the
/// threshold that best separates foreground from background.
fn otsu_threshold(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }

    // Find value range
    let min_val = values.iter().copied().reduce(f32::min).unwrap_or(0.0);
    let max_val = values.iter().copied().reduce(f32::max).unwrap_or(255.0);
    let range = max_val - min_val;

    if range <= 0.0 {
        return min_val;
    }

    // Build histogram
    let mut histogram = [0u64; HISTOGRAM_BINS];
    let scale = (HISTOGRAM_BINS - 1) as f32 / range;

    for &val in values {
        let bin = ((val - min_val) * scale) as usize;
        let bin = bin.min(HISTOGRAM_BINS - 1);
        histogram[bin] += 1;
    }

    let total = values.len() as f64;

    // Compute total weighted sum
    let total_sum: f64 = histogram
        .iter()
        .enumerate()
        .map(|(i, &count)| i as f64 * count as f64)
        .sum();

    let mut best_threshold_first = 0;
    let mut best_threshold_last = 0;
    let mut best_variance = 0.0_f64;
    let mut background_count = 0.0_f64;
    let mut background_sum = 0.0_f64;

    for (t, &count) in histogram.iter().enumerate() {
        background_count += count as f64;
        if background_count == 0.0 {
            continue;
        }

        let foreground_count = total - background_count;
        if foreground_count == 0.0 {
            break;
        }

        background_sum += t as f64 * count as f64;
        let foreground_sum = total_sum - background_sum;

        let mean_bg = background_sum / background_count;
        let mean_fg = foreground_sum / foreground_count;
        let diff = mean_bg - mean_fg;

        let variance = background_count * foreground_count * diff * diff;

        if variance > best_variance {
            best_variance = variance;
            best_threshold_first = t;
            best_threshold_last = t;
        } else if (variance - best_variance).abs() < f64::EPSILON * best_variance.abs() {
            best_threshold_last = t;
        }
    }

    // Average first and last bins with max variance for symmetric distributions
    let best_threshold = (best_threshold_first + best_threshold_last) / 2;

    // Convert bin index back to value
    min_val + best_threshold as f32 / scale
}

/// Apply 3D Gaussian smoothing using separable convolution.
///
/// Performs three sequential 1D convolutions (X, Y, Z) for efficiency.
/// Kernel size is determined by `6 * sigma + 1` (covers 99.7% of the distribution).
fn gaussian_smooth_3d(
    values: &[f32],
    cols: usize,
    rows: usize,
    slices: usize,
    sigma: f32,
) -> Vec<f32> {
    let kernel = build_gaussian_kernel(sigma);
    let half = kernel.len() / 2;

    // Pass 1: smooth along X (cols dimension)
    let mut pass_x = values.to_vec();
    for z in 0..slices {
        for y in 0..rows {
            for x in 0..cols {
                let mut sum = 0.0_f32;
                let mut weight = 0.0_f32;
                for (k, &kval) in kernel.iter().enumerate() {
                    let xi = x as isize + k as isize - half as isize;
                    if xi >= 0 && (xi as usize) < cols {
                        let idx = xi as usize + y * cols + z * cols * rows;
                        sum += values[idx] * kval;
                        weight += kval;
                    }
                }
                let idx = x + y * cols + z * cols * rows;
                pass_x[idx] = sum / weight;
            }
        }
    }

    // Pass 2: smooth along Y (rows dimension)
    let mut pass_y = pass_x.clone();
    for z in 0..slices {
        for y in 0..rows {
            for x in 0..cols {
                let mut sum = 0.0_f32;
                let mut weight = 0.0_f32;
                for (k, &kval) in kernel.iter().enumerate() {
                    let yi = y as isize + k as isize - half as isize;
                    if yi >= 0 && (yi as usize) < rows {
                        let idx = x + yi as usize * cols + z * cols * rows;
                        sum += pass_x[idx] * kval;
                        weight += kval;
                    }
                }
                let idx = x + y * cols + z * cols * rows;
                pass_y[idx] = sum / weight;
            }
        }
    }

    // Pass 3: smooth along Z (slices dimension)
    let mut pass_z = pass_y.clone();
    for z in 0..slices {
        for y in 0..rows {
            for x in 0..cols {
                let mut sum = 0.0_f32;
                let mut weight = 0.0_f32;
                for (k, &kval) in kernel.iter().enumerate() {
                    let zi = z as isize + k as isize - half as isize;
                    if zi >= 0 && (zi as usize) < slices {
                        let idx = x + y * cols + zi as usize * cols * rows;
                        sum += pass_y[idx] * kval;
                        weight += kval;
                    }
                }
                let idx = x + y * cols + z * cols * rows;
                pass_z[idx] = sum / weight;
            }
        }
    }

    pass_z
}

/// Build a 1D Gaussian kernel with the given sigma.
fn build_gaussian_kernel(sigma: f32) -> Vec<f32> {
    let radius = (3.0 * sigma).ceil() as usize;
    let size = 2 * radius + 1;
    let mut kernel = Vec::with_capacity(size);

    let two_sigma_sq = 2.0 * sigma * sigma;

    for i in 0..size {
        let x = i as f32 - radius as f32;
        kernel.push((-x * x / two_sigma_sq).exp());
    }

    // Normalize
    let sum: f32 = kernel.iter().sum();
    for val in &mut kernel {
        *val /= sum;
    }

    kernel
}

/// Write a marching cubes mesh as a binary STL file.
fn write_stl_file(mesh: &mcubes::Mesh, path: &Path) -> Result<()> {
    let indices = &mesh.indices;
    let vertices = &mesh.vertices;

    if !indices.len().is_multiple_of(3) {
        anyhow::bail!(
            "Invalid mesh: index count ({}) is not a multiple of 3",
            indices.len()
        );
    }

    let triangles = indices.chunks(3).map(|tri| {
        let v0 = &vertices[tri[0]];
        let v1 = &vertices[tri[1]];
        let v2 = &vertices[tri[2]];

        // Compute face normal from cross product
        let edge1 = [
            v1.posit.x - v0.posit.x,
            v1.posit.y - v0.posit.y,
            v1.posit.z - v0.posit.z,
        ];
        let edge2 = [
            v2.posit.x - v0.posit.x,
            v2.posit.y - v0.posit.y,
            v2.posit.z - v0.posit.z,
        ];

        let nx = edge1[1] * edge2[2] - edge1[2] * edge2[1];
        let ny = edge1[2] * edge2[0] - edge1[0] * edge2[2];
        let nz = edge1[0] * edge2[1] - edge1[1] * edge2[0];

        // Normalize
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        let (nx, ny, nz) = if len > 0.0 {
            (nx / len, ny / len, nz / len)
        } else {
            (0.0, 0.0, 1.0)
        };

        stl_io::Triangle {
            normal: stl_io::Normal::new([nx, ny, nz]),
            vertices: [
                stl_io::Vertex::new([v0.posit.x, v0.posit.y, v0.posit.z]),
                stl_io::Vertex::new([v1.posit.x, v1.posit.y, v1.posit.z]),
                stl_io::Vertex::new([v2.posit.x, v2.posit.y, v2.posit.z]),
            ],
        }
    });

    let mut file = BufWriter::new(
        File::create(path).with_context(|| format!("Failed to create STL file: {path:?}"))?,
    );
    stl_io::write_stl(&mut file, triangles)
        .with_context(|| format!("Failed to write STL data: {path:?}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Otsu Threshold Tests
    // =========================================================================

    mod otsu {
        use super::*;

        #[test]
        fn bimodal_distribution_finds_midpoint() {
            // 50 values at 50.0, 50 values at 200.0
            let mut values = vec![50.0_f32; 50];
            values.extend(vec![200.0_f32; 50]);

            let threshold = otsu_threshold(&values);

            // Threshold should be between the two peaks
            assert!(
                threshold > 50.0 && threshold < 200.0,
                "Expected threshold between 50 and 200, got {threshold}"
            );
        }

        #[test]
        fn uniform_values_returns_minimum() {
            let values = vec![100.0_f32; 100];
            let threshold = otsu_threshold(&values);
            assert!(
                (threshold - 100.0).abs() < f32::EPSILON,
                "Expected ~100.0 for uniform data, got {threshold}"
            );
        }

        #[test]
        fn empty_input_returns_zero() {
            assert!((otsu_threshold(&[]) - 0.0).abs() < f32::EPSILON);
        }

        #[test]
        fn single_value_returns_that_value() {
            let threshold = otsu_threshold(&[42.0]);
            assert!(
                (threshold - 42.0).abs() < f32::EPSILON,
                "Expected 42.0, got {threshold}"
            );
        }
    }

    // =========================================================================
    // Gaussian Smoothing Tests
    // =========================================================================

    mod smoothing {
        use super::*;

        #[test]
        fn kernel_sums_to_one() {
            let kernel = build_gaussian_kernel(1.0);
            let sum: f32 = kernel.iter().sum();
            assert!((sum - 1.0).abs() < 1e-5, "Kernel sum {sum} should be ~1.0");
        }

        #[test]
        fn kernel_is_symmetric() {
            let kernel = build_gaussian_kernel(2.0);
            let n = kernel.len();
            for i in 0..n / 2 {
                assert!(
                    (kernel[i] - kernel[n - 1 - i]).abs() < 1e-6,
                    "Kernel not symmetric at index {i}"
                );
            }
        }

        #[test]
        fn smoothing_preserves_uniform_volume() {
            let values = vec![100.0_f32; 3 * 3 * 3];
            let smoothed = gaussian_smooth_3d(&values, 3, 3, 3, 1.0);

            for (i, &val) in smoothed.iter().enumerate() {
                assert!(
                    (val - 100.0).abs() < 1e-3,
                    "Uniform volume changed at index {i}: {val}"
                );
            }
        }

        #[test]
        fn smoothing_reduces_spike() {
            // Volume with a central spike
            let (cols, rows, slices) = (5, 5, 5);
            let mut values = vec![0.0_f32; cols * rows * slices];
            // Set center voxel to high value (x=2, y=2, z=2)
            let center = 2 + 2 * cols + 2 * cols * rows;
            values[center] = 255.0;

            let smoothed = gaussian_smooth_3d(&values, cols, rows, slices, 1.0);

            // Center should be reduced
            assert!(
                smoothed[center] < 255.0,
                "Spike was not smoothed: {}",
                smoothed[center]
            );
            // Center should still be the maximum
            let max = smoothed.iter().copied().reduce(f32::max).unwrap();
            assert!(
                (smoothed[center] - max).abs() < 1e-6,
                "Center should remain the maximum"
            );
        }
    }

    // =========================================================================
    // Validation Tests
    // =========================================================================

    mod validation {
        use super::*;

        #[test]
        fn rejects_too_few_slices() {
            let files: Vec<PathBuf> = (0..3)
                .map(|i| PathBuf::from(format!("test_{i}.dcm")))
                .collect();
            let result = convert_to_stl(&files, Path::new("/tmp/out"), None, 1.0);
            assert!(result.is_err());
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("at least"),
                "Expected 'at least' in error: {err}"
            );
        }
    }
}
