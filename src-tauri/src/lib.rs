// ============================================================
// Synapse — Rust Library Root
// ============================================================

mod commands;
mod engine;
mod input;
mod storage;
mod vision;

use commands::execution::ExecutionManager;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Synapse Engine v{} starting...", env!("CARGO_PKG_VERSION"));

    // Shared execution state
    let execution_manager = Arc::new(ExecutionManager::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(execution_manager)
        .invoke_handler(tauri::generate_handler![
            // Flow CRUD
            commands::flow::save_flow,
            commands::flow::load_flow,
            commands::flow::list_flows,
            commands::flow::delete_flow,
            // System info
            commands::system::get_system_info,
            // Execution control
            commands::execution::execute_flow,
            commands::execution::pause_execution,
            commands::execution::resume_execution,
            commands::execution::stop_execution,
            // Vision
            commands::vision::check_pixel,
            commands::vision::get_pixel,
            commands::vision::find_pixel,
            commands::vision::find_image,
            commands::vision::get_vision_backends,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Synapse");
}
