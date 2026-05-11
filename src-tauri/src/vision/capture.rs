// ============================================================
// Synapse — Screen Capture (GDI BitBlt)
// ============================================================
// Fast screen capture using Windows GDI. This is the primary
// capture backend for pixel analysis and template matching.
//
// Flow: GetDC(desktop) → CreateCompatibleDC → BitBlt → GetDIBits
//
// Future: DXGI Desktop Duplication for GPU-accelerated capture
// ============================================================

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

/// Capture a region of the screen (or full screen if region is None)
#[cfg(target_os = "windows")]
pub fn capture_screen(region: Option<(i32, i32, u32, u32)>) -> Result<ScreenBuffer, String> {
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
