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
use crate::vision::pixel::parse_hex_color;
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
    #[error("'{node}': {message}")]
    InvalidConfig { node: String, message: String },
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

fn invalid(node: &NodeJson, message: impl Into<String>) -> CompileError {
    let label = node
        .data
        .config
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or(&node.id);
    CompileError::InvalidConfig {
        node: label.to_string(),
        message: message.into(),
    }
}

fn int_field(config: &serde_json::Value, key: &str) -> i32 {
    config.get(key).and_then(|v| v.as_i64()).unwrap_or(0) as i32
}

/// A `#RRGGBB` color, validated at compile time
fn color_field(node: &NodeJson, key: &str) -> Result<String, CompileError> {
    let raw = node.data.config.get(key).and_then(|v| v.as_str()).unwrap_or("");
    parse_hex_color(raw).map_err(|_| invalid(node, format!("Geçersiz renk '{raw}' ({key}), #RRGGBB biçiminde olmalı")))?;
    Ok(raw.trim().to_string())
}

/// Manhattan RGB distance tolerance (0–765)
fn tolerance_field(config: &serde_json::Value) -> u32 {
    config.get("tolerance").and_then(|v| v.as_u64()).unwrap_or(10).min(765) as u32
}

fn poll_interval_field(config: &serde_json::Value, default_ms: u64) -> u64 {
    config
        .get("pollIntervalMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(default_ms)
        .max(10)
}

/// Search region from the editor's flat `regionX/Y/W/H` fields (or a nested
/// `region: {x, y, w, h}` object). A zero width or height means "whole screen".
fn region_field(config: &serde_json::Value) -> Option<Region> {
    let nested = config.get("region");
    let field = |flat: &str, key: &str| {
        config
            .get(flat)
            .or_else(|| nested.and_then(|r| r.get(key)))
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    };
    let (w, h) = (field("regionW", "w"), field("regionH", "h"));
    (w > 0 && h > 0).then(|| Region {
        x: field("regionX", "x") as i32,
        y: field("regionY", "y") as i32,
        w: w as u32,
        h: h as u32,
    })
}

fn image_check(node: &NodeJson) -> Result<VisionCheck, CompileError> {
    let c = &node.data.config;
    let template_path = c
        .get("templatePath")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if template_path.is_empty() {
        return Err(invalid(node, "Şablon görseli seçilmedi (templatePath)"));
    }
    let confidence = c.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.9);
    if !(confidence > 0.0 && confidence <= 1.0) {
        return Err(invalid(node, format!("Güven değeri 0 ile 1 arasında olmalı (şu an {confidence})")));
    }
    Ok(VisionCheck::Image {
        template_path,
        confidence,
        region: region_field(c),
    })
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
                        let is_branch = matches!(
                            node.data.category.as_str(),
                            "condition" | "loop"
                        );

                        // Primary output (handle "output-0" or an edge without
                        // a handle). Single-output nodes may use any edge, but a
                        // branch whose true/body output is unconnected ends the
                        // flow there instead of borrowing its else edge.
                        let primary = children
                            .iter()
                            .find(|(_, h)| h.as_deref() == Some("output-0") || h.is_none())
                            .or(if is_branch { None } else { children.first() });

                        instructions[idx].next = match primary {
                            Some((target_id, _)) => node_to_index.get(target_id).copied(),
                            None => Some(halt_idx),
                        };

                        if is_branch {
                            // Secondary output (handle "output-1") → else/exit
                            // branch. Without one the flow ends when the
                            // condition fails, rather than falling through to
                            // the true branch.
                            let else_idx = children
                                .iter()
                                .find(|(_, h)| h.as_deref() == Some("output-1"))
                                .and_then(|(else_id, _)| node_to_index.get(else_id).copied())
                                .unwrap_or(halt_idx);

                            match &mut instructions[idx].opcode {
                                Opcode::Branch { else_target, .. }
                                | Opcode::VisionBranch { else_target, .. } => {
                                    *else_target = Some(else_idx);
                                }
                                Opcode::LoopStart { exit_target, .. } => {
                                    *exit_target = else_idx;
                                }
                                _ => {}
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
            // ─── Triggers
            // Hotkey/timer triggers just start the flow (Nop at execution)
            "hotkey_trigger" | "timer_trigger" => Ok(Opcode::Nop),

            // Screen triggers wait until their condition holds
            "pixel_color_trigger" => {
                let color = color_field(node, "color")?;
                let tolerance = tolerance_field(c);
                let check = match region_field(c) {
                    Some(region) => VisionCheck::ColorInRegion { region, color, tolerance },
                    None => VisionCheck::Pixel {
                        x: int_field(c, "x"),
                        y: int_field(c, "y"),
                        color,
                        tolerance,
                    },
                };
                Ok(Opcode::WaitForVision {
                    check,
                    poll_interval_ms: poll_interval_field(c, 100),
                })
            }
            "image_match_trigger" => Ok(Opcode::WaitForVision {
                check: image_check(node)?,
                poll_interval_ms: poll_interval_field(c, 250),
            }),

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
            "pixel_check" => Ok(Opcode::VisionBranch {
                check: VisionCheck::Pixel {
                    x: int_field(c, "x"),
                    y: int_field(c, "y"),
                    color: color_field(node, "expectedColor")?,
                    tolerance: tolerance_field(c),
                },
                else_target: None, // Patched in second pass
            }),
            "image_exists" => Ok(Opcode::VisionBranch {
                check: image_check(node)?,
                else_target: None, // Patched in second pass
            }),
            "if_else" => {
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

    // ─── Vision nodes ────────────────────────────────

    use serde_json::json;

    fn node(id: &str, kind: &str, category: &str, config: serde_json::Value) -> serde_json::Value {
        json!({"id": id, "type": category, "data": {"nodeKind": kind, "category": category, "config": config}})
    }

    fn edge(source: &str, target: &str, handle: Option<&str>) -> serde_json::Value {
        json!({"source": source, "target": target, "sourceHandle": handle})
    }

    fn compile_graph(nodes: Vec<serde_json::Value>, edges: Vec<serde_json::Value>) -> Result<CompiledFlow, CompileError> {
        compile(&json!({"id": "f", "name": "Vision", "nodes": nodes, "edges": edges}).to_string())
    }

    fn by_node<'a>(flow: &'a CompiledFlow, id: &str) -> (usize, &'a Instruction) {
        flow.instructions
            .iter()
            .enumerate()
            .find(|(_, i)| i.node_id == id)
            .unwrap_or_else(|| panic!("node {id} not compiled"))
    }

    fn halt_index(flow: &CompiledFlow) -> usize {
        by_node(flow, "__halt__").0
    }

    fn delay(id: &str) -> serde_json::Value {
        node(id, "delay", "action", json!({"label": id}))
    }

    #[test]
    fn pixel_check_compiles_to_vision_branch() {
        let flow = compile_graph(
            vec![
                node("c", "pixel_check", "condition", json!({"label": "Px", "x": -20, "y": 40, "expectedColor": "#00FF00", "tolerance": 12})),
                delay("yes"),
                delay("no"),
            ],
            vec![edge("c", "yes", Some("output-0")), edge("c", "no", Some("output-1"))],
        )
        .unwrap();

        let (_, branch) = by_node(&flow, "c");
        let (yes, _) = by_node(&flow, "yes");
        let (no, _) = by_node(&flow, "no");
        assert_eq!(branch.next, Some(yes));
        match &branch.opcode {
            Opcode::VisionBranch { check, else_target } => {
                assert_eq!(
                    *check,
                    VisionCheck::Pixel { x: -20, y: 40, color: "#00FF00".into(), tolerance: 12 }
                );
                assert_eq!(*else_target, Some(no));
            }
            other => panic!("expected VisionBranch, got {other:?}"),
        }
    }

    #[test]
    fn unconnected_condition_outputs_end_the_flow() {
        // Only the "false" output is connected: the true case must not
        // borrow that edge, it ends the flow.
        let flow = compile_graph(
            vec![node("c", "if_else", "condition", json!({"label": "If"})), delay("no")],
            vec![edge("c", "no", Some("output-1"))],
        )
        .unwrap();
        let (_, branch) = by_node(&flow, "c");
        let (no, _) = by_node(&flow, "no");
        assert_eq!(branch.next, Some(halt_index(&flow)));
        assert!(matches!(branch.opcode, Opcode::Branch { else_target: Some(t), .. } if t == no));

        // Only the "true" output is connected: a failed check ends the flow
        // instead of falling through to the true branch.
        let flow = compile_graph(
            vec![
                node("c", "image_exists", "condition", json!({"label": "Img", "templatePath": "a.bmp"})),
                delay("yes"),
            ],
            vec![edge("c", "yes", Some("output-0"))],
        )
        .unwrap();
        let (_, branch) = by_node(&flow, "c");
        let (yes, _) = by_node(&flow, "yes");
        let halt = halt_index(&flow);
        assert_eq!(branch.next, Some(yes));
        assert!(matches!(branch.opcode, Opcode::VisionBranch { else_target: Some(t), .. } if t == halt));
    }

    #[test]
    fn loop_without_exit_edge_ends_the_flow() {
        let flow = compile_graph(
            vec![node("l", "loop", "loop", json!({"label": "Loop", "count": 3})), delay("body")],
            vec![edge("l", "body", Some("output-0"))],
        )
        .unwrap();
        let (_, lp) = by_node(&flow, "l");
        let halt = halt_index(&flow);
        assert!(matches!(lp.opcode, Opcode::LoopStart { exit_target, .. } if exit_target == halt));
    }

    #[test]
    fn pixel_trigger_waits_for_pixel_or_region() {
        let flow = compile_graph(
            vec![node("t", "pixel_color_trigger", "trigger", json!({"label": "T", "x": 5, "y": 6, "color": "#ff0000", "tolerance": 3}))],
            vec![],
        )
        .unwrap();
        let (entry, trigger) = by_node(&flow, "t");
        assert_eq!(flow.entry_point, entry);
        match &trigger.opcode {
            Opcode::WaitForVision { check, poll_interval_ms } => {
                assert_eq!(*poll_interval_ms, 100);
                assert_eq!(
                    *check,
                    VisionCheck::Pixel { x: 5, y: 6, color: "#ff0000".into(), tolerance: 3 }
                );
            }
            other => panic!("expected WaitForVision, got {other:?}"),
        }

        let flow = compile_graph(
            vec![node("t", "pixel_color_trigger", "trigger", json!({
                "label": "T", "color": "#ff0000", "regionX": -100, "regionY": 0,
                "regionW": 50, "regionH": 20, "pollIntervalMs": 40
            }))],
            vec![],
        )
        .unwrap();
        match &by_node(&flow, "t").1.opcode {
            Opcode::WaitForVision { check, poll_interval_ms } => {
                assert_eq!(*poll_interval_ms, 40);
                assert_eq!(
                    *check,
                    VisionCheck::ColorInRegion {
                        region: Region { x: -100, y: 0, w: 50, h: 20 },
                        color: "#ff0000".into(),
                        tolerance: 10,
                    }
                );
            }
            other => panic!("expected WaitForVision, got {other:?}"),
        }
    }

    #[test]
    fn image_trigger_reads_region_and_defaults() {
        // Nested `region` objects are accepted too; zero size means full screen
        for (config, region) in [
            (json!({"label": "I", "templatePath": " btn.bmp ", "confidence": 0.8}), None),
            (
                json!({"label": "I", "templatePath": "btn.bmp", "confidence": 0.8, "region": {"x": 1, "y": 2, "w": 30, "h": 40}}),
                Some(Region { x: 1, y: 2, w: 30, h: 40 }),
            ),
            (
                json!({"label": "I", "templatePath": "btn.bmp", "confidence": 0.8, "regionW": 0, "regionH": 90}),
                None,
            ),
        ] {
            let flow = compile_graph(vec![node("t", "image_match_trigger", "trigger", config)], vec![]).unwrap();
            match &by_node(&flow, "t").1.opcode {
                Opcode::WaitForVision { check, poll_interval_ms } => {
                    assert_eq!(*poll_interval_ms, 250);
                    assert_eq!(
                        *check,
                        VisionCheck::Image { template_path: "btn.bmp".into(), confidence: 0.8, region }
                    );
                }
                other => panic!("expected WaitForVision, got {other:?}"),
            }
        }
    }

    #[test]
    fn invalid_vision_config_is_a_compile_error() {
        let cases = [
            node("n", "image_exists", "condition", json!({"label": "Görsel", "templatePath": ""})),
            node("n", "image_exists", "condition", json!({"label": "Görsel", "templatePath": "a.bmp", "confidence": 1.5})),
            node("n", "image_match_trigger", "trigger", json!({"label": "Görsel", "templatePath": "a.bmp", "confidence": 0})),
            node("n", "pixel_check", "condition", json!({"label": "Görsel", "expectedColor": "green"})),
            node("n", "pixel_color_trigger", "trigger", json!({"label": "Görsel", "color": "#12"})),
        ];
        for case in cases {
            match compile_graph(vec![case.clone()], vec![]) {
                Err(CompileError::InvalidConfig { node, .. }) => assert_eq!(node, "Görsel"),
                other => panic!("expected InvalidConfig for {case}, got {other:?}"),
            }
        }
    }
}
