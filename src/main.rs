// src/main.rs
mod app;
mod config;
mod db;
mod error;
mod utils; 

use crate::app::AptSync;
use clap::{CommandFactory, Parser, Subcommand};
use std::process::exit;

// --- CLI Definition ---
#[derive(Parser)]
#[command(author, version, about = "Declarative package manager for APT", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Perform a dry run without making any changes
    #[arg(long)]
    dry_run: bool,

    /// Skip confirmation prompts
    #[arg(short = 'y', long)]
    yes: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Update system packages based on configuration.apt
    Update,
    /// List packages installed and managed by aptsync
    List {
        /// Show all details including installation date
        #[arg(short, long)]
        detailed: bool,
    },
    /// Initialize config file and database structure (no sudo needed)
    Init,
}

// --- Main Entry Point ---
fn main() {
    let cli = Cli::parse();

    // Show help and exit if no command is provided
    if cli.command.is_none() {
        let mut cmd = Cli::command(); // Get a mutable command instance
        if let Err(e) = cmd.print_help() {
             eprintln!("Error printing help: {}", e);
             // Exit even if help fails to print
        }
        exit(0); // Exit successfully after showing help (or attempting to)
    }

    // Create AptSync instance (initialization logic is now in app::new)
    let apt_sync = match AptSync::new(cli.dry_run, cli.yes) {
        Ok(instance) => instance,
        Err(e) => {
            eprintln!("Error initializing AptSync: {}", e);
            exit(1); // Exit with error code
        }
    };

    // Execute the chosen command (logic is now in app methods)
    let command_result = match cli.command {
        Some(Commands::Update) => apt_sync.update(),
        Some(Commands::List { detailed }) => apt_sync.list_packages(detailed),
        Some(Commands::Init) => apt_sync.init(),
        None => unreachable!(), // Already handled the None case above
    };

    // Handle potential errors from command execution
    if let Err(e) = command_result {
        eprintln!("\nError: {}", e);
        exit(1); // Exit with error code
    }

    exit(0); // Explicitly exit with success code
}
