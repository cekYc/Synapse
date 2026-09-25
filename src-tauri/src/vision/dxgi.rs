// ============================================================
// Synapse — Screen Capture (DXGI Desktop Duplication)
// ============================================================
// Captures the primary monitor through the DXGI Desktop
// Duplication API. The compositor hands us the desktop image as a
// GPU texture, so a capture is a GPU-side copy of just the
// requested region plus a small readback — much cheaper than GDI
// BitBlt, especially for repeated small reads (pixel checks).
//
// DXGI only delivers a frame when the desktop changed, so the
// latest frame is kept in a GPU texture and reused while nothing
// is presented. Any failure is reported to the caller, which falls
// back to GDI (see `capture.rs`).
// ============================================================

use super::capture::ScreenBuffer;
use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

/// How long to wait for the very first frame after (re)creating the
/// duplication; later captures never wait and reuse the last frame.
const FIRST_FRAME_TIMEOUT_MS: u32 = 500;
/// Staging textures kept for recently used region sizes
const STAGING_CACHE: usize = 4;

#[derive(Debug)]
pub enum DxgiError {
    /// The duplication became invalid (mode change, secure desktop,
    /// fullscreen switch…); recreate the capturer and retry.
    AccessLost,
    /// The requested region is not entirely on the duplicated monitor
    OutsideOutput,
    Other(String),
}

impl std::fmt::Display for DxgiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DxgiError::AccessLost => write!(f, "desktop duplication access lost"),
            DxgiError::OutsideOutput => write!(f, "region is outside the primary monitor"),
            DxgiError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

fn other(context: &str, e: windows::core::Error) -> DxgiError {
    if e.code() == DXGI_ERROR_ACCESS_LOST {
        DxgiError::AccessLost
    } else {
        DxgiError::Other(format!("{context}: {e}"))
    }
}

pub struct DxgiCapturer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    /// Top-left of the monitor in virtual-desktop coordinates
    origin: (i32, i32),
    width: u32,
    height: u32,
    /// GPU copy of the most recent desktop frame
    frame: ID3D11Texture2D,
    has_frame: bool,
    /// CPU-readable textures keyed by region size, most recent last
    staging: Vec<(u32, u32, ID3D11Texture2D)>,
    adapter_name: String,
}

// SAFETY: D3D11 devices are free-threaded. The immediate context and the
// duplication are only used through `&mut self`, and the capturer is kept
// behind a Mutex, so they are never accessed concurrently.
unsafe impl Send for DxgiCapturer {}

impl DxgiCapturer {
    pub fn new() -> Result<Self, DxgiError> {
        unsafe {
            let factory: IDXGIFactory1 =
                CreateDXGIFactory1().map_err(|e| other("CreateDXGIFactory1", e))?;
            let (adapter, output, desc) = find_primary_output(&factory)?;

            let rotation = desc.Rotation;
            if rotation != DXGI_MODE_ROTATION_IDENTITY && rotation != DXGI_MODE_ROTATION_UNSPECIFIED {
                return Err(DxgiError::Other("rotated displays are not supported".into()));
            }

            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN, // required when an adapter is given
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| other("D3D11CreateDevice", e))?;
            let device: ID3D11Device =
                device.ok_or_else(|| DxgiError::Other("D3D11CreateDevice returned no device".into()))?;
            let context: ID3D11DeviceContext =
                context.ok_or_else(|| DxgiError::Other("D3D11CreateDevice returned no context".into()))?;

            let output1: IDXGIOutput1 = output.cast().map_err(|e| other("IDXGIOutput1", e))?;
            let duplication = output1
                .DuplicateOutput(&device)
                .map_err(|e| other("DuplicateOutput", e))?;

            let dupl_desc = duplication.GetDesc();
            if dupl_desc.ModeDesc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
                return Err(DxgiError::Other(format!(
                    "unsupported desktop format {:?}",
                    dupl_desc.ModeDesc.Format
                )));
            }
            let (width, height) = (dupl_desc.ModeDesc.Width, dupl_desc.ModeDesc.Height);

            let frame = create_texture(&device, width, height, D3D11_USAGE_DEFAULT, 0)?;

            let adapter_desc = adapter.GetDesc1().map_err(|e| other("GetDesc1", e))?;
            let adapter_name = String::from_utf16_lossy(&adapter_desc.Description)
                .trim_end_matches('\0')
                .to_string();

            let rect = desc.DesktopCoordinates;
            Ok(Self {
                device,
                context,
                duplication,
                origin: (rect.left, rect.top),
                width,
                height,
                frame,
                has_frame: false,
                staging: Vec::new(),
                adapter_name,
            })
        }
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// Capture a region given in virtual-desktop coordinates, or the whole
    /// primary monitor when `region` is `None`
    pub fn capture(&mut self, region: Option<(i32, i32, u32, u32)>) -> Result<ScreenBuffer, DxgiError> {
        let (x, y, w, h) = region.unwrap_or((self.origin.0, self.origin.1, self.width, self.height));
        if w == 0 || h == 0 {
            return Err(DxgiError::Other("capture region is empty".into()));
        }

        // Translate to monitor-local coordinates and require full containment
        let lx = x as i64 - self.origin.0 as i64;
        let ly = y as i64 - self.origin.1 as i64;
        if lx < 0
            || ly < 0
            || lx + w as i64 > self.width as i64
            || ly + h as i64 > self.height as i64
        {
            return Err(DxgiError::OutsideOutput);
        }
        let (lx, ly) = (lx as u32, ly as u32);

        self.refresh_frame()?;
        let staging = self.staging_texture(w, h)?;

        unsafe {
            let src_box = D3D11_BOX {
                left: lx,
                top: ly,
                front: 0,
                right: lx + w,
                bottom: ly + h,
                back: 1,
            };
            self.context
                .CopySubresourceRegion(&staging, 0, 0, 0, 0, &self.frame, 0, Some(&src_box));

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| other("Map", e))?;

            let row_bytes = w as usize * 4;
            let mut data = vec![0u8; row_bytes * h as usize];
            let src = mapped.pData as *const u8;
            for row in 0..h as usize {
                std::ptr::copy_nonoverlapping(
                    src.add(row * mapped.RowPitch as usize),
                    data.as_mut_ptr().add(row * row_bytes),
                    row_bytes,
                );
            }
            self.context.Unmap(&staging, 0);

            Ok(ScreenBuffer {
                data,
                width: w,
                height: h,
                stride: w * 4,
            })
        }
    }

    /// Pull the latest desktop frame into `self.frame` if one is pending
    fn refresh_frame(&mut self) -> Result<(), DxgiError> {
        let timeout = if self.has_frame { 0 } else { FIRST_FRAME_TIMEOUT_MS };
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;

        let acquired = unsafe { self.duplication.AcquireNextFrame(timeout, &mut info, &mut resource) };
        match acquired {
            Ok(()) => {
                // A frame with LastPresentTime == 0 only carries pointer
                // updates; keep the previous image unless we have none yet.
                let stored = if info.LastPresentTime != 0 || !self.has_frame {
                    self.store_frame(resource)
                } else {
                    Ok(())
                };
                // Always hand the frame back, even if the copy failed
                let released = unsafe { self.duplication.ReleaseFrame() };
                stored?;
                released.map_err(|e| other("ReleaseFrame", e))
            }
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                if self.has_frame {
                    Ok(()) // desktop unchanged since the last frame
                } else {
                    Err(DxgiError::Other("no desktop frame received yet".into()))
                }
            }
            Err(e) => Err(other("AcquireNextFrame", e)),
        }
    }

    fn store_frame(&mut self, resource: Option<IDXGIResource>) -> Result<(), DxgiError> {
        let resource =
            resource.ok_or_else(|| DxgiError::Other("AcquireNextFrame returned no resource".into()))?;
        let texture: ID3D11Texture2D = resource.cast().map_err(|e| other("frame texture", e))?;
        unsafe { self.context.CopyResource(&self.frame, &texture) };
        self.has_frame = true;
        Ok(())
    }

    fn staging_texture(&mut self, w: u32, h: u32) -> Result<ID3D11Texture2D, DxgiError> {
        if let Some(pos) = self.staging.iter().position(|(sw, sh, _)| (*sw, *sh) == (w, h)) {
            let entry = self.staging.remove(pos);
            let texture = entry.2.clone();
            self.staging.push(entry);
            return Ok(texture);
        }
        let texture = create_texture(
            &self.device,
            w,
            h,
            D3D11_USAGE_STAGING,
            D3D11_CPU_ACCESS_READ.0 as u32,
        )?;
        if self.staging.len() >= STAGING_CACHE {
            self.staging.remove(0);
        }
        self.staging.push((w, h, texture.clone()));
        Ok(texture)
    }
}

/// The output whose desktop rectangle starts at (0, 0) — the primary monitor
unsafe fn find_primary_output(
    factory: &IDXGIFactory1,
) -> Result<(IDXGIAdapter1, IDXGIOutput, DXGI_OUTPUT_DESC), DxgiError> {
    let mut adapter_index = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(adapter_index) } {
        let mut output_index = 0;
        while let Ok(output) = unsafe { adapter.EnumOutputs(output_index) } {
            if let Ok(desc) = unsafe { output.GetDesc() } {
                let r = desc.DesktopCoordinates;
                if desc.AttachedToDesktop.as_bool() && r.left == 0 && r.top == 0 {
                    return Ok((adapter, output, desc));
                }
            }
            output_index += 1;
        }
        adapter_index += 1;
    }
    Err(DxgiError::Other("primary monitor output not found".into()))
}

fn create_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    usage: D3D11_USAGE,
    cpu_access: u32,
) -> Result<ID3D11Texture2D, DxgiError> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: usage,
        BindFlags: 0,
        CPUAccessFlags: cpu_access,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }
        .map_err(|e| other("CreateTexture2D", e))?;
    texture.ok_or_else(|| DxgiError::Other("CreateTexture2D returned no texture".into()))
}
