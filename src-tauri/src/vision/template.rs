// ============================================================
// Synapse — Template Matching
// ============================================================
// Searches the screen for a sub-image (template) using zero-mean
// normalized cross-correlation (NCC). Pure Rust — no OpenCV.
//
// Large searches run on the GPU (wgpu compute, `gpu.rs`); small
// ones, or machines without a suitable GPU, use the parallel CPU
// matcher (`ncc.rs`). Both return the same position and score.
//
// Used by: ImageMatchTrigger, ImageExists condition
// ============================================================

use super::capture::{capture_screen, ScreenBuffer};
use super::{gpu, ncc};
use std::path::Path;

/// Result of a template match
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MatchResult {
    /// Top-left X of the best match (screen coordinates)
    pub x: u32,
    /// Top-left Y of the best match (screen coordinates)
    pub y: u32,
    /// Center X of the match
    pub center_x: u32,
    /// Center Y of the match
    pub center_y: u32,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,
}

/// Search the screen for a template image
pub fn find_template(
    template_path: &str,
    min_confidence: f64,
    region: Option<(i32, i32, u32, u32)>,
) -> Result<Option<MatchResult>, String> {
    // Load the template image
    let template = load_image_as_buffer(template_path)?;

    // Capture the screen region
    let screen = capture_screen(region)?;

    let Some(best) = match_template(&screen, &template)? else {
        return Ok(None); // Template larger than search area
    };

    if best.score >= min_confidence {
        let (offset_x, offset_y) = match region {
            Some((rx, ry, _, _)) => (rx as u32, ry as u32),
            None => (0, 0),
        };

        Ok(Some(MatchResult {
            x: best.x + offset_x,
            y: best.y + offset_y,
            center_x: best.x + offset_x + template.width / 2,
            center_y: best.y + offset_y + template.height / 2,
            confidence: best.score,
        }))
    } else {
        tracing::debug!(
            "Template match best score {:.3} below threshold {:.3}",
            best.score,
            min_confidence
        );
        Ok(None)
    }
}

/// Best NCC match of `template` inside `screen` (buffer coordinates).
/// `Ok(None)` if the template does not fit; `Err` if it is blank.
pub fn match_template(
    screen: &ScreenBuffer,
    template: &ScreenBuffer,
) -> Result<Option<ncc::NccMatch>, String> {
    if template.width > screen.width || template.height > screen.height {
        return Ok(None);
    }
    let prepared = ncc::PreparedTemplate::new(template)?;
    let Some((search_w, search_h)) = ncc::search_dims(screen, &prepared) else {
        return Ok(None);
    };

    let work = search_w as u64 * search_h as u64 * prepared.pixel_count() as u64;
    if gpu::should_use(work) {
        if let Some(matcher) = gpu::global() {
            match matcher.find_best(screen, &prepared) {
                Ok(result) => return Ok(result),
                Err(e) => tracing::warn!("GPU template matching failed, using CPU: {e}"),
            }
        }
    }

    Ok(ncc::find_best_cpu(screen, &prepared))
}

/// Load a BMP/raw image file as a ScreenBuffer
/// For MVP we support simple 24-bit BMP files
fn load_image_as_buffer(path: &str) -> Result<ScreenBuffer, String> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(format!("Template image not found: {}", path.display()));
    }

    let data = std::fs::read(path).map_err(|e| format!("Failed to read template: {e}"))?;

    // Minimal BMP parser (supports 24-bit and 32-bit uncompressed)
    if data.len() < 54 || &data[0..2] != b"BM" {
        return Err("Invalid BMP file or unsupported format".into());
    }

    let data_offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
    let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]) as u32;
    let height_raw = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    let height = height_raw.unsigned_abs();
    let top_down = height_raw < 0;
    let bpp = u16::from_le_bytes([data[28], data[29]]);

    if bpp != 24 && bpp != 32 {
        return Err(format!("Unsupported BMP bit depth: {bpp}bpp (need 24 or 32)"));
    }

    let bytes_per_pixel = (bpp / 8) as u32;
    let src_stride = ((width * bytes_per_pixel + 3) / 4) * 4;
    let dst_stride = width * 4;
    let mut pixels = vec![0u8; (dst_stride * height) as usize];

    for y in 0..height {
        let src_y = if top_down { y } else { height - 1 - y };
        let src_row_start = data_offset + (src_y * src_stride) as usize;

        for x in 0..width {
            let src_px = src_row_start + (x * bytes_per_pixel) as usize;
            let dst_px = (y * dst_stride + x * 4) as usize;

            if src_px + (bytes_per_pixel as usize) <= data.len() && dst_px + 4 <= pixels.len() {
                pixels[dst_px] = data[src_px];         // B
                pixels[dst_px + 1] = data[src_px + 1]; // G
                pixels[dst_px + 2] = data[src_px + 2]; // R
                pixels[dst_px + 3] = if bpp == 32 { data[src_px + 3] } else { 255 }; // A
            }
        }
    }

    Ok(ScreenBuffer {
        data: pixels,
        width,
        height,
        stride: dst_stride,
    })
}
