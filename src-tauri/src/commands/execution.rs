// ============================================================
// Synapse — Execution IPC Commands
// ============================================================

use crate::engine::{compiler, context::ExecutionContext, executor};
use parking_lot::Mutex;
use std::sync::Arc;
use std::thread::JoinHandle;
use tauri::{AppHandle, State};

/// Shared execution state managed by Tauri
pub struct ExecutionManager {
    context: Mutex<Option<ExecutionContext>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl ExecutionManager {
    pub fn new() -> Self {
        Self {
            context: Mutex::new(None),
            handle: Mutex::new(None),
        }
    }
}

#[tauri::command]
pub fn execute_flow(
    app_handle: AppHandle,
    flow_json: String,
    state: State<'_, Arc<ExecutionManager>>,
) -> Result<(), String> {
    // Check if already running
    {
        let handle = state.handle.lock();
        if let Some(h) = handle.as_ref() {
            if !h.is_finished() {
                return Err("A flow is already running. Stop it first.".into());
            }
        }
    }

    // Compile the flow
    let compiled = compiler::compile(&flow_json)
        .map_err(|e| format!("Compilation error: {e}"))?;

    tracing::info!(
        "Compiled flow '{}': {} instructions",
        compiled.flow_name,
        compiled.instructions.len()
    );

    // Create a new execution context
    let ctx = ExecutionContext::new();

    // Store context for pause/resume/stop
    *state.context.lock() = Some(ctx.clone());

    // Start execution on dedicated thread
    let join_handle = executor::execute_flow(app_handle, compiled, ctx);
    *state.handle.lock() = Some(join_handle);

    Ok(())
}

#[tauri::command]
pub fn pause_execution(state: State<'_, Arc<ExecutionManager>>) -> Result<(), String> {
    let ctx = state.context.lock();
    if let Some(ctx) = ctx.as_ref() {
        ctx.pause();
        tracing::info!("Execution paused");
        Ok(())
    } else {
        Err("No flow is running".into())
    }
}

#[tauri::command]
pub fn resume_execution(state: State<'_, Arc<ExecutionManager>>) -> Result<(), String> {
    let ctx = state.context.lock();
    if let Some(ctx) = ctx.as_ref() {
        ctx.resume();
        tracing::info!("Execution resumed");
        Ok(())
    } else {
        Err("No flow is running".into())
    }
}

#[tauri::command]
pub fn stop_execution(state: State<'_, Arc<ExecutionManager>>) -> Result<(), String> {
    let ctx = state.context.lock();
    if let Some(ctx) = ctx.as_ref() {
        ctx.cancel();
        tracing::info!("Execution stop requested");
        Ok(())
    } else {
        Err("No flow is running".into())
    }
}
