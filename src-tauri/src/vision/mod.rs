pub mod capture;
#[cfg(target_os = "windows")]
mod dxgi;
mod gpu;
mod ncc;
pub mod pixel;
pub mod template;

use serde::Serialize;

/// Template matcher backend in use
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MatcherBackend {
    /// wgpu compute shader (used for large searches)
    Gpu,
    /// Parallel CPU matcher
    Cpu,
}

/// Which vision backends are active, for display in the UI
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionBackends {
    pub capture: capture::CaptureBackend,
    pub capture_adapter: Option<String>,
    pub matcher: MatcherBackend,
    pub gpu_adapter: Option<String>,
}

/// Probe (and lazily initialize) the capture and matching backends
pub fn backends() -> VisionBackends {
    let (capture, capture_adapter) = capture::backend_info();
    let gpu = gpu::global();
    VisionBackends {
        capture,
        capture_adapter,
        matcher: if gpu.is_some() { MatcherBackend::Gpu } else { MatcherBackend::Cpu },
        gpu_adapter: gpu.map(|m| m.adapter_name().to_string()),
    }
}
