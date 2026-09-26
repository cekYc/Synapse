// ============================================================
// Synapse — Flow Sharing IPC Commands
// ============================================================
// Export the editor's flow as a `.synapse` package or a share
// code, and preview/import packages from others. Importing only
// validates, stores the embedded images and returns the flow for
// the editor — it never runs anything.
// ============================================================

use crate::sharing::{self, review, Package, ShareMeta};
use crate::storage::flows;
use serde::Deserialize;
use std::path::PathBuf;

/// Where a package to import comes from
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PackageSource {
    File { path: String },
    Code { code: String },
}

impl PackageSource {
    fn read(&self) -> Result<Package, String> {
        match self {
            PackageSource::File { path } => sharing::read_file(&PathBuf::from(path)),
            PackageSource::Code { code } => sharing::read_share_code(code),
        }
    }
}

async fn blocking<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("Sharing task failed: {e}"))?
}

/// Write the flow as a `.synapse` package file
#[tauri::command]
pub async fn export_flow_package(flow_json: String, meta: ShareMeta, path: String) -> Result<(), String> {
    blocking(move || {
        let package = sharing::build_package(&flow_json, meta)?;
        let mut path = PathBuf::from(path);
        if path.extension().is_none() {
            path.set_extension(sharing::FILE_EXTENSION);
        }
        std::fs::write(&path, sharing::to_file_bytes(&package)?)
            .map_err(|e| format!("Dosya kaydedilemedi: {e}"))?;
        tracing::info!("Flow exported: {}", path.display());
        Ok(())
    })
    .await
}

/// Encode the flow as a copy-and-paste share code
#[tauri::command]
pub async fn create_share_code(flow_json: String, meta: ShareMeta) -> Result<String, String> {
    blocking(move || sharing::to_share_code(&sharing::build_package(&flow_json, meta)?)).await
}

/// Validate a package and describe what it contains, without importing it
#[tauri::command]
pub async fn preview_flow_package(source: PackageSource) -> Result<review::ImportPreview, String> {
    blocking(move || Ok(review::preview(&source.read()?))).await
}

/// Import a package: store its images and return the flow JSON to load
#[tauri::command]
pub async fn import_flow_package(source: PackageSource) -> Result<String, String> {
    blocking(move || {
        let package = source.read()?;
        let flow = sharing::install(package, &flows::assets_dir()?)?;
        tracing::info!("Flow package imported");
        Ok(flow)
    })
    .await
}
