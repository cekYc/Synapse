// ============================================================
// Synapse — Import Safety Review
// ============================================================
// Summarizes what a shared flow will do before the user imports
// it. Every flow drives the mouse and keyboard; the review points
// out the steps that can reach beyond the flow itself — starting
// programs, typing text (which a focused terminal would execute),
// Windows-key shortcuts (Win+R opens the Run dialog) — plus a few
// things worth knowing (endless loops, templates that point
// outside the package).
// ============================================================

use super::{node_label, Package, ShareMeta, ASSET_REF_PREFIX};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Info,
    Warning,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub risk: Risk,
    pub node_id: String,
    pub node_label: String,
    pub message: String,
}

/// Everything the import dialog shows before the user confirms
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub meta: ShareMeta,
    pub flow_name: String,
    pub app_version: String,
    pub created_at: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub asset_count: usize,
    /// Mouse/keyboard steps (clicks, moves, key presses, typing)
    pub input_steps: usize,
    pub findings: Vec<Finding>,
    /// Highest risk among the findings (`info` when there are none)
    pub risk: Risk,
}

pub fn preview(package: &Package) -> ImportPreview {
    let nodes: &[Value] = package
        .flow
        .get("nodes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    let mut findings: Vec<Finding> = nodes.iter().flat_map(review_node).collect();
    // Most serious first, then in flow order
    findings.sort_by(|a, b| b.risk.cmp(&a.risk));

    ImportPreview {
        meta: package.meta.clone(),
        flow_name: package
            .flow
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&package.meta.name)
            .to_string(),
        app_version: package.app_version.clone(),
        created_at: package.created_at.clone(),
        node_count: nodes.len(),
        edge_count: package
            .flow
            .get("edges")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        asset_count: package.assets.len(),
        input_steps: nodes
            .iter()
            .filter(|n| {
                matches!(
                    kind(n),
                    "mouse_click" | "mouse_move" | "key_press" | "type_text"
                )
            })
            .count(),
        risk: findings.iter().map(|f| f.risk).max().unwrap_or(Risk::Info),
        findings,
    }
}

fn kind(node: &Value) -> &str {
    node.pointer("/data/nodeKind").and_then(Value::as_str).unwrap_or("")
}

fn review_node(node: &Value) -> Vec<Finding> {
    let config = node.pointer("/data/config").cloned().unwrap_or(Value::Null);
    let text = |key: &str| config.get(key).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let finding = |risk: Risk, message: String| Finding {
        risk,
        node_id: node.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
        node_label: node_label(node),
        message,
    };

    let mut out = Vec::new();
    match kind(node) {
        "run_program" => {
            let args: Vec<&str> = config
                .get("args")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let command = std::iter::once(text("path").as_str())
                .chain(args.iter().copied())
                .collect::<Vec<_>>()
                .join(" ");
            out.push(finding(
                Risk::Danger,
                format!("Program çalıştırır: {}", shorten(&command, 160)),
            ));
        }
        "type_text" => {
            let typed = text("text");
            if !typed.is_empty() {
                out.push(finding(
                    Risk::Warning,
                    format!(
                        "Metin yazar (odaktaki pencereye gider; terminalde komut olarak çalışabilir): “{}”",
                        shorten(&typed, 120)
                    ),
                ));
            }
        }
        "key_press" => {
            let has_win = config
                .get("modifiers")
                .and_then(Value::as_array)
                .is_some_and(|m| m.iter().any(|v| v.as_str() == Some("win")));
            if has_win {
                out.push(finding(
                    Risk::Warning,
                    format!("Windows tuşu kısayolu gönderir: Win + {}", text("key")),
                ));
            }
        }
        "loop" if config.get("count").and_then(Value::as_u64) == Some(0) => {
            out.push(finding(
                Risk::Info,
                "Sonsuz döngü içerir (Durdur ile sonlandırılır)".into(),
            ));
        }
        _ => {}
    }

    let template = text("templatePath");
    if !template.is_empty() && !template.starts_with(ASSET_REF_PREFIX) {
        out.push(finding(
            Risk::Info,
            format!(
                "Şablon görseli pakette değil, bu yola başvuruyor: {}",
                shorten(&template, 120)
            ),
        ));
    }
    out
}

fn shorten(text: &str, max_chars: usize) -> String {
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() > max_chars {
        let cut: String = single_line.chars().take(max_chars).collect();
        format!("{cut}…")
    } else {
        single_line
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{flow, node};
    use super::super::{build_package, ShareMeta};
    use super::*;
    use serde_json::json;

    fn preview_of(nodes: Vec<Value>) -> ImportPreview {
        let package = build_package(&flow(nodes, vec![]), ShareMeta::default()).unwrap();
        preview(&package)
    }

    #[test]
    fn harmless_flow_has_no_findings() {
        let p = preview_of(vec![
            node("c", "mouse_click", json!({"label": "Tıkla", "x": 1, "y": 2})),
            node("k", "key_press", json!({"label": "Enter", "key": "Enter", "modifiers": ["ctrl"]})),
            node("d", "delay", json!({"label": "Bekle"})),
        ]);
        assert!(p.findings.is_empty());
        assert_eq!(p.risk, Risk::Info);
        assert_eq!((p.node_count, p.input_steps), (3, 2));
    }

    #[test]
    fn risky_steps_are_reported_most_serious_first() {
        let p = preview_of(vec![
            node("l", "loop", json!({"label": "Sonsuz", "count": 0})),
            node("t", "type_text", json!({"label": "Yaz", "text": "del /q *\n"})),
            node("k", "key_press", json!({"label": "Çalıştır", "key": "r", "modifiers": ["win"]})),
            node("r", "run_program", json!({"label": "Program", "path": "cmd.exe", "args": ["/c", "echo hi"]})),
        ]);

        let risks: Vec<Risk> = p.findings.iter().map(|f| f.risk).collect();
        assert_eq!(risks, vec![Risk::Danger, Risk::Warning, Risk::Warning, Risk::Info]);
        assert_eq!(p.risk, Risk::Danger);
        assert_eq!(p.findings[0].node_label, "Program");
        assert!(p.findings[0].message.contains("cmd.exe /c echo hi"));
        assert!(p.findings.iter().any(|f| f.message.contains("del /q *")));
        assert!(p.findings.iter().any(|f| f.message.contains("Win + r")));
    }

    #[test]
    fn long_text_is_shortened_to_one_line() {
        assert_eq!(shorten("a\n  b\tc", 10), "a b c");
        assert_eq!(shorten(&"x".repeat(200), 5), "xxxxx…");
    }
}
