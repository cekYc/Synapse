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

        // Execute the instruction
        let next_pc = execute_instruction(&instr, &input, ctx, app, pc)?;

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
        } => {
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
        } => {
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

            // Sleep in small chunks so we can check for cancellation
            let chunk = 50u64;
            let mut remaining = actual_ms;
            while remaining > 0 {
                if ctx.is_cancelled() {
                    return Ok(NextPc::Halt);
                }
                while ctx.is_paused() {
                    if ctx.is_cancelled() {
                        return Ok(NextPc::Halt);
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                let sleep_ms = remaining.min(chunk);
                thread::sleep(Duration::from_millis(sleep_ms));
                remaining = remaining.saturating_sub(sleep_ms);
            }

            Ok(NextPc::Continue)
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

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max])
    } else {
        s.to_string()
    }
}
