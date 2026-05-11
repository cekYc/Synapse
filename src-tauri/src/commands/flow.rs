// ============================================================
// Synapse — Flow IPC Commands
// ============================================================

use crate::storage::flows;

#[tauri::command]
pub fn save_flow(flow: String) -> Result<(), String> {
    flows::save(&flow)
}

#[tauri::command]
pub fn load_flow(id: String) -> Result<String, String> {
    flows::load(&id)
}

#[tauri::command]
pub fn list_flows() -> Result<Vec<flows::FlowSummary>, String> {
    flows::list()
}

#[tauri::command]
pub fn delete_flow(id: String) -> Result<(), String> {
    flows::delete(&id)
}
