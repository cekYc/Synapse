// ============================================================
// Synapse — Flow Compiler (JSON Graph → IR)
// ============================================================
// Transforms the React Flow node/edge graph (received as JSON)
// into a linear sequence of IR instructions. The compiler:
//
// 1. Parses the JSON into internal node/edge structs
// 2. Finds the entry node (trigger or first node with no inputs)
// 3. Topologically walks the graph following edges
// 4. Emits IR instructions with correct jump targets
// ============================================================

use crate::engine::ir::*;
use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum CompileError {
    #[error("JSON parse error: {0}")]
    ParseError(#[from] serde_json::Error),
    #[error("No entry node found (add a trigger or starting node)")]
    NoEntryNode,
    #[error("Unknown node kind: {0}")]
    UnknownNodeKind(String),
    #[error("Node '{0}' has no outgoing edge but is not a terminal node")]
    DanglingNode(String),
}

/// Intermediate JSON structures matching the frontend schema
#[derive(Debug, Deserialize)]
struct FlowJson {
    id: String,
    name: String,
    nodes: Vec<NodeJson>,
    edges: Vec<EdgeJson>,
}

#[derive(Debug, Deserialize)]
struct NodeJson {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    node_type: Option<String>,
    data: NodeDataJson,
}

#[derive(Debug, Deserialize)]
struct NodeDataJson {
    #[serde(rename = "nodeKind")]
    node_kind: String,
    category: String,
    config: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct EdgeJson {
    source: String,
    target: String,
    #[serde(rename = "sourceHandle")]
    source_handle: Option<String>,
}

/// Parse the `inputLevel` field of a node config into an [`InputLevel`].
/// Defaults to `Standard` when absent or unrecognized.
fn parse_input_level(config: &serde_json::Value) -> InputLevel {
    match config.get("inputLevel").and_then(|v| v.as_str()) {
        Some("interception") => InputLevel::Interception,
        Some("virtual_hid") => InputLevel::VirtualHid,
        _ => InputLevel::Standard,
    }
}

/// Compile a flow JSON string into a CompiledFlow IR
pub fn compile(flow_json: &str) -> Result<CompiledFlow, CompileError> {
    let flow: FlowJson = serde_json::from_str(flow_json)?;
    let compiler = FlowCompiler::new(flow);
    compiler.compile()
}

struct FlowCompiler {
    flow: FlowJson,
    /// node_id → list of (target_node_id, source_handle)
    adjacency: HashMap<String, Vec<(String, Option<String>)>>,
    /// node_id → set of source_node_ids (for finding entry points)
    incoming: HashMap<String, Vec<String>>,
}

impl FlowCompiler {
    fn new(flow: FlowJson) -> Self {
        let mut adjacency: HashMap<String, Vec<(String, Option<String>)>> = HashMap::new();
        let mut incoming: HashMap<String, Vec<String>> = HashMap::new();

        // Initialize all nodes
        for node in &flow.nodes {
            adjacency.entry(node.id.clone()).or_default();
            incoming.entry(node.id.clone()).or_default();
        }

        // Build adjacency from edges
        for edge in &flow.edges {
            adjacency
                .entry(edge.source.clone())
                .or_default()
                .push((edge.target.clone(), edge.source_handle.clone()));
            incoming
                .entry(edge.target.clone())
                .or_default()
                .push(edge.source.clone());
        }

        Self {
            flow,
            adjacency,
            incoming,
        }
    }

    fn compile(self) -> Result<CompiledFlow, CompileError> {
        if self.flow.nodes.is_empty() {
            return Ok(CompiledFlow {
                flow_id: self.flow.id.clone(),
                flow_name: self.flow.name.clone(),
                instructions: vec![Instruction {
                    node_id: "__halt__".into(),
                    label: "End".into(),
                    opcode: Opcode::Halt,
                    next: None,
                }],
                entry_point: 0,
            });
        }

        // Find entry nodes (no incoming edges, or trigger category)
        let entry_id = self.find_entry_node()?;

        // Walk the graph and emit instructions
        let mut instructions: Vec<Instruction> = Vec::new();
        let mut node_to_index: HashMap<String, usize> = HashMap::new();
        let node_map: HashMap<String, &NodeJson> =
            self.flow.nodes.iter().map(|n| (n.id.clone(), n)).collect();

        // First pass: emit instructions in topological order
        let mut visit_stack = vec![entry_id.clone()];
        let mut visited = std::collections::HashSet::new();

        while let Some(node_id) = visit_stack.pop() {
            if visited.contains(&node_id) {
                continue;
            }
            visited.insert(node_id.clone());

            let node = match node_map.get(&node_id) {
                Some(n) => n,
                None => continue,
            };

            let idx = instructions.len();
            node_to_index.insert(node_id.clone(), idx);

            let opcode = self.node_to_opcode(node)?;
            instructions.push(Instruction {
                node_id: node_id.clone(),
                label: node
                    .data
                    .config
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string(),
                opcode,
                next: None, // Will be patched in second pass
            });

            // Queue children (in reverse so first child is processed first)
            if let Some(children) = self.adjacency.get(&node_id) {
                for (child_id, _handle) in children.iter().rev() {
                    visit_stack.push(child_id.clone());
                }
            }
        }

        // Add a Halt instruction at the end
        let halt_idx = instructions.len();
        instructions.push(Instruction {
            node_id: "__halt__".into(),
            label: "End".into(),
            opcode: Opcode::Halt,
            next: None,
        });

        // Second pass: patch `next` pointers and branch targets
        for node in &self.flow.nodes {
            if let Some(&idx) = node_to_index.get(&node.id) {
                if let Some(children) = self.adjacency.get(&node.id) {
                    if children.is_empty() {
                        // Terminal node → go to halt
                        instructions[idx].next = Some(halt_idx);
                    } else {
                        // Primary output (handle "output-0" or first edge)
                        let primary = children
                            .iter()
                            .find(|(_, h)| h.as_deref() == Some("output-0") || h.is_none())
                            .or(children.first());

                        if let Some((target_id, _)) = primary {
                            instructions[idx].next = node_to_index.get(target_id).copied();
                        } else {
                            instructions[idx].next = Some(halt_idx);
                        }

                        // For condition/branch nodes, patch else_target
                        let is_branch = matches!(
                            node.data.category.as_str(),
                            "condition" | "loop"
                        );
                        if is_branch {
                            // Secondary output (handle "output-1") → else/exit branch
                            let secondary = children
                                .iter()
                                .find(|(_, h)| h.as_deref() == Some("output-1"));

                            if let Some((else_id, _)) = secondary {
                                let else_idx = node_to_index.get(else_id).copied();
                                match &mut instructions[idx].opcode {
                                    Opcode::Branch { else_target, .. } => {
                                        *else_target = else_idx;
                                    }
                                    Opcode::LoopStart { exit_target, .. } => {
                                        if let Some(ei) = else_idx {
                                            *exit_target = ei;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                } else {
                    instructions[idx].next = Some(halt_idx);
                }
            }
        }

        let entry_point = node_to_index.get(&entry_id).copied().unwrap_or(0);

        Ok(CompiledFlow {
            flow_id: self.flow.id,
            flow_name: self.flow.name,
            instructions,
            entry_point,
        })
    }

    fn find_entry_node(&self) -> Result<String, CompileError> {
        // Prefer trigger nodes
        for node in &self.flow.nodes {
            if node.data.category == "trigger" {
                return Ok(node.id.clone());
            }
        }
        // Fall back to nodes with no incoming edges
        for node in &self.flow.nodes {
            if self
                .incoming
                .get(&node.id)
                .map(|v| v.is_empty())
                .unwrap_or(true)
            {
                return Ok(node.id.clone());
            }
        }
        // Last resort: first node
        self.flow
            .nodes
            .first()
            .map(|n| n.id.clone())
            .ok_or(CompileError::NoEntryNode)
    }

    fn node_to_opcode(&self, node: &NodeJson) -> Result<Opcode, CompileError> {
        let c = &node.data.config;
        let kind = node.data.node_kind.as_str();

        match kind {
            // ─── Triggers (treated as Nop at execution — they just start the flow)
            "hotkey_trigger" | "pixel_color_trigger" | "image_match_trigger"
            | "timer_trigger" => Ok(Opcode::Nop),

            // ─── Actions
            "mouse_click" => Ok(Opcode::MouseClick {
                button: match c.get("button").and_then(|v| v.as_str()).unwrap_or("left") {
                    "right" => MouseButton::Right,
                    "middle" => MouseButton::Middle,
                    _ => MouseButton::Left,
                },
                click_type: match c
                    .get("clickType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("single")
                {
                    "double" => ClickType::Double,
                    "hold" => ClickType::Hold,
                    _ => ClickType::Single,
                },
                x: c.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                y: c.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                relative: c.get("relative").and_then(|v| v.as_bool()).unwrap_or(false),
                input_level: parse_input_level(c),
            }),

            "mouse_move" => Ok(Opcode::MouseMove {
                x: c.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                y: c.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                duration_ms: c.get("duration").and_then(|v| v.as_u64()).unwrap_or(200),
                curve: match c
                    .get("curve")
                    .and_then(|v| v.as_str())
                    .unwrap_or("humanized")
                {
                    "linear" => MoveCurve::Linear,
                    "bezier" => MoveCurve::Bezier,
                    _ => MoveCurve::Humanized,
                },
                relative: c.get("relative").and_then(|v| v.as_bool()).unwrap_or(false),
            }),

            "key_press" => {
                let modifiers: Vec<KeyModifier> = c
                    .get("modifiers")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|m| match m.as_str()? {
                                "ctrl" => Some(KeyModifier::Ctrl),
                                "alt" => Some(KeyModifier::Alt),
                                "shift" => Some(KeyModifier::Shift),
                                "win" => Some(KeyModifier::Win),
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(Opcode::KeyPress {
                    key: c
                        .get("key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Return")
                        .to_string(),
                    modifiers,
                    hold_ms: c.get("holdMs").and_then(|v| v.as_u64()).unwrap_or(50),
                    input_level: parse_input_level(c),
                })
            }

            "type_text" => Ok(Opcode::TypeText {
                text: c
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                delay_per_char_ms: c
                    .get("delayPerChar")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(30),
                humanized: c
                    .get("humanized")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
            }),

            "delay" => Ok(Opcode::Delay {
                duration_ms: c
                    .get("durationMs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1000),
                random_range_ms: c
                    .get("randomRange")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            }),

            "run_program" => Ok(Opcode::RunProgram {
                path: c
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                args: c
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| a.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                wait_for_exit: c
                    .get("waitForExit")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
            }),

            "set_variable" => Ok(Opcode::SetVariable {
                name: c
                    .get("variableName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("var")
                    .to_string(),
                value: c
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                value_type: match c
                    .get("valueType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("string")
                {
                    "number" => VarType::Number,
                    "boolean" => VarType::Boolean,
                    _ => VarType::String,
                },
            }),

            // ─── Conditions
            "if_else" | "pixel_check" | "image_exists" => {
                let condition = Condition {
                    left: CondOperand::Literal(
                        c.get("leftOperand")
                            .and_then(|v| v.as_str())
                            .unwrap_or("true")
                            .to_string(),
                    ),
                    operator: match c
                        .get("operator")
                        .and_then(|v| v.as_str())
                        .unwrap_or("==")
                    {
                        "!=" => CondOperator::Neq,
                        ">" => CondOperator::Gt,
                        "<" => CondOperator::Lt,
                        ">=" => CondOperator::Gte,
                        "<=" => CondOperator::Lte,
                        _ => CondOperator::Eq,
                    },
                    right: CondOperand::Literal(
                        c.get("rightOperand")
                            .and_then(|v| v.as_str())
                            .unwrap_or("true")
                            .to_string(),
                    ),
                };
                Ok(Opcode::Branch {
                    condition,
                    else_target: None, // Patched in second pass
                })
            }

            // ─── Loops
            "loop" => {
                let count = c.get("count").and_then(|v| v.as_u64()).unwrap_or(10) as u32;
                Ok(Opcode::LoopStart {
                    count,
                    exit_target: 0, // Patched in second pass
                })
            }
            "while_loop" => {
                // While loops compile to a Branch + Jump combo
                let condition = Condition {
                    left: CondOperand::Literal(
                        c.get("condition")
                            .and_then(|v| v.as_str())
                            .unwrap_or("true")
                            .to_string(),
                    ),
                    operator: CondOperator::Eq,
                    right: CondOperand::Literal("true".to_string()),
                };
                Ok(Opcode::Branch {
                    condition,
                    else_target: None,
                })
            }

            _ => Err(CompileError::UnknownNodeKind(kind.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trigger wired to a single mouse-click action should compile to
    /// Nop → MouseClick → Halt with correctly linked `next` pointers.
    #[test]
    fn compiles_trigger_action_chain() {
        let json = r#"{
            "id": "f1",
            "name": "Test",
            "nodes": [
                {"id": "t", "type": "trigger", "data": {"nodeKind": "hotkey_trigger", "category": "trigger", "config": {"label": "Trig"}}},
                {"id": "a", "type": "action", "data": {"nodeKind": "mouse_click", "category": "action", "config": {"label": "Click", "x": 100, "y": 200}}}
            ],
            "edges": [
                {"source": "t", "target": "a"}
            ]
        }"#;

        let flow = compile(json).expect("compile should succeed");
        // entry = trigger (Nop), then action, then halt.
        let entry = &flow.instructions[flow.entry_point];
        assert!(matches!(entry.opcode, Opcode::Nop));

        let next_idx = entry.next.expect("trigger should link to action");
        let action = &flow.instructions[next_idx];
        match &action.opcode {
            Opcode::MouseClick { x, y, input_level, .. } => {
                assert_eq!(*x, 100);
                assert_eq!(*y, 200);
                assert_eq!(*input_level, InputLevel::Standard);
            }
            other => panic!("expected MouseClick, got {other:?}"),
        }

        // Action links to a terminal Halt.
        let halt = &flow.instructions[action.next.expect("action links onward")];
        assert!(matches!(halt.opcode, Opcode::Halt));
    }

    /// The `inputLevel` config field must round-trip into the IR so the
    /// executor can reject unavailable backends explicitly.
    #[test]
    fn parses_input_level_selection() {
        let json = r#"{
            "id": "f2",
            "name": "Levels",
            "nodes": [
                {"id": "a", "type": "action", "data": {"nodeKind": "mouse_click", "category": "action", "config": {"label": "Click", "inputLevel": "interception"}}}
            ],
            "edges": []
        }"#;

        let flow = compile(json).expect("compile should succeed");
        let has_interception = flow.instructions.iter().any(|i| {
            matches!(
                &i.opcode,
                Opcode::MouseClick { input_level: InputLevel::Interception, .. }
            )
        });
        assert!(has_interception, "inputLevel=interception must reach the IR");
    }

    /// An empty graph should still yield a valid single-Halt program.
    #[test]
    fn empty_flow_compiles_to_halt() {
        let json = r#"{"id": "e", "name": "Empty", "nodes": [], "edges": []}"#;
        let flow = compile(json).expect("empty flow compiles");
        assert_eq!(flow.instructions.len(), 1);
        assert!(matches!(flow.instructions[0].opcode, Opcode::Halt));
    }

    /// Condition nodes emit a Branch whose else-target follows "output-1".
    #[test]
    fn branch_else_target_wired_from_secondary_handle() {
        let json = r#"{
            "id": "f3",
            "name": "Branch",
            "nodes": [
                {"id": "c", "type": "condition", "data": {"nodeKind": "if_else", "category": "condition", "config": {"label": "If", "leftOperand": "1", "operator": "==", "rightOperand": "1"}}},
                {"id": "yes", "type": "action", "data": {"nodeKind": "delay", "category": "action", "config": {"label": "Yes"}}},
                {"id": "no", "type": "action", "data": {"nodeKind": "delay", "category": "action", "config": {"label": "No"}}}
            ],
            "edges": [
                {"source": "c", "target": "yes", "sourceHandle": "output-0"},
                {"source": "c", "target": "no", "sourceHandle": "output-1"}
            ]
        }"#;

        let flow = compile(json).expect("compile should succeed");
        let branch = flow
            .instructions
            .iter()
            .find(|i| matches!(i.opcode, Opcode::Branch { .. }))
            .expect("a Branch opcode should exist");

        match &branch.opcode {
            Opcode::Branch { else_target, .. } => {
                assert!(else_target.is_some(), "else_target must be wired from output-1");
            }
            _ => unreachable!(),
        }
    }
}
