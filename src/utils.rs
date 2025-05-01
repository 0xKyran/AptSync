// src/utils.rs
use crate::error::{AptSyncError, Result};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use std::env;

/// Checks if the current user has root privileges (UID 0).
pub fn check_sudo() -> Result<()> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| AptSyncError::Custom(format!("Failed to execute 'id -u': {}", e)))?;

    if !output.status.success() {
         return Err(AptSyncError::Custom(format!("'id -u' command failed: {}", String::from_utf8_lossy(&output.stderr))));
    }

    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if uid != "0" {
        return Err(AptSyncError::Custom(
            "This command requires root privileges. Please run with sudo.".to_string(),
        ));
    }
    Ok(())
}

/// Determines the appropriate home directory, considering SUDO_USER.
pub fn get_user_home() -> Result<PathBuf> {
    if let Ok(sudo_user) = env::var("SUDO_USER") {
         if !sudo_user.is_empty() && sudo_user != "root" {
            let output = Command::new("getent")
                .args(["passwd", &sudo_user])
                .output()
                .map_err(|e| AptSyncError::Custom(format!("Failed to get user info for {}: {}", sudo_user, e)))?;

            if output.status.success() {
                let passwd_entry = String::from_utf8_lossy(&output.stdout);
                if let Some(home_dir) = passwd_entry.split(':').nth(5) {
                     if !home_dir.is_empty() {
                          return Ok(PathBuf::from(home_dir));
                     }
                }
            } else {
                 eprintln!("Warning: Could not get home directory for SUDO_USER '{}'. Falling back.", sudo_user);
            }
        }
    }
    dirs::home_dir()
        .ok_or_else(|| AptSyncError::Custom("Could not find home directory".to_string()))
}

/// Retrieves the set of packages currently marked as manually installed. Requires sudo context.
pub fn get_manual_packages() -> Result<HashSet<String>> {
    // Sudo check should happen in the calling function (app::update)
    let output = Command::new("apt-mark")
        .args(["showmanual"])
        .output()
        .map_err(|e| AptSyncError::Custom(format!("Failed to execute 'apt-mark showmanual': {}", e)))?;

    if !output.status.success() {
         return Err(AptSyncError::Custom(format!("'apt-mark showmanual' failed: {}", String::from_utf8_lossy(&output.stderr))));
    }

    let packages_stdout = String::from_utf8_lossy(&output.stdout);
    let mut manual_packages = HashSet::new();

    for package_name in packages_stdout.lines() {
        let package_name = package_name.trim();
        if !package_name.is_empty() {
            // Basic check: Avoid inserting things that look like warnings/errors
            if !package_name.contains(':') && !package_name.contains(' ') {
                manual_packages.insert(package_name.to_string());
            } else {
                eprintln!("Warning: Skipping potential non-package line from apt-mark: '{}'", package_name);
            }
        }
    }
    Ok(manual_packages)
}
