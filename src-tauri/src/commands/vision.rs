// ============================================================
// Synapse — Vision IPC Commands
// ============================================================
// Capture and matching can take tens of milliseconds (or more for
// large CPU searches). Tauri runs synchronous commands on the main
// thread, which would freeze the UI, so every command here is async
// and does its work on the blocking thread pool.
// ============================================================

use crate::vision::{self, pixel, template, VisionBackends};

async fn blocking<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("Vision task failed: {e}"))?
}

#[tauri::command]
pub async fn check_pixel(x: i32, y: i32, color: String, tolerance: u32) -> Result<bool, String> {
    blocking(move || pixel::check_pixel_color(x, y, &color, tolerance)).await
}

#[tauri::command]
pub async fn get_pixel(x: i32, y: i32) -> Result<String, String> {
    blocking(move || pixel::get_pixel_color(x, y)).await
}

#[tauri::command]
pub async fn find_pixel(
    color: String,
    tolerance: u32,
    region: Option<(i32, i32, u32, u32)>,
) -> Result<Option<(i32, i32)>, String> {
    blocking(move || pixel::find_pixel_color(region, &color, tolerance)).await
}

#[tauri::command]
pub async fn find_image(
    template_path: String,
    confidence: f64,
    region: Option<(i32, i32, u32, u32)>,
) -> Result<Option<(i32, i32, f64)>, String> {
    blocking(move || {
        Ok(template::find_template(&template_path, confidence, region)?
            .map(|m| (m.center_x, m.center_y, m.confidence)))
    })
    .await
}

/// Report the active capture/matching backends (initializes them lazily)
#[tauri::command]
pub async fn get_vision_backends() -> Result<VisionBackends, String> {
    blocking(|| Ok(vision::backends())).await
}
