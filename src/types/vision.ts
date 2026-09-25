// ============================================================
// Synapse — Vision Backend Types
// ============================================================
// Mirrors `VisionBackends` in src-tauri/src/vision/mod.rs.
// ============================================================

export type CaptureBackend = 'dxgi' | 'gdi' | 'unsupported';
export type MatcherBackend = 'gpu' | 'cpu';

export interface VisionBackends {
  capture: CaptureBackend;
  captureAdapter: string | null;
  matcher: MatcherBackend;
  gpuAdapter: string | null;
}
