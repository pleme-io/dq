//! Format auto-detection from file extensions and content parsing.
//!
//! Content detection uses a parse-attempt cascade (inspired by GitHub Linguist
//! and Google Magika) rather than naive string heuristics. The cascade order
//! exploits the fact that each format's parser rejects non-conforming input
//! quickly:
//!
//! 1. Extension match (unambiguous)
//! 2. Binary check (non-UTF8 → MsgPack)
//! 3. Parse-attempt cascade on content:
//!    JSON first (fastest to reject, unambiguous `{`/`[` syntax)
//!    → TOML (rarely overlaps, `[section]` + `key = val`)
//!    → HCL (block syntax distinctive)
//!    → YAML last (accepts almost anything as a valid scalar)

use crate::FormatKind;

/// Detect format from file path extension.
pub fn detect_from_path(path: &str) -> Option<FormatKind> {
    let ext = path.rsplit('.').next()?.to_lowercase();
    match ext.as_str() {
        "json" | "jsonl" | "geojson" => Some(FormatKind::Json),
        "yaml" | "yml" => Some(FormatKind::Yaml),
        "toml" => Some(FormatKind::Toml),
        "hcl" | "tf" | "tfvars" => Some(FormatKind::Hcl),
        "csv" | "tsv" => Some(FormatKind::Csv),
        "msgpack" | "mp" => Some(FormatKind::MsgPack),
        _ => None,
    }
}

/// Detect format from content using parse-attempt cascade.
///
/// Tries actual parsers in order of specificity. Each parser either succeeds
/// (confirming the format) or fails quickly. YAML is tried last because
/// almost any valid TOML/HCL/JSON text is also valid YAML (as a scalar string).
pub fn detect_from_content(input: &[u8]) -> Option<FormatKind> {
    // Stage 1: Binary check — non-UTF8 content is likely MessagePack
    let text = match std::str::from_utf8(input) {
        Ok(s) => s,
        Err(_) => {
            // Verify it actually parses as MessagePack
            if rmp_serde::from_slice::<serde_json::Value>(input).is_ok() {
                return Some(FormatKind::MsgPack);
            }
            return None;
        }
    };

    let trimmed = text.trim();

    // Empty content is not detectable
    if trimmed.is_empty() {
        return None;
    }

    // Stage 2: Try JSON (fastest rejection, unambiguous syntax)
    // JSON must start with { or [ for objects/arrays
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return Some(FormatKind::Json);
    }

    // Stage 3: Try TOML (rarely overlaps with other formats)
    if toml::from_str::<toml::Value>(trimmed).is_ok() {
        return Some(FormatKind::Toml);
    }

    // Stage 4: Try HCL (distinctive block syntax)
    // HCL parser is very permissive — verify the content has actual
    // HCL structure (attributes or blocks), not just a bare identifier
    if let Ok(body) = hcl::from_str::<hcl::Body>(trimmed) {
        if !body.0.is_empty() {
            return Some(FormatKind::Hcl);
        }
    }

    // Stage 5: Try YAML (last — accepts almost anything as valid scalar)
    if serde_saphyr::from_str::<serde_json::Value>(trimmed).is_ok() {
        return Some(FormatKind::Yaml);
    }

    None
}
