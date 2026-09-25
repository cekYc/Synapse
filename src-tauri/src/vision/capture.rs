// ============================================================
// Synapse — Screen Capture
// ============================================================
// Two Windows backends:
//
// - DXGI Desktop Duplication (preferred): GPU-side copy of the
//   requested region of the primary monitor — see `dxgi.rs`.
// - GDI BitBlt (fallback): GetDC(desktop) → CreateCompatibleDC →
//   BitBlt → GetDIBits. Works anywhere on the virtual desktop.
//
// `capture_screen` tries DXGI first and silently falls back to GDI
// when DXGI is unavailable, the region is not on the primary
// monitor, or a capture fails. Set SYNAPSE_CAPTURE=gdi to force GDI.
// ============================================================

use serde::Serialize;

/// Raw screen buffer in BGRA format
#[derive(Debug, Clone)]
pub struct ScreenBuffer {
    /// Pixel data in BGRA8 format (4 bytes per pixel)
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Bytes per row (may include padding)
    pub stride: u32,
}

impl ScreenBuffer {
    /// Get a pixel color at (x, y) as (R, G, B, A)
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<(u8, u8, u8, u8)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (y * self.stride + x * 4) as usize;
        if offset + 3 >= self.data.len() {
            return None;
        }
        // BGRA → RGBA
        Some((
            self.data[offset + 2], // R
            self.data[offset + 1], // G
            self.data[offset],     // B
            self.data[offset + 3], // A
        ))
    }
}

/// Screen capture backend in use
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)] // which variants are constructed depends on the platform
pub enum CaptureBackend {
    Dxgi,
    Gdi,
    /// Screen capture is only implemented on Windows
    Unsupported,
}

/// Capture a region of the screen (or the primary monitor if region is None)
#[cfg(target_os = "windows")]
pub fn capture_screen(region: Option<(i32, i32, u32, u32)>) -> Result<ScreenBuffer, String> {
    if let Some(buffer) = dxgi_backend::capture(region) {
        return Ok(buffer);
    }
    capture_gdi(region)
}

/// The backend serving captures right now, with the GPU adapter name for
/// DXGI. Initializes DXGI on first call, so this may take a moment.
pub fn backend_info() -> (CaptureBackend, Option<String>) {
    #[cfg(target_os = "windows")]
    {
        match dxgi_backend::status() {
            Some(adapter) => (CaptureBackend::Dxgi, Some(adapter)),
            None => (CaptureBackend::Gdi, None),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        (CaptureBackend::Unsupported, None)
    }
}

#[cfg(target_os = "windows")]
mod dxgi_backend {
    use super::super::dxgi::{DxgiCapturer, DxgiError};
    use super::ScreenBuffer;
    use parking_lot::Mutex;
    use std::time::{Duration, Instant};

    /// After DXGI fails, use GDI for this long before trying DXGI again
    /// (e.g. while the secure desktop / UAC prompt is shown)
    const RETRY_AFTER: Duration = Duration::from_secs(5);

    struct State {
        capturer: Option<DxgiCapturer>,
        last_failure: Option<Instant>,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        capturer: None,
        last_failure: None,
    });

    fn disabled() -> bool {
        std::env::var("SYNAPSE_CAPTURE").is_ok_and(|v| v.eq_ignore_ascii_case("gdi"))
    }

    /// Capture through DXGI; `None` means this capture should use GDI
    pub fn capture(region: Option<(i32, i32, u32, u32)>) -> Option<ScreenBuffer> {
        if disabled() {
            return None;
        }
        let mut state = STATE.lock();
        // A lost duplication is recreated and retried once
        for _ in 0..2 {
            let capturer = ensure(&mut state)?;
            match capturer.capture(region) {
                Ok(buffer) => return Some(buffer),
                Err(DxgiError::OutsideOutput) => return None,
                Err(DxgiError::AccessLost) => {
                    tracing::debug!("DXGI access lost, recreating duplication");
                    state.capturer = None;
                }
                Err(e) => {
                    tracing::warn!("DXGI capture failed, falling back to GDI: {e}");
                    state.capturer = None;
                    state.last_failure = Some(Instant::now());
                    return None;
                }
            }
        }
        None
    }

    /// Adapter name when DXGI capture is available
    pub fn status() -> Option<String> {
        if disabled() {
            return None;
        }
        let mut state = STATE.lock();
        ensure(&mut state).map(|c| c.adapter_name().to_string())
    }

    fn ensure(state: &mut State) -> Option<&mut DxgiCapturer> {
        if state.capturer.is_none() {
            if state.last_failure.is_some_and(|t| t.elapsed() < RETRY_AFTER) {
                return None;
            }
            match DxgiCapturer::new() {
                Ok(capturer) => {
                    tracing::info!("DXGI desktop duplication ready on {}", capturer.adapter_name());
                    state.capturer = Some(capturer);
                    state.last_failure = None;
                }
                Err(e) => {
                    tracing::warn!("DXGI desktop duplication unavailable, using GDI: {e}");
                    state.last_failure = Some(Instant::now());
                    return None;
                }
            }
        }
        state.capturer.as_mut()
    }
}

/// GDI capture of a region of the virtual desktop (primary monitor if None)
#[cfg(target_os = "windows")]
fn capture_gdi(region: Option<(i32, i32, u32, u32)>) -> Result<ScreenBuffer, String> {
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    unsafe {
        let hdc_screen = GetDC(std::ptr::null_mut());
        if hdc_screen.is_null() {
            return Err("GetDC failed".into());
        }

        let (src_x, src_y, cap_w, cap_h) = match region {
            Some((x, y, w, h)) => (x, y, w, h),
            None => {
                let w = GetSystemMetrics(SM_CXSCREEN) as u32;
                let h = GetSystemMetrics(SM_CYSCREEN) as u32;
                (0, 0, w, h)
            }
        };
        if cap_w == 0 || cap_h == 0 {
            ReleaseDC(std::ptr::null_mut(), hdc_screen);
            return Err("Capture region is empty".into());
        }

        let hdc_mem = CreateCompatibleDC(hdc_screen);
        if hdc_mem.is_null() {
            ReleaseDC(std::ptr::null_mut(), hdc_screen);
            return Err("CreateCompatibleDC failed".into());
        }

        let hbmp = CreateCompatibleBitmap(hdc_screen, cap_w as i32, cap_h as i32);
        if hbmp.is_null() {
            DeleteDC(hdc_mem);
            ReleaseDC(std::ptr::null_mut(), hdc_screen);
            return Err("CreateCompatibleBitmap failed".into());
        }

        let old_bmp = SelectObject(hdc_mem, hbmp);

        // BitBlt from screen to memory DC
        let success = BitBlt(
            hdc_mem,
            0,
            0,
            cap_w as i32,
            cap_h as i32,
            hdc_screen,
            src_x,
            src_y,
            SRCCOPY,
        );

        if success == 0 {
            SelectObject(hdc_mem, old_bmp);
            DeleteObject(hbmp);
            DeleteDC(hdc_mem);
            ReleaseDC(std::ptr::null_mut(), hdc_screen);
            return Err("BitBlt failed".into());
        }

        // Extract pixels via GetDIBits
        let stride = ((cap_w * 4 + 3) / 4) * 4; // DWORD-aligned
        let buf_size = (stride * cap_h) as usize;
        let mut pixels: Vec<u8> = vec![0u8; buf_size];

        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: cap_w as i32,
            biHeight: -(cap_h as i32), // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: buf_size as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };

        let lines = GetDIBits(
            hdc_mem,
            hbmp,
            0,
            cap_h,
            pixels.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );

        // Cleanup GDI objects
        SelectObject(hdc_mem, old_bmp);
        DeleteObject(hbmp);
        DeleteDC(hdc_mem);
        ReleaseDC(std::ptr::null_mut(), hdc_screen);

        if lines == 0 {
            return Err("GetDIBits failed".into());
        }

        Ok(ScreenBuffer {
            data: pixels,
            width: cap_w,
            height: cap_h,
            stride,
        })
    }
}

#[cfg(not(target_os = "windows"))]
pub fn capture_screen(_region: Option<(i32, i32, u32, u32)>) -> Result<ScreenBuffer, String> {
    Err("Screen capture is only supported on Windows".into())
}
