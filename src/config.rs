// src/config.rs
use crate::error::{AptSyncError, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Parses the main config file, handling @import directives recursively.
/// Returns a deduplicated list of package names.
pub fn parse_config_file(path: &Path) -> Result<Vec<String>> {
    // Initialize the visited set for cycle detection
    parse_config_recursive(path, &mut HashSet::new())
}

// Helper function to handle recursion and prevent cycles
fn parse_config_recursive(path: &Path, visited_paths: &mut HashSet<PathBuf>) -> Result<Vec<String>> {
    // Attempt to get the canonical path early to handle relative paths consistently
    let absolute_path = match path.canonicalize() {
         Ok(p) => p,
         Err(e) => return Err(AptSyncError::Custom(format!("Failed to canonicalize path '{}': {}", path.display(), e))),
    };


    // Check for import cycles
    if !visited_paths.insert(absolute_path.clone()) {
        return Err(AptSyncError::Custom(format!(
            "Circular @import detected involving file: {}",
            path.display() // Display original path in error for clarity
        )));
    }

    if !path.exists() {
        visited_paths.remove(&absolute_path); 
        return Err(AptSyncError::Custom(format!(
            "Configuration file not found: {}",
            path.display()
        )));
    }

    let content = fs::read_to_string(path)?;
    let mut packages = Vec::new();
    let base_dir = path.parent().unwrap_or_else(|| Path::new(".")); // Use current dir if path has no parent

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("@import") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 2 {
                visited_paths.remove(&absolute_path); // Clean up before returning error
                return Err(AptSyncError::Custom(format!(
                    "Invalid @import syntax in {}:{} - Expected '@import <path>'",
                    path.display(),
                    line_num + 1
                )));
            }
            let import_path_str = parts[1];
            // Resolve the import path relative to the current file's directory
            let import_path = base_dir.join(import_path_str);

            // Recursively parse the imported file
            match parse_config_recursive(&import_path, visited_paths) {
                Ok(imported_packages) => packages.extend(imported_packages),
                Err(e) => {
                    visited_paths.remove(&absolute_path); // Clean up before propagating error
                    // Add context to the error message
                    return Err(AptSyncError::Custom(format!(
                        "Error importing file '{}' (from {}): {}",
                        import_path.display(),
                        path.display(),
                        e
                    )));
                }
            }
        } else {
            // Basic validation: ensure package name doesn't contain whitespace
            if line.contains(char::is_whitespace) {
                visited_paths.remove(&absolute_path); // Clean up before returning error
                return Err(AptSyncError::Custom(format!(
                    "Invalid package name '{}' (contains whitespace) in {}:{}",
                    line,
                    path.display(),
                    line_num + 1
                )));
            }
            packages.push(line.to_string());
        }
    }

    visited_paths.remove(&absolute_path); // Remove after successful parse of this file

    // Deduplicate packages specified multiple times within this file and its imports
    // Note: This deduplication happens *after* processing imports.
    let unique_packages: HashSet<String> = packages.into_iter().collect();
    Ok(unique_packages.into_iter().collect())
}
