//! Structure-aware guard for JSON-shaped output.
//!
//! Line filters can cut JSON mid-structure; broken JSON misleads an agent
//! worse than verbose JSON. If raw output carries JSON but the filtered
//! result no longer does, re-compact from the parsed tree instead —
//! valid by construction, re-parsed as a final check before reaching
//! the agent.
//!
//! Recognised shapes: a single JSON document, NDJSON (one value per
//! line), and a JSON document followed by non-JSON trailer lines (stderr
//! is appended to stdout, so warnings often land after the JSON).

use crate::level::Level;
use serde_json::Value;

/// JSON-shaped output, as detected in raw text.
enum Shape {
    Single(Value),
    NdJson(Vec<Value>),
    WithTrailer(Value, String),
}

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
    if !(t.starts_with('{') || t.starts_with('[')) {
        return None;
    }
    if let Ok(v) = serde_json::from_str(t) {
        return Some(Shape::Single(v));
    }
    // NDJSON: every non-blank line is its own value
    let lines: Vec<&str> = t.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.len() >= 2 {
        if let Some(vals) = lines.iter().map(|l| serde_json::from_str(l).ok()).collect() {
            return Some(Shape::NdJson(vals));
        }
    }
    // One document + non-JSON trailer (e.g. appended stderr warnings).
    // A trailer that itself starts with {/[ means malformed JSON — bail.
    let mut stream = serde_json::Deserializer::from_str(t).into_iter::<Value>();
    if let Some(Ok(v)) = stream.next() {
        let rest = t[stream.byte_offset()..].trim();
        if !rest.is_empty() && !rest.starts_with('{') && !rest.starts_with('[') {
            return Some(Shape::WithTrailer(v, rest.to_string()));
        }
    }
    None
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
    let out = match shape {
        Shape::Single(v) => serde_json::to_string(&prune(v, level)).ok()?,
        Shape::NdJson(vals) => {
            let (max_records, _) = caps(level);
            let mut lines: Vec<String> = vals
                .iter()
                .take(max_records)
                .filter_map(|v| serde_json::to_string(&prune(v, level)).ok())
                .collect();
            if vals.len() > max_records {
                lines.push(format!(
                    "\"... {} more records (lowfat; LOWFAT_LEVEL=lite for more)\"",
                    vals.len() - max_records
                ));
            }
            lines.join("\n")
        }
        Shape::WithTrailer(v, trailer) => {
            format!(
                "{}\n{trailer}",
                serde_json::to_string(&prune(v, level)).ok()?
            )
        }
    };
    Some(out)
}

/// If `raw` is JSON-shaped but `filtered` no longer is, return a compacted,
/// re-validated version built from the raw tree. None = keep `filtered`.
pub fn guard_json(raw: &str, filtered: &str, level: Level) -> Option<String> {
    let shape = analyze(raw)?;
    // Empty is a deliberate filter result (e.g. grep with no matches), and
    // still-JSON-shaped output (passthrough, grep over NDJSON) is fine.
    if filtered.trim().is_empty() || analyze(filtered).is_some() {
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
    fn malformed_json_is_left_alone() {
        // complete value followed by more JSON = malformed document, not a trailer
        let raw = r#"{"a":1}{"b":2}{"c":"#;
        assert!(guard_json(raw, "x", Level::Full).is_none());
    }
}
