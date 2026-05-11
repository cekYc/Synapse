// ============================================================
// Synapse — Runtime Context
// ============================================================
// Holds the mutable state during flow execution: variables,
// loop counters, and execution control flags.
// ============================================================

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared execution context passed to the executor
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    inner: Arc<ContextInner>,
}

#[derive(Debug)]
struct ContextInner {
    /// User-defined variables (read/write from flow)
    variables: RwLock<HashMap<String, String>>,
    /// Loop iteration counters keyed by instruction index
    loop_counters: RwLock<HashMap<usize, u32>>,
    /// Pause flag — executor checks this between instructions
    paused: AtomicBool,
    /// Cancel flag — executor stops immediately
    cancelled: AtomicBool,
}

impl ExecutionContext {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ContextInner {
                variables: RwLock::new(HashMap::new()),
                loop_counters: RwLock::new(HashMap::new()),
                paused: AtomicBool::new(false),
                cancelled: AtomicBool::new(false),
            }),
        }
    }

    // ─── Variable Operations ────────────────────────

    pub fn set_variable(&self, name: &str, value: &str) {
        self.inner
            .variables
            .write()
            .insert(name.to_string(), value.to_string());
    }

    pub fn get_variable(&self, name: &str) -> Option<String> {
        self.inner.variables.read().get(name).cloned()
    }

    #[allow(dead_code)]
    pub fn get_all_variables(&self) -> HashMap<String, String> {
        self.inner.variables.read().clone()
    }

    // ─── Loop Counter Operations ────────────────────

    /// Initialize or reset a loop counter for instruction at `idx`
    pub fn init_loop_counter(&self, idx: usize, count: u32) {
        self.inner
            .loop_counters
            .write()
            .insert(idx, count);
    }

    /// Decrement loop counter. Returns remaining count.
    /// Returns None if counter doesn't exist.
    pub fn decrement_loop_counter(&self, idx: usize) -> Option<u32> {
        let mut counters = self.inner.loop_counters.write();
        if let Some(counter) = counters.get_mut(&idx) {
            if *counter > 0 {
                *counter -= 1;
            }
            Some(*counter)
        } else {
            None
        }
    }

    /// Check if loop counter has been initialized for this instruction
    pub fn has_loop_counter(&self, idx: usize) -> bool {
        self.inner.loop_counters.read().contains_key(&idx)
    }

    // ─── Control Flow ───────────────────────────────

    pub fn pause(&self) {
        self.inner.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.inner.paused.store(false, Ordering::SeqCst);
    }

    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Resolve a condition operand to its string value
    pub fn resolve_operand(&self, operand: &crate::engine::ir::CondOperand) -> String {
        match operand {
            crate::engine::ir::CondOperand::Literal(s) => s.clone(),
            crate::engine::ir::CondOperand::Variable(name) => {
                self.get_variable(name).unwrap_or_default()
            }
        }
    }

    /// Evaluate a condition
    pub fn evaluate_condition(&self, condition: &crate::engine::ir::Condition) -> bool {
        let left = self.resolve_operand(&condition.left);
        let right = self.resolve_operand(&condition.right);

        // Try numeric comparison first
        let numeric = left.parse::<f64>().ok().zip(right.parse::<f64>().ok());

        match &condition.operator {
            crate::engine::ir::CondOperator::Eq => left == right,
            crate::engine::ir::CondOperator::Neq => left != right,
            crate::engine::ir::CondOperator::Gt => {
                numeric.map(|(l, r)| l > r).unwrap_or(left > right)
            }
            crate::engine::ir::CondOperator::Lt => {
                numeric.map(|(l, r)| l < r).unwrap_or(left < right)
            }
            crate::engine::ir::CondOperator::Gte => {
                numeric.map(|(l, r)| l >= r).unwrap_or(left >= right)
            }
            crate::engine::ir::CondOperator::Lte => {
                numeric.map(|(l, r)| l <= r).unwrap_or(left <= right)
            }
        }
    }
}
