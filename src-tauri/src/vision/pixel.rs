// ============================================================
// Synapse — Pixel Analysis
// ============================================================
// Provides pixel color checking and searching functionality
// used by PixelColorTrigger and PixelCheck condition nodes.
//
// Coordinates are virtual-desktop coordinates and may be negative
// (monitors left of / above the primary monitor).
// ============================================================

use super::capture::{capture_screen, ScreenBuffer};

/// An RGB color
pub type Rgb = (u8, u8, u8);

/// Check if a pixel at (x, y) matches the expected color within tolerance
pub fn check_pixel_color(
    x: i32,
    y: i32,
    expected_hex: &str,
    tolerance: u32,
) -> Result<bool, String> {
    pixel_matches(x, y, parse_hex_color(expected_hex)?, tolerance)
}

/// Check a pixel against an already parsed color
pub fn pixel_matches(x: i32, y: i32, expected: Rgb, tolerance: u32) -> Result<bool, String> {
    let buffer = capture_screen(Some((x, y, 1, 1)))?;
    let (r, g, b, _) = buffer.get_pixel(0, 0).ok_or("Pixel out of bounds")?;

    let matches = color_distance((r, g, b), expected) <= tolerance;
    tracing::debug!(
        "PixelCheck ({x},{y}): got=#{r:02X}{g:02X}{b:02X} expected=#{:02X}{:02X}{:02X} tol={tolerance} → {matches}",
        expected.0,
        expected.1,
        expected.2
    );
    Ok(matches)
}

/// Search for a pixel color in a screen region, returns the first match coords
pub fn find_pixel_color(
    region: Option<(i32, i32, u32, u32)>,
    target_hex: &str,
    tolerance: u32,
) -> Result<Option<(i32, i32)>, String> {
    find_color(region, parse_hex_color(target_hex)?, tolerance)
}

/// Search a screen region (primary monitor if `None`) for an already parsed
/// color; returns the first match in row-major order, in screen coordinates
pub fn find_color(
    region: Option<(i32, i32, u32, u32)>,
    target: Rgb,
    tolerance: u32,
) -> Result<Option<(i32, i32)>, String> {
    let buffer = capture_screen(region)?;
    let (origin_x, origin_y) = region.map(|(x, y, _, _)| (x, y)).unwrap_or((0, 0));

    Ok(first_match(&buffer, target, tolerance)
        .map(|(x, y)| (origin_x + x as i32, origin_y + y as i32)))
}

/// First pixel (row-major) within `tolerance` of `target`, in buffer coordinates
fn first_match(buffer: &ScreenBuffer, target: Rgb, tolerance: u32) -> Option<(u32, u32)> {
    for y in 0..buffer.height {
        for x in 0..buffer.width {
            if let Some((r, g, b, _)) = buffer.get_pixel(x, y) {
                if color_distance((r, g, b), target) <= tolerance {
                    return Some((x, y));
                }
            }
        }
    }
    None
}

/// Get the color of a pixel at screen position (x, y) as hex string
pub fn get_pixel_color(x: i32, y: i32) -> Result<String, String> {
    let buffer = capture_screen(Some((x, y, 1, 1)))?;
    let (r, g, b, _) = buffer.get_pixel(0, 0).ok_or("Pixel out of bounds")?;
    Ok(format!("#{:02X}{:02X}{:02X}", r, g, b))
}

/// Calculate the Manhattan color distance between two RGB colors
fn color_distance(a: Rgb, b: Rgb) -> u32 {
    let dr = (a.0 as i32 - b.0 as i32).unsigned_abs();
    let dg = (a.1 as i32 - b.1 as i32).unsigned_abs();
    let db = (a.2 as i32 - b.2 as i32).unsigned_abs();
    dr + dg + db
}

/// Parse a hex color string (#RRGGBB or RRGGBB) into (R, G, B)
pub fn parse_hex_color(hex: &str) -> Result<Rgb, String> {
    let digits = hex.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("Invalid hex color: '{hex}' (expected #RRGGBB)"));
    }
    let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).unwrap();
    Ok((channel(0), channel(2), channel(4)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(pixels: &[[u8; 3]], width: u32) -> ScreenBuffer {
        let data = pixels.iter().flat_map(|&[r, g, b]| [b, g, r, 255]).collect();
        ScreenBuffer {
            data,
            width,
            height: pixels.len() as u32 / width,
            stride: width * 4,
        }
    }

    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_hex_color("#FF8000"), Ok((255, 128, 0)));
        assert_eq!(parse_hex_color("00ff7f"), Ok((0, 255, 127)));
        assert_eq!(parse_hex_color(" #0a0B0c "), Ok((10, 11, 12)));
        for bad in ["", "#FFF", "#GG0000", "#FF00000", "#ÿÿÿ"] {
            assert!(parse_hex_color(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn first_match_respects_tolerance_and_order() {
        let img = image(
            &[[0, 0, 0], [10, 10, 10], [200, 0, 0], [205, 0, 0], [0, 0, 0], [200, 0, 0]],
            3,
        );
        assert_eq!(first_match(&img, (200, 0, 0), 0), Some((2, 0)));
        assert_eq!(first_match(&img, (203, 0, 0), 2), Some((0, 1)));
        assert_eq!(first_match(&img, (5, 5, 5), 15), Some((0, 0)));
        assert_eq!(first_match(&img, (0, 0, 255), 30), None);
    }
}
