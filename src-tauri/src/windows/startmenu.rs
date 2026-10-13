//! Start Menu shortcut index used to resolve application names dynamically
//! (so "open Spotify" works for any installed app without hardcoding).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use walkdir::WalkDir;

use crate::util::{normalize_text, similarity};

static INDEX: OnceLock<HashMap<String, PathBuf>> = OnceLock::new();

/// Lazily-built index: normalized shortcut name -> .lnk path.
pub fn index() -> &'static HashMap<String, PathBuf> {
    INDEX.get_or_init(build_index)
}

/// Find the best Start Menu match for a normalized application name.
pub fn search(normalized_name: &str) -> Option<PathBuf> {
    let index = index();
    if normalized_name.is_empty() {
        return None;
    }

    if let Some(exact) = index.get(normalized_name) {
        return Some(exact.clone());
    }

    let mut best: Option<(f32, &PathBuf)> = None;
    for (key, path) in index.iter() {
        // Prefer keys that contain the query (e.g. "visual studio code" contains "code").
        let score = if key.contains(normalized_name) && normalized_name.len() >= 3 {
            (0.9f32).max(similarity(normalized_name, key))
        } else {
            similarity(normalized_name, key)
        };
        if best.map(|(s, _)| score > s).unwrap_or(true) {
            best = Some((score, path));
        }
    }

    match best {
        Some((score, path)) if score >= 0.72 => Some(path.clone()),
        _ => None,
    }
}

fn build_index() -> HashMap<String, PathBuf> {
    let mut map: HashMap<String, PathBuf> = HashMap::new();
    for root in roots() {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root)
            .max_depth(8)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            let is_lnk = path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("lnk"))
                .unwrap_or(false);
            if !is_lnk {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let key = normalize_text(stem);
            if key.is_empty() || key.contains("uninstall") || key.contains("remove ") {
                continue;
            }
            map.entry(key).or_insert_with(|| path.to_path_buf());
        }
    }
    map
}

fn roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(
            PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs"),
        );
    }
    if let Some(programdata) = std::env::var_os("PROGRAMDATA") {
        roots.push(
            PathBuf::from(programdata).join("Microsoft\\Windows\\Start Menu\\Programs"),
        );
    }
    roots
}
