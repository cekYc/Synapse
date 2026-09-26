// ============================================================
// Synapse — Flow Sharing (portable packages & share codes)
// ============================================================
// A shared flow is a self-contained, versioned JSON package:
//
//   {
//     "format": "synapse-flow", "formatVersion": 1,
//     "appVersion": "0.1.0", "createdAt": "…",
//     "meta":   { "name", "description", "author", "tags" },
//     "flow":   { …the editor's flow document… },
//     "assets": { "img1": "<base64 BMP>", … }
//   }
//
// Template images are embedded as assets and referenced from
// nodes as `asset:<id>`, so a package works on any machine.
// It is saved as a `.synapse` file or turned into a share code
// (`SYN1:` + base64url(deflate(json))) for copy & paste.
//
// Packages come from other people, so import is defensive: the
// package is fully validated before anything touches the disk,
// asset file names are generated (never taken from the package),
// sizes are capped (including the decompressed size of share
// codes), and nothing is executed — the user reviews the safety
// report (`review.rs`) and loads the flow into the editor.
// ============================================================

pub mod review;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "synapse-flow";
pub const FORMAT_VERSION: u32 = 1;
pub const FILE_EXTENSION: &str = "synapse";
const SHARE_CODE_PREFIX: &str = "SYN1:";
const ASSET_REF_PREFIX: &str = "asset:";

/// Upper bound for a package file and for a decoded share code
const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
/// Upper bound for a single embedded template image
const MAX_ASSET_BYTES: usize = 10 * 1024 * 1024;
const MAX_NODES: usize = 10_000;
const MAX_EDGES: usize = 20_000;

/// Node kinds this version understands, with the category each belongs to.
/// Must stay in sync with the compiler (checked by a test).
pub(crate) const NODE_KINDS: &[(&str, &str)] = &[
    ("hotkey_trigger", "trigger"),
    ("pixel_color_trigger", "trigger"),
    ("image_match_trigger", "trigger"),
    ("timer_trigger", "trigger"),
    ("mouse_click", "action"),
    ("mouse_move", "action"),
    ("key_press", "action"),
    ("type_text", "action"),
    ("delay", "action"),
    ("run_program", "action"),
    ("set_variable", "action"),
    ("if_else", "condition"),
    ("pixel_check", "condition"),
    ("image_exists", "condition"),
    ("loop", "loop"),
    ("while_loop", "loop"),
];

/// Descriptive metadata the author attaches when sharing
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub format: String,
    pub format_version: u32,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub created_at: String,
    pub meta: ShareMeta,
    pub flow: Value,
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
}

// ─── Export ─────────────────────────────────────────

/// Build a package from the editor's flow JSON, embedding every template
/// image the flow references
pub fn build_package(flow_json: &str, mut meta: ShareMeta) -> Result<Package, String> {
    let mut flow: Value =
        serde_json::from_str(flow_json).map_err(|e| format!("Akış okunamadı: {e}"))?;
    let nodes = flow
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or("Akışta düğüm listesi yok")?;

    let mut assets = BTreeMap::new();
    let mut ids_by_path: HashMap<String, String> = HashMap::new();

    for node in nodes.iter_mut() {
        let label = node_label(node);
        let Some(config) = node.pointer_mut("/data/config").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(path) = config.get("templatePath").and_then(Value::as_str) else {
            continue;
        };
        let path = path.trim().to_string();
        if path.is_empty() || path.starts_with(ASSET_REF_PREFIX) {
            continue;
        }

        let asset_id = match ids_by_path.get(&path) {
            Some(id) => id.clone(),
            None => {
                let bytes = std::fs::read(&path)
                    .map_err(|e| format!("'{label}': şablon görseli okunamadı ({path}): {e}"))?;
                check_asset(&bytes).map_err(|e| format!("'{label}': {e} ({path})"))?;
                let id = format!("img{}", assets.len() + 1);
                assets.insert(id.clone(), STANDARD.encode(&bytes));
                ids_by_path.insert(path.clone(), id.clone());
                id
            }
        };
        config.insert(
            "templatePath".into(),
            Value::String(format!("{ASSET_REF_PREFIX}{asset_id}")),
        );
    }

    if meta.name.trim().is_empty() {
        meta.name = flow
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("İsimsiz Flow")
            .to_string();
    }
    meta.tags = normalize_tags(&meta.tags);

    let package = Package {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        meta,
        flow,
        assets,
    };
    validate(&package)?;
    Ok(package)
}

/// Serialize a package for a `.synapse` file (readable JSON)
pub fn to_file_bytes(package: &Package) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(package).map_err(|e| format!("Paket yazılamadı: {e}"))
}

/// Encode a package as a compact share code
pub fn to_share_code(package: &Package) -> Result<String, String> {
    let json = serde_json::to_vec(package).map_err(|e| format!("Paket yazılamadı: {e}"))?;
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(&json)
        .and_then(|_| encoder.finish())
        .map(|compressed| format!("{SHARE_CODE_PREFIX}{}", URL_SAFE_NO_PAD.encode(compressed)))
        .map_err(|e| format!("Paylaşım kodu oluşturulamadı: {e}"))
}

// ─── Import ─────────────────────────────────────────

/// Read and validate a `.synapse` file
pub fn read_file(path: &Path) -> Result<Package, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Dosya açılamadı: {e}"))?;
    let bytes = read_capped(file)?;
    parse(&bytes)
}

/// Decode and validate a share code (whitespace is ignored, so codes that
/// were wrapped across lines still work)
pub fn read_share_code(code: &str) -> Result<Package, String> {
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let payload = compact
        .strip_prefix(SHARE_CODE_PREFIX)
        .ok_or("Geçersiz paylaşım kodu (SYN1: ile başlamalı)")?;
    let compressed = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "Paylaşım kodu bozuk (base64 çözülemedi)")?;
    parse(&read_capped(DeflateDecoder::new(compressed.as_slice()))?)
}

/// Read at most `MAX_PACKAGE_BYTES`; guards against huge files and
/// decompression bombs
fn read_capped(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Paket okunamadı: {e}"))?;
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(format!(
            "Paket çok büyük (en fazla {} MB)",
            MAX_PACKAGE_BYTES / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

fn parse(bytes: &[u8]) -> Result<Package, String> {
    // Check the envelope first so unrelated JSON gets a clear message
    let raw: Value = serde_json::from_slice(bytes).map_err(|_| "Dosya bir Synapse paketi değil")?;
    if raw.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err("Dosya bir Synapse paketi değil".into());
    }
    match raw.get("formatVersion").and_then(Value::as_u64) {
        Some(v) if v == FORMAT_VERSION as u64 => {}
        Some(v) if v > FORMAT_VERSION as u64 => {
            return Err(format!(
                "Paket daha yeni bir Synapse sürümüyle oluşturulmuş (format {v}); uygulamayı güncelleyin"
            ))
        }
        _ => return Err("Desteklenmeyen paket formatı sürümü".into()),
    }

    let package: Package =
        serde_json::from_value(raw).map_err(|e| format!("Paket yapısı geçersiz: {e}"))?;
    validate(&package)?;
    Ok(package)
}

/// Structural checks shared by export and import
fn validate(package: &Package) -> Result<(), String> {
    let nodes = package
        .flow
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or("Pakette düğüm listesi yok")?;
    let edges = match package.flow.get("edges") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(edges)) => edges.as_slice(),
        Some(_) => return Err("Paketteki bağlantı listesi geçersiz".into()),
    };
    if nodes.len() > MAX_NODES || edges.len() > MAX_EDGES {
        return Err("Pakette çok fazla düğüm veya bağlantı var".into());
    }

    let kinds: HashMap<&str, &str> = NODE_KINDS.iter().copied().collect();
    let mut ids = HashSet::new();
    let mut referenced_assets = HashSet::new();

    for node in nodes {
        let id = node.get("id").and_then(Value::as_str).ok_or("Kimliği olmayan düğüm var")?;
        if !ids.insert(id) {
            return Err(format!("Aynı kimliğe sahip birden fazla düğüm var: '{id}'"));
        }
        let kind = node.pointer("/data/nodeKind").and_then(Value::as_str).unwrap_or("");
        let category = node.pointer("/data/category").and_then(Value::as_str).unwrap_or("");
        match kinds.get(kind) {
            None => {
                return Err(format!(
                    "Paket bu sürümde olmayan bir düğüm türü içeriyor: '{kind}'; uygulamayı güncelleyin"
                ))
            }
            Some(expected) if *expected != category => {
                return Err(format!("'{id}' düğümünün kategorisi türüyle uyuşmuyor"))
            }
            Some(_) => {}
        }
        if !node.pointer("/data/config").is_some_and(Value::is_object) {
            return Err(format!("'{id}' düğümünün ayarları eksik"));
        }
        if let Some(asset) = node
            .pointer("/data/config/templatePath")
            .and_then(Value::as_str)
            .and_then(|p| p.trim().strip_prefix(ASSET_REF_PREFIX))
        {
            if !package.assets.contains_key(asset) {
                return Err(format!("'{id}' düğümü pakette olmayan bir görsele başvuruyor"));
            }
            referenced_assets.insert(asset);
        }
    }

    for edge in edges {
        for end in ["source", "target"] {
            let node = edge.get(end).and_then(Value::as_str).unwrap_or("");
            if !ids.contains(node) {
                return Err("Paketteki bir bağlantı var olmayan bir düğüme gidiyor".into());
            }
        }
    }

    for (id, data) in &package.assets {
        if !is_valid_asset_id(id) {
            return Err("Paketteki bir görselin adı geçersiz".into());
        }
        if referenced_assets.contains(id.as_str()) {
            let bytes = STANDARD.decode(data).map_err(|_| "Paketteki bir görsel bozuk")?;
            check_asset(&bytes)?;
        }
    }
    Ok(())
}

/// Write the package's images under `assets_root/<new flow id>/` and return
/// the flow JSON ready to load into the editor. The flow gets a fresh id so
/// importing never overwrites an existing flow.
pub fn install(package: Package, assets_root: &Path) -> Result<String, String> {
    validate(&package)?;
    let mut flow = package.flow;
    let flow_id = uuid::Uuid::new_v4().to_string();

    let asset_dir = assets_root.join(&flow_id);
    let mut written: HashMap<String, PathBuf> = HashMap::new();

    if let Some(nodes) = flow.get_mut("nodes").and_then(Value::as_array_mut) {
        for node in nodes.iter_mut() {
            // The editor picks the node component from `type`; derive it
            // from the validated category instead of trusting the package
            if let Some(category) = node.pointer("/data/category").cloned() {
                node["type"] = category;
            }
            let Some(config) = node.pointer_mut("/data/config").and_then(Value::as_object_mut) else {
                continue;
            };
            let Some(asset_id) = config
                .get("templatePath")
                .and_then(Value::as_str)
                .and_then(|p| p.trim().strip_prefix(ASSET_REF_PREFIX))
                .map(str::to_string)
            else {
                continue;
            };

            let path = match written.get(&asset_id) {
                Some(path) => path.clone(),
                None => {
                    // `validate` guarantees the id is a plain token, so the
                    // file stays inside `asset_dir`
                    let bytes = STANDARD
                        .decode(&package.assets[&asset_id])
                        .map_err(|_| "Paketteki bir görsel bozuk")?;
                    std::fs::create_dir_all(&asset_dir)
                        .map_err(|e| format!("Görsel klasörü oluşturulamadı: {e}"))?;
                    let path = asset_dir.join(format!("{asset_id}.bmp"));
                    std::fs::write(&path, bytes).map_err(|e| format!("Görsel kaydedilemedi: {e}"))?;
                    written.insert(asset_id.clone(), path.clone());
                    path
                }
            };
            config.insert(
                "templatePath".into(),
                Value::String(path.to_string_lossy().into_owned()),
            );
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    if let Some(obj) = flow.as_object_mut() {
        obj.insert("id".into(), Value::String(flow_id));
        obj.insert("updatedAt".into(), Value::String(now.clone()));
        obj.entry("createdAt").or_insert(Value::String(now));
        if !obj.get("name").is_some_and(Value::is_string) {
            obj.insert("name".into(), Value::String(package.meta.name.clone()));
        }
        obj.entry("edges").or_insert(Value::Array(Vec::new()));
        obj.entry("viewport")
            .or_insert(serde_json::json!({ "x": 0, "y": 0, "zoom": 1 }));
    }

    serde_json::to_string(&flow).map_err(|e| format!("Akış yazılamadı: {e}"))
}

// ─── Helpers ────────────────────────────────────────

fn check_asset(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(format!(
            "Şablon görseli çok büyük (en fazla {} MB)",
            MAX_ASSET_BYTES / (1024 * 1024)
        ));
    }
    if bytes.len() < 54 || &bytes[..2] != b"BM" {
        return Err("Şablon görseli BMP değil".into());
    }
    Ok(())
}

fn is_valid_asset_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty() && seen.insert(t.clone()))
        .take(10)
        .collect()
}

pub(crate) fn node_label(node: &Value) -> String {
    node.pointer("/data/config/label")
        .and_then(Value::as_str)
        .or_else(|| node.get("id").and_then(Value::as_str))
        .unwrap_or("?")
        .to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// A tiny valid 2×2 24-bit BMP
    pub fn bmp() -> Vec<u8> {
        let mut data = b"BM".to_vec();
        data.extend_from_slice(&70u32.to_le_bytes());
        data.extend_from_slice(&[0; 4]);
        data.extend_from_slice(&54u32.to_le_bytes());
        data.extend_from_slice(&40u32.to_le_bytes());
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&[1, 0, 24, 0]);
        data.extend_from_slice(&[0; 24]);
        data.extend_from_slice(&[0, 0, 255, 255, 255, 255, 0, 0]);
        data.extend_from_slice(&[0, 255, 0, 0, 0, 0, 0, 0]);
        data
    }

    pub struct TempDir(pub PathBuf);
    impl TempDir {
        pub fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "synapse-share-{name}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub fn node(id: &str, kind: &str, config: Value) -> Value {
        let category = NODE_KINDS.iter().find(|(k, _)| *k == kind).unwrap().1;
        json!({"id": id, "type": category, "position": {"x": 0, "y": 0},
               "data": {"nodeKind": kind, "category": category, "config": config}})
    }

    pub fn flow(nodes: Vec<Value>, edges: Vec<Value>) -> String {
        json!({"id": "orig-id", "name": "Demo", "version": 1, "nodes": nodes, "edges": edges,
               "viewport": {"x": 0, "y": 0, "zoom": 1}, "variables": []})
        .to_string()
    }

    fn meta() -> ShareMeta {
        ShareMeta { name: "Demo".into(), author: "tester".into(), ..Default::default() }
    }

    fn sample_with_template(dir: &Path) -> String {
        let template = dir.join("button.bmp");
        std::fs::write(&template, bmp()).unwrap();
        let path = template.to_string_lossy().into_owned();
        flow(
            vec![
                node("t", "image_match_trigger", json!({"label": "Tetik", "templatePath": path, "confidence": 0.9})),
                node("c", "image_exists", json!({"label": "Var mı", "templatePath": path, "confidence": 0.8})),
                node("d", "delay", json!({"label": "Bekle", "durationMs": 100})),
            ],
            vec![json!({"source": "t", "target": "c"}), json!({"source": "c", "target": "d", "sourceHandle": "output-0"})],
        )
    }

    #[test]
    fn file_round_trip_embeds_and_restores_templates() {
        let src = TempDir::new("src");
        let package = build_package(&sample_with_template(&src.0), meta()).unwrap();

        // One asset shared by both nodes, referenced by id, no local paths left
        assert_eq!(package.assets.len(), 1);
        let text = String::from_utf8(to_file_bytes(&package).unwrap()).unwrap();
        assert!(text.contains("\"asset:img1\""));
        assert!(!text.contains("button.bmp"));

        let dest = TempDir::new("dest");
        let file = dest.0.join("demo.synapse");
        std::fs::write(&file, to_file_bytes(&package).unwrap()).unwrap();
        let imported: Value =
            serde_json::from_str(&install(read_file(&file).unwrap(), &dest.0.join("assets")).unwrap()).unwrap();

        let new_id = imported["id"].as_str().unwrap();
        assert_ne!(new_id, "orig-id", "imports get a fresh id");
        let restored = imported["nodes"][0]["data"]["config"]["templatePath"].as_str().unwrap();
        assert_eq!(restored, imported["nodes"][1]["data"]["config"]["templatePath"].as_str().unwrap());
        assert!(Path::new(restored).starts_with(dest.0.join("assets").join(new_id)));
        assert_eq!(std::fs::read(restored).unwrap(), bmp());
        assert_eq!(imported["edges"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn install_derives_node_type_from_category() {
        let mut spoofed = node("d", "delay", json!({"label": "Bekle"}));
        spoofed["type"] = json!("custom-component");
        let package = package_with(json!({"name": "X", "nodes": [spoofed]}), BTreeMap::new());
        let dest = TempDir::new("type");
        let imported: Value = serde_json::from_str(&install(package, &dest.0).unwrap()).unwrap();
        assert_eq!(imported["nodes"][0]["type"], "action");
        assert!(imported["edges"].as_array().unwrap().is_empty());
    }

    #[test]
    fn share_code_round_trip() {
        let src = TempDir::new("code");
        let package = build_package(&sample_with_template(&src.0), meta()).unwrap();
        let code = to_share_code(&package).unwrap();
        assert!(code.starts_with("SYN1:"));

        // Wrapped across lines when pasted
        let wrapped: String = code
            .chars()
            .collect::<Vec<_>>()
            .chunks(40)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n  ");
        let decoded = read_share_code(&wrapped).unwrap();
        assert_eq!(decoded.meta.name, "Demo");
        assert_eq!(decoded.assets, package.assets);
        assert_eq!(decoded.flow, package.flow);
    }

    #[test]
    fn export_requires_existing_bmp_templates() {
        let dir = TempDir::new("missing");
        let missing = flow(
            vec![node("c", "image_exists", json!({"label": "Var mı", "templatePath": dir.0.join("nope.bmp")}))],
            vec![],
        );
        let err = build_package(&missing, meta()).unwrap_err();
        assert!(err.contains("Var mı"), "{err}");

        let png = dir.0.join("image.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n not a bmp at all, padding padding padding padding").unwrap();
        let not_bmp = flow(vec![node("c", "image_exists", json!({"label": "Var mı", "templatePath": png}))], vec![]);
        assert!(build_package(&not_bmp, meta()).unwrap_err().contains("BMP"));
    }

    #[test]
    fn meta_defaults_and_tags_are_normalized() {
        let package = build_package(
            &flow(vec![node("d", "delay", json!({"label": "Bekle"}))], vec![]),
            ShareMeta {
                name: "  ".into(),
                tags: vec![" Oyun ".into(), "oyun".into(), "".into(), "Ofis".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(package.meta.name, "Demo");
        assert_eq!(package.meta.tags, vec!["oyun", "ofis"]);
    }

    fn package_with(flow_value: Value, assets: BTreeMap<String, String>) -> Package {
        Package {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            app_version: "0.1.0".into(),
            created_at: String::new(),
            meta: meta(),
            flow: flow_value,
            assets,
        }
    }

    #[test]
    fn rejects_malformed_packages() {
        let ok_nodes = vec![node("a", "delay", json!({"label": "A"}))];
        let mut unknown_kind = node("a", "delay", json!({}));
        unknown_kind["data"]["nodeKind"] = json!("self_destruct");
        let mut wrong_category = node("a", "delay", json!({}));
        wrong_category["data"]["category"] = json!("trigger");

        let cases: Vec<(Value, BTreeMap<String, String>, &str)> = vec![
            (json!({"nodes": ok_nodes, "edges": [{"source": "a", "target": "ghost"}]}), BTreeMap::new(), "bağlantı"),
            (json!({"nodes": [ok_nodes[0], ok_nodes[0]]}), BTreeMap::new(), "Aynı kimliğe"),
            (json!({"nodes": [unknown_kind]}), BTreeMap::new(), "düğüm türü"),
            (json!({"nodes": [wrong_category]}), BTreeMap::new(), "kategori"),
            (json!({"nodes": [node("a", "image_exists", json!({"templatePath": "asset:img9"}))]}), BTreeMap::new(), "olmayan bir görsel"),
            (
                json!({"nodes": [node("a", "image_exists", json!({"templatePath": "asset:img1"}))]}),
                BTreeMap::from([("img1".to_string(), STANDARD.encode(b"not an image"))]),
                "BMP",
            ),
            (
                json!({"nodes": ok_nodes}),
                BTreeMap::from([("../../evil".to_string(), STANDARD.encode(bmp()))]),
                "adı geçersiz",
            ),
        ];
        for (flow_value, assets, expected) in cases {
            let err = validate(&package_with(flow_value.clone(), assets)).unwrap_err();
            assert!(err.contains(expected), "{flow_value}: expected '{expected}', got '{err}'");
        }
    }

    #[test]
    fn rejects_foreign_or_future_files() {
        assert!(parse(b"{\"hello\": 1}").unwrap_err().contains("Synapse paketi değil"));
        assert!(parse(b"not json").unwrap_err().contains("Synapse paketi değil"));
        let future = json!({"format": FORMAT, "formatVersion": 99, "meta": {"name": "x"}, "flow": {"nodes": []}});
        assert!(parse(future.to_string().as_bytes()).unwrap_err().contains("daha yeni"));
        assert!(read_share_code("hello").is_err());
        assert!(read_share_code("SYN1:!!!").is_err());
    }

    #[test]
    fn share_code_decompression_is_capped() {
        // ~65 MB of zeros compresses to a few dozen KB
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
        let chunk = vec![0u8; 1 << 20];
        for _ in 0..65 {
            encoder.write_all(&chunk).unwrap();
        }
        let code = format!("SYN1:{}", URL_SAFE_NO_PAD.encode(encoder.finish().unwrap()));
        assert!(read_share_code(&code).unwrap_err().contains("çok büyük"));
    }

    #[test]
    fn node_kinds_match_the_compiler() {
        use crate::engine::compiler::{compile, CompileError};
        for (kind, category) in NODE_KINDS {
            let flow = json!({"id": "f", "name": "k", "edges": [],
                "nodes": [{"id": "n", "data": {"nodeKind": kind, "category": category, "config": {}}}]});
            if let Err(CompileError::UnknownNodeKind(k)) = compile(&flow.to_string()) {
                panic!("sharing accepts '{k}' but the compiler does not know it");
            }
        }
    }
}
