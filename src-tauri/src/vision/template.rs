// ============================================================
// Synapse — Template Matching
// ============================================================
// Searches the screen for a sub-image (template) using
// normalized cross-correlation (NCC). This is a pure-Rust
// implementation — no OpenCV dependency required.
//
// Used by: ImageMatchTrigger, ImageExists condition
// ============================================================

use super::capture::{capture_screen, ScreenBuffer};
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

    if template.width > screen.width || template.height > screen.height {
        return Ok(None); // Template larger than search area
    }

    // Perform NCC template matching
    let search_w = screen.width - template.width + 1;
    let search_h = screen.height - template.height + 1;

    let mut best_score = 0.0f64;
    let mut best_x = 0u32;
    let mut best_y = 0u32;

    // Pre-compute template mean and norm
    let (tmpl_mean_r, tmpl_mean_g, tmpl_mean_b, tmpl_pixels) = compute_template_stats(&template);
    let tmpl_norm = compute_norm(&template, tmpl_mean_r, tmpl_mean_g, tmpl_mean_b);

    if tmpl_norm < 1e-10 {
        return Err("Template image is blank or nearly uniform".into());
    }

    for sy in 0..search_h {
        for sx in 0..search_w {
            let score = compute_ncc(
                &screen,
                &template,
                sx,
                sy,
                tmpl_mean_r,
                tmpl_mean_g,
                tmpl_mean_b,
                tmpl_norm,
                tmpl_pixels,
            );

            if score > best_score {
                best_score = score;
                best_x = sx;
                best_y = sy;
            }
        }
    }

    if best_score >= min_confidence {
        let (offset_x, offset_y) = match region {
            Some((rx, ry, _, _)) => (rx as u32, ry as u32),
            None => (0, 0),
        };

        Ok(Some(MatchResult {
            x: best_x + offset_x,
            y: best_y + offset_y,
            center_x: best_x + offset_x + template.width / 2,
            center_y: best_y + offset_y + template.height / 2,
            confidence: best_score,
        }))
    } else {
        tracing::debug!(
            "Template match best score {:.3} below threshold {:.3}",
            best_score,
            min_confidence
        );
        Ok(None)
    }
}

/// Compute template statistics (mean per channel, pixel count)
fn compute_template_stats(tmpl: &ScreenBuffer) -> (f64, f64, f64, u64) {
    let mut sum_r = 0u64;
    let mut sum_g = 0u64;
    let mut sum_b = 0u64;
    let mut count = 0u64;

    for y in 0..tmpl.height {
        for x in 0..tmpl.width {
            if let Some((r, g, b, _)) = tmpl.get_pixel(x, y) {
                sum_r += r as u64;
                sum_g += g as u64;
                sum_b += b as u64;
                count += 1;
            }
        }
    }

    let mean_r = sum_r as f64 / count as f64;
    let mean_g = sum_g as f64 / count as f64;
    let mean_b = sum_b as f64 / count as f64;
    (mean_r, mean_g, mean_b, count)
}

/// Compute the norm of (template - mean) for NCC denominator
fn compute_norm(tmpl: &ScreenBuffer, mean_r: f64, mean_g: f64, mean_b: f64) -> f64 {
    let mut sum_sq = 0.0f64;

    for y in 0..tmpl.height {
        for x in 0..tmpl.width {
            if let Some((r, g, b, _)) = tmpl.get_pixel(x, y) {
                let dr = r as f64 - mean_r;
                let dg = g as f64 - mean_g;
                let db = b as f64 - mean_b;
                sum_sq += dr * dr + dg * dg + db * db;
            }
        }
    }

    sum_sq.sqrt()
}

/// Compute Normalized Cross-Correlation at a specific position
fn compute_ncc(
    screen: &ScreenBuffer,
    tmpl: &ScreenBuffer,
    offset_x: u32,
    offset_y: u32,
    tmpl_mean_r: f64,
    tmpl_mean_g: f64,
    tmpl_mean_b: f64,
    tmpl_norm: f64,
    _pixel_count: u64,
) -> f64 {
    // Compute screen patch mean
    let mut sum_r = 0u64;
    let mut sum_g = 0u64;
    let mut sum_b = 0u64;
    let mut count = 0u64;

    for y in 0..tmpl.height {
        for x in 0..tmpl.width {
            if let Some((r, g, b, _)) = screen.get_pixel(offset_x + x, offset_y + y) {
                sum_r += r as u64;
                sum_g += g as u64;
                sum_b += b as u64;
                count += 1;
            }
        }
    }

    if count == 0 {
        return 0.0;
    }

    let patch_mean_r = sum_r as f64 / count as f64;
    let patch_mean_g = sum_g as f64 / count as f64;
    let patch_mean_b = sum_b as f64 / count as f64;

    // Compute cross-correlation and patch norm
    let mut cross = 0.0f64;
    let mut patch_sq = 0.0f64;

    for y in 0..tmpl.height {
        for x in 0..tmpl.width {
            let screen_px = screen.get_pixel(offset_x + x, offset_y + y);
            let tmpl_px = tmpl.get_pixel(x, y);

            if let (Some((sr, sg, sb, _)), Some((tr, tg, tb, _))) = (screen_px, tmpl_px) {
                let sdr = sr as f64 - patch_mean_r;
                let sdg = sg as f64 - patch_mean_g;
                let sdb = sb as f64 - patch_mean_b;

                let tdr = tr as f64 - tmpl_mean_r;
                let tdg = tg as f64 - tmpl_mean_g;
                let tdb = tb as f64 - tmpl_mean_b;

                cross += sdr * tdr + sdg * tdg + sdb * tdb;
                patch_sq += sdr * sdr + sdg * sdg + sdb * sdb;
            }
        }
    }

    let patch_norm = patch_sq.sqrt();
    let denom = tmpl_norm * patch_norm;

    if denom < 1e-10 {
        0.0
    } else {
        (cross / denom).clamp(0.0, 1.0)
    }
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
