// ============================================================
// Synapse — Vision IPC Commands
// ============================================================

use crate::vision::{pixel, template};

#[tauri::command]
pub fn check_pixel(x: i32, y: i32, color: String, tolerance: u32) -> Result<bool, String> {
    pixel::check_pixel_color(x, y, &color, tolerance)
}

#[tauri::command]
pub fn get_pixel(x: i32, y: i32) -> Result<String, String> {
    pixel::get_pixel_color(x, y)
}

#[tauri::command]
pub fn find_pixel(
    color: String,
    tolerance: u32,
    region: Option<(i32, i32, u32, u32)>,
) -> Result<Option<(u32, u32)>, String> {
    pixel::find_pixel_color(region, &color, tolerance)
}

#[tauri::command]
pub fn find_image(
    template_path: String,
    confidence: f64,
    region: Option<(i32, i32, u32, u32)>,
) -> Result<Option<(u32, u32, f64)>, String> {
    match template::find_template(&template_path, confidence, region)? {
        Some(result) => Ok(Some((result.center_x, result.center_y, result.confidence))),
        None => Ok(None),
    }
}
