// ============================================================
// Synapse — Intermediate Representation (IR)
// ============================================================
// The IR is the bridge between the visual flow graph (JSON) and
// the executor. The compiler transforms the DAG of nodes/edges
// into a linear sequence of IR instructions with explicit jump
// targets for branches and loops.
//
// Design principles:
// - Each instruction is self-contained (no back-references)
// - Branch/loop targets use instruction indices
// - The IR is cheap to clone and send across threads
// ============================================================

use serde::{Deserialize, Serialize};

/// A compiled flow ready for execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledFlow {
    pub flow_id: String,
    pub flow_name: String,
    pub instructions: Vec<Instruction>,
    /// Index of the first instruction to execute
    pub entry_point: usize,
}

/// A single IR instruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instruction {
    /// Unique ID matching the original React Flow node
    pub node_id: String,
    /// Human-readable label for status reporting
    pub label: String,
    /// The operation to perform
    pub opcode: Opcode,
    /// Index of the next instruction (None = end of flow)
    pub next: Option<usize>,
}

/// All executable operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Opcode {
    // ─── Actions ─────────────────────────────────────
    MouseClick {
        button: MouseButton,
        click_type: ClickType,
        x: i32,
        y: i32,
        relative: bool,
        #[serde(default)]
        input_level: InputLevel,
    },
    MouseMove {
        x: i32,
        y: i32,
        duration_ms: u64,
        curve: MoveCurve,
        relative: bool,
    },
    KeyPress {
        key: String,
        modifiers: Vec<KeyModifier>,
        hold_ms: u64,
        #[serde(default)]
        input_level: InputLevel,
    },
    TypeText {
        text: String,
        delay_per_char_ms: u64,
        humanized: bool,
    },
    Delay {
        duration_ms: u64,
        random_range_ms: u64,
    },
    RunProgram {
        path: String,
        args: Vec<String>,
        wait_for_exit: bool,
    },
    SetVariable {
        name: String,
        value: String,
        value_type: VarType,
    },

    // ─── Vision ──────────────────────────────────────
    /// Branch on a screen check: if it holds → next, otherwise → else_target
    VisionBranch {
        check: VisionCheck,
        else_target: Option<usize>,
    },
    /// Block until a screen check holds (pixel/image triggers), polling
    /// every `poll_interval_ms`; honors pause and stop while waiting
    WaitForVision {
        check: VisionCheck,
        poll_interval_ms: u64,
    },

    // ─── Flow Control ────────────────────────────────
    /// Conditional branch: if true → next, if false → else_target
    Branch {
        condition: Condition,
        else_target: Option<usize>,
    },
    /// Jump unconditionally to target
    Jump {
        target: usize,
    },
    /// Loop header: decrement counter, if > 0 continue, else jump to exit
    LoopStart {
        count: u32, // 0 = infinite
        exit_target: usize,
    },
    /// Jump back to loop header
    LoopEnd {
        loop_start: usize,
    },

    // ─── Meta ────────────────────────────────────────
    /// No operation (used as placeholder)
    Nop,
    /// End of flow
    Halt,
}

/// A condition evaluated against the screen
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum VisionCheck {
    /// The pixel at (x, y) is within `tolerance` of `color`
    Pixel {
        x: i32,
        y: i32,
        color: String,
        tolerance: u32,
    },
    /// Some pixel inside `region` is within `tolerance` of `color`
    ColorInRegion {
        region: Region,
        color: String,
        tolerance: u32,
    },
    /// The template image is found with at least `confidence`
    Image {
        template_path: String,
        confidence: f64,
        region: Option<Region>,
    },
}

/// A screen rectangle in virtual-desktop coordinates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Region {
    pub fn as_tuple(&self) -> (i32, i32, u32, u32) {
        (self.x, self.y, self.w, self.h)
    }
}

/// Which input backend an action requests.
///
/// The visual editor lets users pick an input "level" per action. Only the
/// standard backend (L1: enigo/SendInput) is currently implemented. The higher
/// levels are declared here so flows can carry the selection round-trip, and so
/// the executor can report a clear, explicit error instead of silently running
/// on L1 when an unavailable backend is requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum InputLevel {
    /// L1 — standard OS-level input injection (enigo / SendInput).
    #[default]
    Standard,
    /// L2 — Interception kernel driver. Not implemented.
    Interception,
    /// L3 — virtual HID device. Not implemented.
    VirtualHid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClickType {
    Single,
    Double,
    Hold,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MoveCurve {
    Linear,
    Bezier,
    Humanized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyModifier {
    Ctrl,
    Alt,
    Shift,
    Win,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VarType {
    String,
    Number,
    Boolean,
}

/// Condition used in Branch instructions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub left: CondOperand,
    pub operator: CondOperator,
    pub right: CondOperand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CondOperand {
    Literal(String),
    Variable(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CondOperator {
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
}
