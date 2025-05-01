// src/app.rs
use crate::config;
use crate::db::{self, PackageInfo, get_all_tracked_packages, mark_package_installed, remove_package_tracking, is_db_empty, first_run_populate};
use crate::error::{AptSyncError, Result};
use crate::utils;
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Main application structure holding state and logic.
pub struct AptSync {
    db_path: PathBuf,
    dry_run: bool,
    skip_prompt: bool,
}

impl AptSync {
    pub fn new(dry_run: bool, skip_prompt: bool) -> Result<Self> {
        let home = utils::get_user_home()?;
        let config_dir = home.join(".config").join("aptsync");
        let db_path = config_dir.join("packages.db");
        fs::create_dir_all(&config_dir)?;
        Ok(Self {
            db_path,
            dry_run,
            skip_prompt,
        })
    }

    fn ensure_db_initialized(&self) -> Result<()> {
        if !self.db_path.exists() {
            println!("Database file not found at '{}'. Initializing...", self.db_path.display());
            db::init_db(&self.db_path)?;
            println!("Database structure initialized.");
        }
        Ok(())
    }

    pub fn init(&self) -> Result<()> {
        let config_path = Path::new("configuration.apt");
        if config_path.exists() {
            println!("'{}' already exists in the current directory.", config_path.display());
        } else {
            let config_content = r#"# AptSync Configuration File (configuration.apt)
# Add packages you want managed by aptsync, one per line.
# Lines starting with # are comments. Example:
# git
# vim
# Use @import ./path/to/another.apt to include other files.
"#;
            fs::write(&config_path, config_content)?;
            println!("Created '{}' in the current directory.", config_path.display());
        }
        self.ensure_db_initialized()?;
        println!("\nInitialization complete.");
        println!("1. Add desired packages to 'configuration.apt'.");
        println!("2. Run 'sudo aptsync update' to establish baseline (first time) and sync.");
        Ok(())
    }

    pub fn list_packages(&self, detailed: bool) -> Result<()> {
        self.ensure_db_initialized()?;

        let conn = rusqlite::Connection::open(&self.db_path)?;
        let mut stmt = if detailed {
            conn.prepare("SELECT name, install_date FROM packages WHERE installed_by_aptsync = 1 ORDER BY install_date DESC")?
        } else {
            conn.prepare("SELECT name FROM packages WHERE installed_by_aptsync = 1 ORDER BY name ASC")?
        };

        let mut count = 0;
        println!("Packages managed by aptsync (according to last sync):");
        println!("-------------------------------------------------------");

        if detailed {
            let packages_iter = stmt.query_map([], |row| {
                Ok(PackageInfo {
                    name: row.get(0)?,
                    install_date: row.get(1)?,
                })
            })?;
            for package_result in packages_iter {
                match package_result {
                    Ok(pkg) => {
                        println!("{} (installed on {})", pkg.name, pkg.install_date);
                        count += 1;
                    },
                    Err(e) => eprintln!("Error reading package details: {}", e),
                }
            }
        } else {
            let packages_iter = stmt.query_map([], |row| {
                row.get::<_, String>(0)
            })?;
            for package_result in packages_iter {
                 match package_result {
                    Ok(pkg_name) => {
                         println!("{}", pkg_name);
                         count += 1;
                    },
                    Err(e) => eprintln!("Error reading package name: {}", e),
                 }
            }
        }

        if count == 0 {
             println!("(No packages currently marked as managed by aptsync in the database)");
        }
        Ok(())
    }


    pub fn update(&self) -> Result<()> {
        self.ensure_db_initialized()?;
        utils::check_sudo()?;

        // --- Handle first run: Populate database with baseline and exit ---
        if is_db_empty(&self.db_path)? {
            println!("Database is empty. Populating baseline packages...");
            first_run_populate(&self.db_path)?;
            println!("Baseline established. Run 'sudo aptsync update' again to sync with configuration.");
            return Ok(());
        }

        // --- Get current system state, desired state, and DB state ---
        println!("Checking current manually installed packages ('apt-mark showmanual')...");
        let current_manual_packages = utils::get_manual_packages()?;
        println!("Found {} manually installed packages.", current_manual_packages.len());

        let config_path = Path::new("configuration.apt");
        if !config_path.exists() {
            return Err(AptSyncError::Custom(format!(
                "Configuration file '{}' not found.", config_path.display()
            )));
        }
        println!("Parsing configuration file '{}'...", config_path.display());
        let desired_packages_vec = config::parse_config_file(config_path)?;
        let desired_packages: HashSet<String> = desired_packages_vec.into_iter().collect();
        println!("Found {} packages listed in configuration.", desired_packages.len());

        println!("Loading package status from database...");
        let db_package_status = get_all_tracked_packages(&self.db_path)?;
        println!("Loaded status for {} packages from database.", db_package_status.len());

        // --- Calculate changes ---
        let to_install_vec: Vec<String> = desired_packages.iter().cloned().collect();
        let mut to_remove_vec: Vec<String> = Vec::new();
        for pkg_name in &current_manual_packages {
            if !desired_packages.contains(pkg_name) {
                match db_package_status.get(pkg_name) {
                    Some(true) | None => { // Managed (true) or Unknown (None) -> remove
                        to_remove_vec.push(pkg_name.clone());
                    }
                    Some(false) => { // Baseline (false) -> ignore
                    }
                }
            }
        }

        // --- Display proposed changes ---
        let install_candidates: Vec<_> = desired_packages.difference(&current_manual_packages).collect();
        let no_real_installs = install_candidates.is_empty() && desired_packages.iter().all(|p| current_manual_packages.contains(p));
        let no_changes = no_real_installs && to_remove_vec.is_empty();

        if no_changes {
             println!("\nSystem state matches configuration. No changes needed.");
             // Still run apt update below, but skip prompts/actions
        } else {
            println!("\nThe following changes will be made to align with '{}':", config_path.display());
        }

        // Display installs
        if !to_install_vec.is_empty() {
            println!("\nPackages to ensure are installed (from config):");
            if !install_candidates.is_empty() {
                 for package in &install_candidates {
                     println!("  + {}", package);
                 }
            }
            let already_installed_in_config: Vec<_> = desired_packages.intersection(&current_manual_packages).collect();
            if !already_installed_in_config.is_empty() && !install_candidates.is_empty() {
                 println!("  (Plus ensuring {} other configured packages are present)", already_installed_in_config.len());
            } else if !already_installed_in_config.is_empty() && install_candidates.is_empty() {
                 println!("  (All configured packages seem to be marked as manually installed already, will ensure state)");
            } else if already_installed_in_config.is_empty() && install_candidates.is_empty() {
                 println!("  (Configuration list is empty)");
            }
        } else {
             if to_remove_vec.is_empty() { // Only print if no removals either
                 println!("\nConfiguration is empty and no manually installed packages to remove.");
             } else {
                 println!("\nConfiguration is empty.");
             }
        }

        // Display removals
        if !to_remove_vec.is_empty() {
            println!("\nPackages to remove (manually installed, not in config, not baseline):");
            for package in &to_remove_vec {
                println!("  - {}", package);
            }
        }

        // --- Handle dry run ---
        if self.dry_run {
            println!("\n-- Dry run enabled. No changes will be applied to the system or database. --");
            return Ok(());
        }

        // --- Confirmation prompt ---
        // Check if there are actual installs OR removals planned
        // so check install_candidates for *new* installs.
        let installs_planned = !install_candidates.is_empty();
        let removals_planned = !to_remove_vec.is_empty();

        // Only prompt if there are actual changes to be made by apt
        if installs_planned || removals_planned {
            // Adjust warning message based on actions
            if removals_planned {
                println!("\nWARNING: This will install and/or REMOVE packages from your system!");
            } else { // Only installs planned
                println!("\nWARNING: This will install packages on your system.");
            }

            if !self.skip_prompt {
                print!("Do you want to continue? [y/N] "); // Simplified prompt
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if input.trim().to_lowercase() != "y" {
                    println!("Operation cancelled by user.");
                    return Ok(());
                }
            }
        } else if no_changes {
             // If no changes, skip apt actions but still run apt update
             println!("\nNo package changes required. Running package list update only.");
        } else {
             // This case means only ensuring already installed packages (no new installs, no removals)
             println!("\nProceeding to ensure configured packages are present (no new installs or removals)...");
        }


        // --- Apply Changes ---
        let mut errors_occurred = false;

        // 1. Update apt package list first (always run this)
        println!("\nUpdating package lists ('apt-get update')...");
        let update_status = Command::new("apt-get")
            .args(["update"])
            .status()
            .map_err(|e| AptSyncError::Custom(format!("Failed to execute 'apt-get update': {}", e)))?;

        if !update_status.success() {
            eprintln!("Warning: 'apt-get update' failed. Proceeding, but package information may be outdated.");
        }

        // Only proceed with install/remove if changes were planned
        if installs_planned || removals_planned || !no_changes { // Added !no_changes to ensure state even if no new installs/removals

            // 2. Perform installations (ensure desired state)
            let mut install_succeeded = true;
            if !to_install_vec.is_empty() { // Run install even if only ensuring existing packages
                let pkgs_to_install_str: Vec<&str> = to_install_vec.iter().map(|s| s.as_str()).collect();
                println!("Ensuring packages are installed: {}...", pkgs_to_install_str.join(" "));
                let install_status = Command::new("apt-get")
                    .arg("install")
                    .arg("-y")
                    .args(&pkgs_to_install_str)
                    .status()
                    .map_err(|e| AptSyncError::Custom(format!("Failed to execute 'apt-get install': {}", e)))?;

                if !install_status.success() {
                    eprintln!("Error: 'apt-get install' command failed for one or more packages.");
                    errors_occurred = true;
                    install_succeeded = false;
                }
            } else {
                // This case shouldn't be reached if installs_planned or removals_planned is true,
                // unless config is empty AND removals are planned.
                if !removals_planned {
                     println!("\nNo packages specified in configuration to install.");
                }
            }

            // 3. Perform removals (only if install step didn't have critical errors AND removals are planned)
            let mut remove_succeeded = true;
            if install_succeeded && removals_planned { // Check removals_planned explicitly
                let pkgs_to_remove_str: Vec<&str> = to_remove_vec.iter().map(|s| s.as_str()).collect();
                println!("Removing packages not in configuration: {}...", pkgs_to_remove_str.join(" "));
                let remove_status = Command::new("apt-get")
                    .arg("remove")
                    .arg("-y")
                    .args(&pkgs_to_remove_str)
                    .status()
                    .map_err(|e| AptSyncError::Custom(format!("Failed to execute 'apt-get remove': {}", e)))?;

                if !remove_status.success() {
                    eprintln!("Error: 'apt-get remove' command failed for one or more packages.");
                    errors_occurred = true;
                    remove_succeeded = false;
                }
            }

            // 4. Update Database State (only if corresponding apt command succeeded)
            println!("\nUpdating database state...");
            if install_succeeded {
                for pkg_name in &desired_packages {
                    if let Err(e) = mark_package_installed(&self.db_path, pkg_name) {
                         eprintln!("Error marking package '{}' as managed in database: {}", pkg_name, e);
                         errors_occurred = true;
                    }
                }
            } else {
                 println!("Skipping database update for installed packages due to apt install errors.");
            }

            if remove_succeeded && removals_planned { // Check removals_planned again
                 for pkg_name in &to_remove_vec {
                     if let Err(e) = remove_package_tracking(&self.db_path, pkg_name) {
                         eprintln!("Error removing tracking for package '{}' from database: {}", pkg_name, e);
                         errors_occurred = true;
                     }
                 }
            } else if !remove_succeeded && removals_planned { // Only print skip message if removals were attempted and failed
                 println!("Skipping database update for removed packages due to apt remove errors.");
            }
        }


        // --- Final Status ---
        if errors_occurred {
             println!("\nUpdate process completed with one or more errors.");
             return Err(AptSyncError::Custom("Update completed with errors.".to_string()));
        } else {
             println!("\nUpdate process completed successfully.");
        }

        Ok(())
    }
}
