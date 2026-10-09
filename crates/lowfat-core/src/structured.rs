//! Structure-aware guard for JSON-shaped output.
//!
//! Line filters can cut JSON mid-structure; broken JSON misleads an agent
//! worse than verbose JSON. If raw output carries JSON but the filtered
//! result no longer does, re-compact from the parsed tree instead —
//! valid by construction, re-parsed as a final check before reaching
//! the agent.
//!
//! Recognised shapes: a single JSON document, NDJSON (one value per
//! line), and documents printed back to back. A few non-JSON lines may
//! sit before or after the JSON: stderr is appended to stdout, so
//! warnings often land there.

use crate::level::Level;
use serde_json::Value;

/// JSON-shaped output, as detected in raw text: optional non-JSON lines,
/// one or more JSON values, then optional non-JSON lines.
struct Shape {
    preamble: String,
    values: Vec<Value>,
    trailer: String,
}

/// Most non-JSON lines allowed before or after the JSON (warnings,
/// notices). Past this the text is prose that happens to hold JSON.
const MAX_EXTRA_LINES: usize = 20;

/// (max array items / NDJSON records, max string chars) per level.
fn caps(level: Level) -> (usize, usize) {
    match level {
        Level::Lite => (500, 2000),
        Level::Full => (50, 500),
        Level::Ultra => (10, 120),
    }
}

fn is_valid_json(text: &str) -> bool {
    serde_json::from_str::<serde::de::IgnoredAny>(text.trim()).is_ok()
}

/// Detect a JSON shape. None = not JSON-shaped, leave the text alone.
fn analyze(raw: &str) -> Option<Shape> {
    let t = raw.trim();
    // Warnings can print first, so try each early line that opens a value.
    let mut off = 0;
    for (i, line) in t.split_inclusive('\n').enumerate() {
        if i > MAX_EXTRA_LINES {
            break;
        }
        if line.starts_with(['{', '[']) {
            if let Some(shape) = analyze_from(t, off) {
                return Some(shape);
            }
        }
        off += line.len();
    }
    None
}

/// Read JSON values starting at `off`. Covers a single document, NDJSON,
/// and pretty-printed documents printed back to back (`go list -json`).
fn analyze_from(t: &str, off: usize) -> Option<Shape> {
    let body = &t[off..];
    let mut stream = serde_json::Deserializer::from_str(body).into_iter::<Value>();
    let mut values = Vec::new();
    let mut end = 0;
    loop {
        let rest = &body[end..];
        let next = end + rest.len() - rest.trim_start().len();
        if !body[next..].starts_with(['{', '[']) {
            break;
        }
        match stream.next() {
            Some(Ok(v)) => {
                values.push(v);
                end = stream.byte_offset();
            }
            Some(Err(e)) => {
                // `[WARN] ...` is text: it fails on the line it opens.
                // Anything else is cut or malformed JSON, so bail.
                let line = 1 + body[..next].matches('\n').count();
                if e.is_eof() || e.line() != line {
                    return None;
                }
                break;
            }
            None => break,
        }
    }
    let trailer = body[end..].trim();
    if values.is_empty() || trailer.lines().count() > MAX_EXTRA_LINES {
        return None;
    }
    Some(Shape {
        preamble: t[..off].trim_end().to_string(),
        values,
        trailer: trailer.to_string(),
    })
}

/// True when `raw` is JSON-shaped — see [`guard_json`] for the shapes.
pub fn is_json(raw: &str) -> bool {
    analyze(raw).is_some()
}

/// Cap arrays and long strings, recursively. Omissions become string
/// markers so the result stays parsable.
fn prune(v: &Value, level: Level) -> Value {
    let (max_items, max_str) = caps(level);
    match v {
        Value::Array(items) => {
            let mut out: Vec<Value> = items
                .iter()
                .take(max_items)
                .map(|x| prune(x, level))
                .collect();
            if items.len() > max_items {
                out.push(Value::String(format!(
                    "... {} more items (lowfat; LOWFAT_LEVEL=lite for more)",
                    items.len() - max_items
                )));
            }
            Value::Array(out)
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, x)| (k.clone(), prune(x, level)))
                .collect(),
        ),
        Value::String(s) => {
            let n = s.chars().count();
            if n > max_str {
                let cut: String = s.chars().take(max_str).collect();
                Value::String(format!("{cut}... (+{} chars)", n - max_str))
            } else {
                v.clone()
            }
        }
        other => other.clone(),
    }
}

fn recompact(shape: &Shape, level: Level) -> Option<String> {
    let (max_records, _) = caps(level);
    let mut lines: Vec<String> = Vec::new();
    if !shape.preamble.is_empty() {
        lines.push(shape.preamble.clone());
    }
    for v in shape.values.iter().take(max_records) {
        lines.push(serde_json::to_string(&prune(v, level)).ok()?);
    }
    if shape.values.len() > max_records {
        lines.push(format!(
            "\"... {} more records (lowfat; LOWFAT_LEVEL=lite for more)\"",
            shape.values.len() - max_records
        ));
    }
    if !shape.trailer.is_empty() {
        lines.push(shape.trailer.clone());
    }
    Some(lines.join("\n"))
}

/// True when `filtered` still parses the way the raw did: JSON values, plus
/// at most the raw's own warning lines. Leftover fragments around the JSON,
/// or a filter's "N lines omitted" marker, mean a line filter cut it.
fn is_intact(filtered: &str, raw: &Shape) -> bool {
    analyze(filtered).is_some_and(|f| {
        (f.preamble.is_empty() || f.preamble == raw.preamble)
            && (f.trailer.is_empty() || f.trailer == raw.trailer)
    })
}

/// If `raw` is JSON-shaped but `filtered` no longer is, return a compacted,
/// re-validated version built from the raw tree. None = keep `filtered`.
pub fn guard_json(raw: &str, filtered: &str, level: Level) -> Option<String> {
    let shape = analyze(raw)?;
    // Empty is a deliberate filter result (e.g. grep with no matches), and
    // still-JSON-shaped output (passthrough, grep over NDJSON) is fine.
    if filtered.trim().is_empty() || is_intact(filtered, &shape) {
        return None;
    }
    // Re-parse before it reaches the agent; raw is JSON-shaped by premise.
    match recompact(&shape, level) {
        Some(out) if analyze(&out).is_some() => Some(out),
        _ => Some(raw.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big_array_json(n: usize) -> String {
        let items: Vec<String> = (0..n).map(|i| format!("{{\"id\":{i}}}")).collect();
        format!("[{}]", items.join(","))
    }

    fn ndjson(n: usize) -> String {
        (0..n)
            .map(|i| format!("{{\"id\":{i}}}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn non_json_is_ignored() {
        assert!(guard_json("plain text", "txt", Level::Full).is_none());
    }

    #[test]
    fn intact_filtered_json_is_kept() {
        let raw = r#"{"a": 1}"#;
        assert!(guard_json(raw, r#"{"a":1}"#, Level::Full).is_none());
    }

    #[test]
    fn broken_filtered_json_is_recompacted() {
        let raw = big_array_json(100);
        let broken = &raw[..50]; // mid-structure cut, like a line truncation
        let fixed = guard_json(&raw, broken, Level::Full).unwrap();
        assert!(is_valid_json(&fixed));
        assert!(fixed.contains("50 more items"));
    }

    #[test]
    fn empty_filtered_output_is_respected() {
        // a filter dropping everything is deliberate, not broken structure
        assert!(guard_json(&big_array_json(3), "", Level::Full).is_none());
        assert!(guard_json(&ndjson(5), "\n", Level::Full).is_none());
    }

    #[test]
    fn array_cap_scales_with_level() {
        let raw = big_array_json(600);
        let ultra = guard_json(&raw, "x", Level::Ultra).unwrap();
        let lite = guard_json(&raw, "x", Level::Lite).unwrap();
        assert!(ultra.contains("590 more items"));
        assert!(lite.contains("100 more items"));
    }

    #[test]
    fn long_strings_are_capped() {
        let raw = format!(r#"{{"log": "{}"}}"#, "a".repeat(1000));
        let fixed = guard_json(&raw, "x", Level::Full).unwrap();
        assert!(is_valid_json(&fixed));
        assert!(fixed.contains("(+500 chars)"));
    }

    #[test]
    fn nested_structures_stay_valid() {
        let raw = format!(r#"{{"outer": {{"inner": {}}}}}"#, big_array_json(80));
        let fixed = guard_json(&raw, "x", Level::Full).unwrap();
        assert!(is_valid_json(&fixed));
        assert!(fixed.contains("30 more items"));
    }

    #[test]
    fn ndjson_is_recompacted_per_record() {
        let raw = ndjson(100);
        let fixed = guard_json(&raw, "{broken", Level::Full).unwrap();
        for line in fixed.lines() {
            assert!(is_valid_json(line), "broken record: {line}");
        }
        assert!(fixed.contains("50 more records"));
    }

    #[test]
    fn grep_over_ndjson_is_kept() {
        // dropping whole lines keeps NDJSON valid — guard must not interfere
        let raw = ndjson(10);
        let grepped = ndjson(3);
        assert!(guard_json(&raw, &grepped, Level::Full).is_none());
    }

    #[test]
    fn json_with_stderr_trailer_is_guarded() {
        // exit_command appends stderr after stdout
        let raw = format!("{}\nwarning: deprecated flag\n", big_array_json(80));
        let fixed = guard_json(&raw, "cut {mid", Level::Full).unwrap();
        assert!(fixed.contains("30 more items"));
        assert!(fixed.contains("warning: deprecated flag"));
        let json_part = fixed.lines().next().unwrap();
        assert!(is_valid_json(json_part));
    }

    #[test]
    fn back_to_back_documents_are_guarded() {
        // `go list -json`: pretty documents with no separator.
        let raw = "{\n  \"a\": 1\n}\n{\n  \"a\": 2\n}\n{\n  \"a\": 3\n}\n";
        let fixed = guard_json(raw, "{\n  \"a\": 1\n}\n{\n", Level::Full).unwrap();
        assert_eq!(fixed, "{\"a\":1}\n{\"a\":2}\n{\"a\":3}");
        // Cut inside the second document: not JSON-shaped, so it is repaired.
        assert!(!is_json("{\"a\":1}\n{\n  \"a\":"));
    }

    #[test]
    fn bracketed_trailer_is_text_not_json() {
        let raw = format!("{}\n[WARN] flag is deprecated\n", big_array_json(100));
        let fixed = guard_json(&raw, "[{\"id\":0},", Level::Full).unwrap();
        let (json_part, trailer) = fixed.split_once('\n').unwrap();
        assert!(is_valid_json(json_part));
        assert_eq!(trailer, "[WARN] flag is deprecated");
    }

    #[test]
    fn leading_warning_lines_are_kept() {
        let raw = format!("Warning: deprecated\n{}\n", big_array_json(100));
        let fixed = guard_json(&raw, "Warning: deprecated\n[{\"id\":0},", Level::Full).unwrap();
        let (preamble, json_part) = fixed.split_once('\n').unwrap();
        assert_eq!(preamble, "Warning: deprecated");
        assert!(is_valid_json(json_part));
    }

    #[test]
    fn prose_holding_json_is_not_json_shaped() {
        // Too much text around the value: leave it to the line filters.
        let tail = "log line\n".repeat(MAX_EXTRA_LINES + 1);
        assert!(!is_json(&format!("{{\"a\":1}}\n{tail}")));
        let head = "log line\n".repeat(MAX_EXTRA_LINES + 2);
        assert!(!is_json(&format!("{head}{{\"a\":1}}")));
        assert!(!is_json("[INFO] started\n[INFO] done"));
    }

    #[test]
    fn fragments_around_valid_json_are_repaired() {
        // `tail` on pretty JSON leaves debris, then a parsable inner value.
        let raw = big_array_json(100);
        let tailed = "    \"x\": 1\n  },\n{\"id\":99}\n]";
        assert!(is_valid_json(
            &guard_json(&raw, tailed, Level::Full).unwrap()
        ));
        // A filter's own marker line is not part of the JSON either.
        let cut = format!("{}\n... 3 lines omitted ...", ndjson(2));
        let fixed = guard_json(&ndjson(5), &cut, Level::Full).unwrap();
        assert!(fixed.lines().all(is_valid_json), "got: {fixed}");
    }

    #[test]
    fn malformed_json_is_left_alone() {
        // complete value followed by more JSON = malformed document, not a trailer
        let raw = r#"{"a":1}{"b":2}{"c":"#;
        assert!(guard_json(raw, "x", Level::Full).is_none());
    }
}
