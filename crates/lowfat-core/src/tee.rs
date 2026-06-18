use std::fs;
use std::path::Path;

/// Save raw output to a tee file on command failure.
/// Only saves if exit_code != 0 and output is >= 500 chars.
/// Keeps max 20 files in the tee directory.
pub fn save_on_failure(tee_dir: &Path, label: &str, raw: &str, exit_code: i32) {
    if exit_code == 0 || raw.len() < 500 {
        return;
    }

    if fs::create_dir_all(tee_dir).is_err() {
        return;
    }

    let safe_label = label.replace(' ', "_").replace('/', "_");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let filename = format!("{}_{}.log", timestamp, safe_label);
    let filepath = tee_dir.join(&filename);

    let _ = fs::write(&filepath, raw);

    // Keep max 20 files — remove oldest
    if let Ok(entries) = fs::read_dir(tee_dir) {
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .collect();
        if files.len() > 20 {
            files.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));
            for entry in &files[..files.len() - 20] {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    eprintln!("[raw output saved: {}]", filepath.display());
}
