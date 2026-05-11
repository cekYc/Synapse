// ============================================================
// Synapse — Pixel Analysis
// ============================================================
// Provides pixel color checking and searching functionality
// used by PixelColorTrigger and PixelCheck condition nodes.
// ============================================================

use super::capture::capture_screen;

/// Check if a pixel at (x, y) matches the expected color within tolerance
pub fn check_pixel_color(
    x: i32,
    y: i32,
    expected_hex: &str,
    tolerance: u32,
) -> Result<bool, String> {
    let buffer = capture_screen(Some((x, y, 1, 1)))?;
    let (r, g, b, _) = buffer.get_pixel(0, 0).ok_or("Pixel out of bounds")?;
    let (er, eg, eb) = parse_hex_color(expected_hex)?;

    let matches = color_distance(r, g, b, er, eg, eb) <= tolerance;
    tracing::debug!(
        "PixelCheck ({x},{y}): got=#{:02X}{:02X}{:02X} expected={expected_hex} tol={tolerance} → {matches}",
        r, g, b
    );
    Ok(matches)
}

/// Search for a pixel color in a screen region, returns the first match coords
pub fn find_pixel_color(
    region: Option<(i32, i32, u32, u32)>,
    target_hex: &str,
    tolerance: u32,
) -> Result<Option<(u32, u32)>, String> {
    let buffer = capture_screen(region)?;
    let (tr, tg, tb) = parse_hex_color(target_hex)?;

    for y in 0..buffer.height {
        for x in 0..buffer.width {
            if let Some((r, g, b, _)) = buffer.get_pixel(x, y) {
                if color_distance(r, g, b, tr, tg, tb) <= tolerance {
                    // Convert back to absolute screen coordinates
                    let (offset_x, offset_y) = match region {
                        Some((rx, ry, _, _)) => (rx as u32, ry as u32),
                        None => (0, 0),
                    };
                    return Ok(Some((x + offset_x, y + offset_y)));
                }
            }
        }
    }

    Ok(None)
}

/// Get the color of a pixel at screen position (x, y) as hex string
pub fn get_pixel_color(x: i32, y: i32) -> Result<String, String> {
    let buffer = capture_screen(Some((x, y, 1, 1)))?;
    let (r, g, b, _) = buffer.get_pixel(0, 0).ok_or("Pixel out of bounds")?;
    Ok(format!("#{:02X}{:02X}{:02X}", r, g, b))
}

/// Calculate the Manhattan color distance between two RGB colors
fn color_distance(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) -> u32 {
    let dr = (r1 as i32 - r2 as i32).unsigned_abs();
    let dg = (g1 as i32 - g2 as i32).unsigned_abs();
    let db = (b1 as i32 - b2 as i32).unsigned_abs();
    dr + dg + db
}

/// Parse a hex color string (#RRGGBB or RRGGBB) into (R, G, B)
fn parse_hex_color(hex: &str) -> Result<(u8, u8, u8), String> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Err(format!("Invalid hex color: #{hex}"));
    }
    let r = u8::from_str_radix(&hex[0..2], 16).map_err(|e| format!("Invalid hex: {e}"))?;
    let g = u8::from_str_radix(&hex[2..4], 16).map_err(|e| format!("Invalid hex: {e}"))?;
    let b = u8::from_str_radix(&hex[4..6], 16).map_err(|e| format!("Invalid hex: {e}"))?;
    Ok((r, g, b))
}
