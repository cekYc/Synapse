// ============================================================
// Synapse — Flow Storage (Filesystem-based)
// ============================================================
// Stores flow JSON files in a dedicated directory within the
// app data folder. Each flow is a separate .synapse.json file.
// ============================================================

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct FlowSummary {
    pub id: String,
    pub name: String,
    pub updated_at: String,
}

/// Get the flows storage directory, creating it if needed.
pub fn flows_dir() -> Result<PathBuf, String> {
    let mut dir = data_dir()?;
    dir.push("flows");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create flows dir: {e}"))?;
    Ok(dir)
}

/// Directory for template images that came with imported flows
pub fn assets_dir() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("assets"))
}

/// Platform-specific data directory for Synapse
fn data_dir() -> Result<PathBuf, String> {
    // Use %APPDATA%/Synapse on Windows
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("USERPROFILE")
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join("AppData").join("Roaming")
        });
    let dir = base.join("Synapse");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create data dir: {e}"))?;
    Ok(dir)
}

/// Save a flow JSON string to disk
pub fn save(flow_json: &str) -> Result<(), String> {
    // Extract the flow ID from the JSON
    let parsed: serde_json::Value =
        serde_json::from_str(flow_json).map_err(|e| format!("Invalid JSON: {e}"))?;

    let id = parsed
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Flow JSON missing 'id' field")?;

    let dir = flows_dir()?;
    let path = dir.join(format!("{id}.synapse.json"));

    fs::write(&path, flow_json).map_err(|e| format!("Failed to write flow: {e}"))?;

    tracing::info!("Flow saved: {}", path.display());
    Ok(())
}

/// Load a flow JSON string from disk
pub fn load(id: &str) -> Result<String, String> {
    let dir = flows_dir()?;
    let path = dir.join(format!("{id}.synapse.json"));

    fs::read_to_string(&path).map_err(|e| format!("Failed to read flow '{id}': {e}"))
}

/// List all saved flows (summaries only)
pub fn list() -> Result<Vec<FlowSummary>, String> {
    let dir = flows_dir()?;
    let mut summaries = Vec::new();

    let entries = fs::read_dir(&dir).map_err(|e| format!("Failed to read flows dir: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                    let id = parsed
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let name = parsed
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Untitled")
                        .to_string();
                    let updated_at = parsed
                        .get("updatedAt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    summaries.push(FlowSummary {
                        id,
                        name,
                        updated_at,
                    });
                }
            }
        }
    }

    Ok(summaries)
}

/// Delete a flow by ID
pub fn delete(id: &str) -> Result<(), String> {
    let dir = flows_dir()?;
    let path = dir.join(format!("{id}.synapse.json"));

    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Failed to delete flow '{id}': {e}"))?;
        tracing::info!("Flow deleted: {}", path.display());
        Ok(())
    } else {
        Err(format!("Flow '{id}' not found"))
    }
}
