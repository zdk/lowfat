//! Data file compression (JSON, YAML, TOML, XML, CSV).
//!
//! lite: passthrough (never corrupt structured data)
//! full: truncate arrays >10 items
//! ultra: + collapse nested objects to keys only

use crate::Level;

pub fn compress(content: &str, file_path: &str, level: Level) -> String {
    match level {
        Level::Lite => content.to_string(),
        Level::Full => truncate_arrays(content, file_path),
        Level::Ultra => {
            let s = truncate_arrays(content, file_path);
            collapse_deep_nesting(&s)
        }
    }
}

/// Truncate JSON arrays with >10 items to first 3 + count marker.
fn truncate_arrays(content: &str, file_path: &str) -> String {
    // Only attempt JSON truncation for JSON files
    if !file_path.ends_with(".json") && !file_path.ends_with(".jsonc") {
        return content.to_string();
    }

    // Parse as JSON value to find large arrays
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return content.to_string();
    };

    let truncated = truncate_value(value, 10);
    serde_json::to_string_pretty(&truncated).unwrap_or_else(|_| content.to_string())
}

fn truncate_value(value: serde_json::Value, max_items: usize) -> serde_json::Value {
    match value {
        serde_json::Value::Array(arr) => {
            if arr.len() > max_items {
                let kept: Vec<serde_json::Value> = arr
                    .into_iter()
                    .take(3)
                    .map(|v| truncate_value(v, max_items))
                    .collect();
                let mut result = kept;
                let remaining = result.len();
                result.push(serde_json::Value::String(format!(
                    "... [{} more items]",
                    max_items + 1 - remaining // approximate
                )));
                serde_json::Value::Array(result)
            } else {
                serde_json::Value::Array(
                    arr.into_iter()
                        .map(|v| truncate_value(v, max_items))
                        .collect(),
                )
            }
        }
        serde_json::Value::Object(map) => {
            let truncated_map: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .map(|(k, v)| (k, truncate_value(v, max_items)))
                .collect();
            serde_json::Value::Object(truncated_map)
        }
        other => other,
    }
}

/// For ultra: collapse deeply nested objects to key summaries.
fn collapse_deep_nesting(content: &str) -> String {
    // Simple line-based approach: if nesting >3 levels of indentation, summarize
    let mut result = String::with_capacity(content.len());
    let mut skipped = 0;

    for line in content.lines() {
        let indent = line.len() - line.trim_start().len();
        // ~3 levels = 6 spaces (2-space indent) or 12 spaces (4-space)
        if indent > 8 {
            skipped += 1;
            continue;
        }
        if skipped > 0 {
            let pad: String = " ".repeat(indent.saturating_sub(2).max(4));
            result.push_str(&format!("{}... [{} nested lines]\n", pad, skipped));
            skipped = 0;
        }
        result.push_str(line);
        result.push('\n');
    }

    if skipped > 0 {
        result.push_str(&format!("    ... [{} nested lines]\n", skipped));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lite_passes_through() {
        let input = r#"{"key": "value"}"#;
        assert_eq!(compress(input, "config.json", Level::Lite), input);
    }

    #[test]
    fn truncates_large_json_array() {
        let input = r#"{"items": [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]}"#;
        let result = compress(input, "data.json", Level::Full);
        assert!(result.contains("more items"));
        assert!(!result.contains("15"));
    }

    #[test]
    fn non_json_passthrough() {
        let input = "key: value\nlist:\n  - item1\n  - item2\n";
        assert_eq!(compress(input, "config.yaml", Level::Full), input);
    }
}
