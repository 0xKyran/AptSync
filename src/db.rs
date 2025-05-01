// src/db.rs
use crate::error::{AptSyncError, Result};
use rusqlite::{Connection, params, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Represents detailed package information for the 'list' command.
#[derive(Debug)]
pub struct PackageInfo {
    pub name: String,
    pub install_date: String,
}

/// Creates the packages table if it doesn't exist.
pub fn init_db(db_path: &Path) -> Result<()> {
    let conn = Connection::open(db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS packages (
            name TEXT PRIMARY KEY NOT NULL,
            installed_by_aptsync BOOLEAN NOT NULL DEFAULT 0,
            install_date DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;
    // Optional: Add index for faster lookups if DB grows large
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_packages_name ON packages(name)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_packages_managed ON packages(installed_by_aptsync)",
        [],
    )?;
    Ok(())
}

/// Retrieves all packages currently tracked in the database along with their management status.
/// Returns a HashMap where the key is the package name and the value is the `installed_by_aptsync` flag.
pub fn get_all_tracked_packages(db_path: &Path) -> Result<HashMap<String, bool>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT name, installed_by_aptsync FROM packages")?;
    let packages_iter = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?)) // Tuple (String, bool)
    })?;

    let mut packages_map = HashMap::new();
    for package_result in packages_iter {
        match package_result {
            Ok((name, installed_by_aptsync)) => {
                packages_map.insert(name, installed_by_aptsync);
            }
            Err(e) => {
                // Log error but continue if possible, or return error depending on severity needed
                eprintln!("Error reading package status from database: {}", e);
                // Consider returning Err(e.into()) here if one bad row should stop everything
            }
        }
    }
    Ok(packages_map)
}


/// Marks a package as installed *by aptsync* (managed) in the database.
/// If the package already exists (e.g., as baseline), it updates the flag to true
/// and preserves the original install date. If it's new, it inserts with the current date.
pub fn mark_package_installed(db_path: &Path, name: &str) -> Result<()> {
    let conn = Connection::open(db_path)?;
    // Use INSERT OR REPLACE to handle both new and existing entries.
    // COALESCE preserves the original install_date if the package already exists,
    // otherwise it uses CURRENT_TIMESTAMP for new entries.
    conn.execute(
        "INSERT OR REPLACE INTO packages (name, installed_by_aptsync, install_date)
         VALUES (?1, 1, COALESCE((SELECT install_date FROM packages WHERE name = ?1), CURRENT_TIMESTAMP))",
        params![name],
    )?;
    Ok(())
}

/// Removes a package's tracking record from the database entirely.
pub fn remove_package_tracking(db_path: &Path, name: &str) -> Result<()> {
    let conn = Connection::open(db_path)?;
    let changes = conn.execute("DELETE FROM packages WHERE name = ?1", params![name])?;
    if changes == 0 {
         // This is not necessarily an error, could happen in edge cases. Log as warning.
         eprintln!("Warning: Attempted to remove tracking for '{}', but it was not found in the database.", name);
    }
    Ok(())
}

/// Populates the database with currently manually installed packages during the first run.
/// These packages are marked as baseline (`installed_by_aptsync = 0`).
/// Uses `INSERT OR IGNORE` to avoid errors if run multiple times somehow, though
/// the main app logic prevents this via `is_db_empty`.
pub fn first_run_populate(db_path: &Path) -> Result<()> {
    println!("Querying manually installed packages using 'apt-mark showmanual'...");
    let output = Command::new("apt-mark")
        .args(["showmanual"])
        .output()
        .map_err(|e| AptSyncError::Custom(format!("Failed to execute 'apt-mark showmanual': {}", e)))?;

    if !output.status.success() {
         // Log the specific error from apt-mark
         let stderr = String::from_utf8_lossy(&output.stderr);
         return Err(AptSyncError::Custom(format!("'apt-mark showmanual' failed: {}", stderr)));
    }

    let packages_stdout = String::from_utf8_lossy(&output.stdout);
    let mut conn = Connection::open(db_path)?; // Open connection once

    let mut count = 0;
    // Use a transaction for potentially large inserts
    let tx = conn.transaction()?;
    { // Scope for the prepared statement
        // Mark as baseline (installed_by_aptsync = 0). Use IGNORE for safety.
        // Set install_date to current time for these baseline packages.
        let mut stmt = tx.prepare("INSERT OR IGNORE INTO packages (name, installed_by_aptsync, install_date) VALUES (?1, 0, CURRENT_TIMESTAMP)")?;
        for package_name in packages_stdout.lines() {
            let package_name = package_name.trim();
            if !package_name.is_empty() {
                 // Add stricter check to avoid inserting garbage lines if apt-mark output is weird
                 if !package_name.contains(':') && !package_name.contains(' ') && package_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '+') {
                     let changes = stmt.execute(params![package_name])?;
                     if changes > 0 { // Only count newly inserted packages
                         count += 1;
                     }
                 } else {
                     eprintln!("Warning: Skipping potential non-package line from apt-mark: '{}'", package_name);
                 }
            }
        }
    } // Statement goes out of scope here
    tx.commit()?; // Commit the transaction

    println!("Added {} baseline packages from 'apt-mark showmanual' to the database.", count);
    Ok(())
}

/// Checks if the database contains any package records.
/// Returns true if the 'packages' table is empty, false otherwise.
pub fn is_db_empty(db_path: &Path) -> Result<bool> {
    let conn = Connection::open(db_path)?;
    // Query row with OptionalExtension to handle the case where the table might exist but be empty
    let count_opt: Option<i64> = conn.query_row(
        "SELECT COUNT(*) FROM packages",
        [],
        |row| row.get(0)
    ).optional()?; // Use optional() to return None if no rows

    match count_opt {
        Some(count) => Ok(count == 0),
        None => Ok(true), // No rows found means it's empty
    }
}
