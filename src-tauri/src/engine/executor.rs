// ============================================================
// Synapse — Flow Executor
// ============================================================
// Executes a CompiledFlow by stepping through IR instructions.
// The executor runs on a dedicated thread (not tokio) for
// precise timing of input operations.
//
// Architecture:
// - Main executor loop runs on a std::thread with elevated
//   priority to minimize OS scheduling jitter
// - Tauri events are emitted for real-time UI status updates
// - Pause/cancel is controlled via atomic flags in the context
// ============================================================

use crate::engine::context::ExecutionContext;
use crate::engine::ir::*;
use crate::engine::vision_check::{self, PreparedCheck, VisionHit};
use crate::input::standard::StandardInput;
use crate::input::InputBackend;
use rand::Rng;
use serde::Serialize;

use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Status events emitted to the frontend
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum ExecutionEvent {
    /// Flow execution started
    Started {
        flow_id: String,
    },
    /// Active node changed
    NodeActivated {
        node_id: String,
        label: String,
    },
    /// Node execution completed
    NodeCompleted {
        node_id: String,
    },
    /// A log message from execution
    Log {
        level: String,
        message: String,
    },
    /// Variable changed
    VariableChanged {
        name: String,
        value: String,
    },
    /// Execution paused
    Paused,
    /// Execution resumed
    Resumed,
    /// Execution completed successfully
    Completed {
        flow_id: String,
    },
    /// Execution stopped by user
    Stopped {
        flow_id: String,
    },
    /// Execution encountered an error
    Error {
        flow_id: String,
        message: String,
    },
}

const EVENT_NAME: &str = "synapse://execution";

/// A screen trigger gives up after this many consecutive capture failures
/// (at the default 100 ms poll: about two seconds of failing captures)
const MAX_POLL_FAILURES: u32 = 20;

/// Start executing a compiled flow on a dedicated thread
pub fn execute_flow(
    app_handle: AppHandle,
    compiled: CompiledFlow,
    context: ExecutionContext,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("synapse-executor".into())
        .spawn(move || {
            // Try to set thread priority to above-normal for precise timing
            #[cfg(target_os = "windows")]
            {
                unsafe {
                    let handle = windows_sys::Win32::System::Threading::GetCurrentThread();
                    windows_sys::Win32::System::Threading::SetThreadPriority(
                        handle,
                        windows_sys::Win32::System::Threading::THREAD_PRIORITY_ABOVE_NORMAL as i32,
                    );
                }
            }

            let result = run_executor(&app_handle, &compiled, &context);

            match result {
                Ok(()) => {
                    if context.is_cancelled() {
                        emit_event(&app_handle, ExecutionEvent::Stopped {
                            flow_id: compiled.flow_id.clone(),
                        });
                    } else {
                        emit_event(&app_handle, ExecutionEvent::Completed {
                            flow_id: compiled.flow_id.clone(),
                        });
                    }
                }
                Err(e) => {
                    tracing::error!("Execution error: {e}");
                    emit_event(&app_handle, ExecutionEvent::Error {
                        flow_id: compiled.flow_id.clone(),
                        message: e,
                    });
                }
            }
        })
        .expect("Failed to spawn executor thread")
}

fn run_executor(
    app: &AppHandle,
    flow: &CompiledFlow,
    ctx: &ExecutionContext,
) -> Result<(), String> {
    emit_event(app, ExecutionEvent::Started {
        flow_id: flow.flow_id.clone(),
    });
    emit_event(app, ExecutionEvent::Log {
        level: "info".into(),
        message: format!("Flow '{}' execution started", flow.flow_name),
    });

    // Initialize the input backend
    let input = StandardInput::new()?;

    let mut pc = flow.entry_point; // Program counter

    loop {
        // Check cancellation
        if ctx.is_cancelled() {
            tracing::info!("Execution cancelled");
            return Ok(());
        }

        // Check pause (spin-wait with sleep)
        while ctx.is_paused() {
            if ctx.is_cancelled() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }

        // Bounds check
        if pc >= flow.instructions.len() {
            break;
        }

        let instr = &flow.instructions[pc];

        // Emit node activation event
        if instr.node_id != "__halt__" {
            emit_event(app, ExecutionEvent::NodeActivated {
                node_id: instr.node_id.clone(),
                label: instr.label.clone(),
            });
        }

        // Execute the instruction. On failure, defensively release modifier
        // keys before propagating the error so nothing is left stuck down.
        let next_pc = match execute_instruction(instr, &input, ctx, app, pc) {
            Ok(n) => n,
            Err(e) => {
                input.release_modifiers();
                return Err(e);
            }
        };

        // Emit node completion
        if instr.node_id != "__halt__" {
            emit_event(app, ExecutionEvent::NodeCompleted {
                node_id: instr.node_id.clone(),
            });
        }

        // Advance program counter
        match next_pc {
            NextPc::Continue => {
                pc = instr.next.unwrap_or(flow.instructions.len());
            }
            NextPc::Jump(target) => {
                pc = target;
            }
            NextPc::Halt => {
                break;
            }
        }
    }

    emit_event(app, ExecutionEvent::Log {
        level: "info".into(),
        message: format!("Flow '{}' execution completed", flow.flow_name),
    });

    Ok(())
}

enum NextPc {
    Continue,
    Jump(usize),
    Halt,
}

fn execute_instruction(
    instr: &Instruction,
    input: &StandardInput,
    ctx: &ExecutionContext,
    app: &AppHandle,
    pc: usize,
) -> Result<NextPc, String> {
    match &instr.opcode {
        Opcode::Nop => {
            tracing::debug!("[{}] Nop (trigger placeholder)", instr.label);
            Ok(NextPc::Continue)
        }

        Opcode::Halt => {
            tracing::debug!("Halt reached");
            Ok(NextPc::Halt)
        }

        // ─── Actions ─────────────────────────────────

        Opcode::MouseClick {
            button,
            click_type,
            x,
            y,
            relative,
            input_level,
        } => {
            ensure_level_supported(input_level)?;
            emit_event(app, ExecutionEvent::Log {
                level: "info".into(),
                message: format!("🖱️ Click ({x}, {y})"),
            });
            input.mouse_click(button, click_type, *x, *y, *relative)?;
            Ok(NextPc::Continue)
        }

        Opcode::MouseMove {
            x,
            y,
            duration_ms,
            curve,
            relative,
        } => {
            emit_event(app, ExecutionEvent::Log {
                level: "info".into(),
                message: format!("🖱️ Move to ({x}, {y}) over {duration_ms}ms"),
            });
            input.mouse_move(*x, *y, *duration_ms, curve, *relative)?;
            Ok(NextPc::Continue)
        }

        Opcode::KeyPress {
            key,
            modifiers,
            hold_ms,
            input_level,
        } => {
            ensure_level_supported(input_level)?;
            emit_event(app, ExecutionEvent::Log {
                level: "info".into(),
                message: format!("⌨️ Press '{key}'"),
            });
            input.key_press(key, modifiers, *hold_ms)?;
            Ok(NextPc::Continue)
        }

        Opcode::TypeText {
            text,
            delay_per_char_ms,
            humanized,
        } => {
            emit_event(app, ExecutionEvent::Log {
                level: "info".into(),
                message: format!("⌨️ Type '{}'", truncate_str(text, 30)),
            });
            input.type_text(text, *delay_per_char_ms, *humanized)?;
            Ok(NextPc::Continue)
        }

        Opcode::Delay {
            duration_ms,
            random_range_ms,
        } => {
            let actual_ms = if *random_range_ms > 0 {
                let mut rng = rand::rng();
                let jitter = rng.random_range(-(*random_range_ms as i64)..=(*random_range_ms as i64));
                (*duration_ms as i64 + jitter).max(0) as u64
            } else {
                *duration_ms
            };

            emit_event(app, ExecutionEvent::Log {
                level: "info".into(),
                message: format!("⏱️ Wait {actual_ms}ms"),
            });

            if sleep_cancellable(ctx, actual_ms) {
                Ok(NextPc::Continue)
            } else {
                Ok(NextPc::Halt)
            }
        }

        Opcode::RunProgram {
            path,
            args,
            wait_for_exit,
        } => {
            emit_event(app, ExecutionEvent::Log {
                level: "info".into(),
                message: format!("🚀 Run: {path}"),
            });

            let mut cmd = std::process::Command::new(path);
            cmd.args(args);

            if *wait_for_exit {
                let output = cmd.output().map_err(|e| format!("Failed to run program: {e}"))?;
                if !output.status.success() {
                    emit_event(app, ExecutionEvent::Log {
                        level: "warn".into(),
                        message: format!("Program exited with code: {:?}", output.status.code()),
                    });
                }
            } else {
                cmd.spawn().map_err(|e| format!("Failed to spawn program: {e}"))?;
            }

            Ok(NextPc::Continue)
        }

        Opcode::SetVariable {
            name,
            value,
            value_type: _,
        } => {
            ctx.set_variable(name, value);
            emit_event(app, ExecutionEvent::VariableChanged {
                name: name.clone(),
                value: value.clone(),
            });
            emit_event(app, ExecutionEvent::Log {
                level: "debug".into(),
                message: format!("📝 {name} = '{value}'"),
            });
            Ok(NextPc::Continue)
        }

        // ─── Vision ──────────────────────────────────

        Opcode::VisionBranch { check, else_target } => {
            let hit = PreparedCheck::new(check)?.evaluate()?;
            emit_event(app, ExecutionEvent::Log {
                level: "info".into(),
                message: format!(
                    "👁️ {}: {}",
                    vision_check::describe(check),
                    if hit.is_some() { "found" } else { "not found" }
                ),
            });

            match (hit, else_target) {
                (Some(hit), _) => {
                    record_hit(app, ctx, &hit);
                    Ok(NextPc::Continue)
                }
                (None, Some(target)) => Ok(NextPc::Jump(*target)),
                (None, None) => Ok(NextPc::Continue),
            }
        }

        Opcode::WaitForVision {
            check,
            poll_interval_ms,
        } => {
            // Configuration errors (bad color, missing template) fail at once
            let prepared = PreparedCheck::new(check)?;
            emit_event(app, ExecutionEvent::Log {
                level: "info".into(),
                message: format!("👁️ Waiting for {}", vision_check::describe(check)),
            });

            let mut failures = 0u32;
            loop {
                match prepared.evaluate() {
                    Ok(Some(hit)) => {
                        emit_event(app, ExecutionEvent::Log {
                            level: "info".into(),
                            message: format!("👁️ Triggered at ({}, {})", hit.x, hit.y),
                        });
                        record_hit(app, ctx, &hit);
                        return Ok(NextPc::Continue);
                    }
                    Ok(None) => failures = 0,
                    // Capture can fail transiently (e.g. while a UAC prompt
                    // owns the screen); only give up if it keeps failing
                    Err(e) => {
                        failures += 1;
                        if failures == 1 {
                            emit_event(app, ExecutionEvent::Log {
                                level: "warn".into(),
                                message: format!("Screen capture failed, retrying: {e}"),
                            });
                        }
                        if failures >= MAX_POLL_FAILURES {
                            return Err(e);
                        }
                    }
                }

                if !sleep_cancellable(ctx, *poll_interval_ms) {
                    return Ok(NextPc::Halt);
                }
            }
        }

        // ─── Flow Control ────────────────────────────

        Opcode::Branch {
            condition,
            else_target,
        } => {
            let result = ctx.evaluate_condition(condition);
            emit_event(app, ExecutionEvent::Log {
                level: "debug".into(),
                message: format!("🔀 Branch: condition = {result}"),
            });

            if result {
                Ok(NextPc::Continue) // Follow primary output (next)
            } else if let Some(target) = else_target {
                Ok(NextPc::Jump(*target))
            } else {
                Ok(NextPc::Continue)
            }
        }

        Opcode::Jump { target } => {
            Ok(NextPc::Jump(*target))
        }

        Opcode::LoopStart {
            count,
            exit_target,
        } => {
            if *count == 0 {
                // Infinite loop — always continue
                Ok(NextPc::Continue)
            } else {
                // Initialize counter on first visit
                if !ctx.has_loop_counter(pc) {
                    ctx.init_loop_counter(pc, *count);
                }

                let remaining = ctx.decrement_loop_counter(pc).unwrap_or(0);
                emit_event(app, ExecutionEvent::Log {
                    level: "debug".into(),
                    message: format!("🔁 Loop: {remaining} iterations remaining"),
                });

                if remaining > 0 {
                    Ok(NextPc::Continue) // Enter loop body
                } else {
                    Ok(NextPc::Jump(*exit_target)) // Exit loop
                }
            }
        }

        Opcode::LoopEnd { loop_start } => {
            Ok(NextPc::Jump(*loop_start)) // Jump back to loop header
        }
    }
}

fn emit_event(app: &AppHandle, event: ExecutionEvent) {
    if let Err(e) = app.emit(EVENT_NAME, &event) {
        tracing::warn!("Failed to emit event: {e}");
    }
}

/// Sleep for `ms`, waiting out pauses. Returns `false` if the flow was
/// stopped, in which case the caller should halt.
fn sleep_cancellable(ctx: &ExecutionContext, ms: u64) -> bool {
    const CHUNK_MS: u64 = 50;
    let mut remaining = ms;
    loop {
        if ctx.is_cancelled() {
            return false;
        }
        while ctx.is_paused() {
            if ctx.is_cancelled() {
                return false;
            }
            thread::sleep(Duration::from_millis(CHUNK_MS));
        }
        if remaining == 0 {
            return true;
        }
        let step = remaining.min(CHUNK_MS);
        thread::sleep(Duration::from_millis(step));
        remaining -= step;
    }
}

/// Expose where a vision check matched as flow variables
/// (`match_x`, `match_y` and, for images, `match_confidence`)
fn record_hit(app: &AppHandle, ctx: &ExecutionContext, hit: &VisionHit) {
    let mut vars = vec![("match_x", hit.x.to_string()), ("match_y", hit.y.to_string())];
    if let Some(confidence) = hit.confidence {
        vars.push(("match_confidence", format!("{confidence:.3}")));
    }
    for (name, value) in vars {
        ctx.set_variable(name, &value);
        emit_event(app, ExecutionEvent::VariableChanged {
            name: name.into(),
            value,
        });
    }
}

/// Verify that the requested input backend level is available.
///
/// Only L1 (standard) is implemented. When a flow requests L2/L3, we fail with
/// an explicit message rather than silently downgrading to L1 — the user chose
/// a specific backend and deserves to know it is not running.
fn ensure_level_supported(level: &InputLevel) -> Result<(), String> {
    match level {
        InputLevel::Standard => Ok(()),
        InputLevel::Interception => Err(
            "Interception (L2) giriş sürücüsü bu sürümde mevcut değil. \
             Lütfen ilgili düğümde giriş seviyesini 'standard' olarak seçin."
                .into(),
        ),
        InputLevel::VirtualHid => Err(
            "Virtual HID (L3) giriş arka ucu bu sürümde mevcut değil. \
             Lütfen ilgili düğümde giriş seviyesini 'standard' olarak seçin."
                .into(),
        ),
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_level_is_supported() {
        assert!(ensure_level_supported(&InputLevel::Standard).is_ok());
    }

    #[test]
    fn unavailable_levels_return_explicit_error() {
        // L2/L3 must fail loudly rather than silently downgrading to L1.
        assert!(ensure_level_supported(&InputLevel::Interception).is_err());
        assert!(ensure_level_supported(&InputLevel::VirtualHid).is_err());
    }

    #[test]
    fn sleep_returns_false_once_stopped() {
        let ctx = ExecutionContext::new();
        ctx.cancel();
        let start = std::time::Instant::now();
        assert!(!sleep_cancellable(&ctx, 10_000));
        assert!(start.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn sleep_waits_out_pauses_and_stops_early() {
        let ctx = ExecutionContext::new();
        let start = std::time::Instant::now();
        assert!(sleep_cancellable(&ctx, 60));
        assert!(start.elapsed() >= Duration::from_millis(60));

        // Paused: the sleep outlasts its duration until resumed
        ctx.pause();
        let remote = ctx.clone();
        let resumer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            remote.resume();
        });
        let start = std::time::Instant::now();
        assert!(sleep_cancellable(&ctx, 10));
        assert!(start.elapsed() >= Duration::from_millis(200));
        resumer.join().unwrap();

        // Stopped mid-sleep: returns promptly with false
        let remote = ctx.clone();
        let stopper = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            remote.cancel();
        });
        let start = std::time::Instant::now();
        assert!(!sleep_cancellable(&ctx, 10_000));
        assert!(start.elapsed() < Duration::from_millis(1_000));
        stopper.join().unwrap();
    }
}
