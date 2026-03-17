pub mod parser;
pub mod wgsl;

use std::path::{Path, PathBuf};

/// Recursively collect all `.rs` files from the given paths (files or directories).
pub fn collect_rust_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            for entry in walkdir(path) {
                if entry.extension().is_some_and(|e| e == "rs") {
                    files.push(entry);
                }
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path.clone());
        }
    }
    files
}

fn walkdir(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                results.extend(walkdir(&path));
            } else {
                results.push(path);
            }
        }
    }
    results
}

/// Scan Rust source files and generate WGSL type definitions.
pub fn generate_wgsl_from_files(paths: &[PathBuf]) -> Result<String, String> {
    let files = collect_rust_files(paths);

    if files.is_empty() {
        return Err("No .rs files found in the provided paths".to_string());
    }

    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut aliases = std::collections::HashMap::new();

    for file in &files {
        let content = std::fs::read_to_string(file)
            .map_err(|e| format!("could not read {}: {}", file.display(), e))?;
        let syntax = syn::parse_file(&content)
            .map_err(|e| format!("could not parse {}: {}", file.display(), e))?;
        let (s, e, a) = parser::extract_types(&syntax);
        structs.extend(s);
        enums.extend(e);
        aliases.extend(a);
    }

    parser::resolve_aliases(&mut structs, &mut enums, &aliases);

    Ok(wgsl::generate_wgsl(&structs, &enums))
}
