// ============================================================
// Synapse — System Info IPC Commands
// ============================================================

use serde::Serialize;

#[derive(Serialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub synapse_version: String,
}

#[tauri::command]
pub fn get_system_info() -> SystemInfo {
    SystemInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        synapse_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}
