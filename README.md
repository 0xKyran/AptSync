# aptsync

A simple, declarative package manager for APT systems (Debian/Ubuntu and derivatives) written in Rust. Manage manually installed packages using a straightforward configuration file.

## Motivation

Keeping track of manually installed packages across different machines or after a reinstall can be tedious. `aptsync` aims to simplify this by allowing you to define your desired set of manually installed packages in a plain text file (`configuration.apt`). It then works to ensure your system's manually installed packages match this definition.

## Features

*   **Declarative:** Define your desired packages in `configuration.apt`.
*   **Simple Configuration:** Uses a plain text file with one package per line and support for comments (`#`).
*   **Modular Configuration:** Supports importing other configuration files using `@import ./path/to/file.apt`.
*   **Baseline Aware:** On the first run, it records your existing manually installed packages as a baseline. These baseline packages won't be removed unless you explicitly add them to your `configuration.apt` and later remove them.
*   **Syncing:**
    *   Installs packages listed in the configuration that aren't currently installed.
    *   Removes packages that were manually installed *after* the initial baseline was established and are *not* listed in the configuration.
*   **Tracking:** Uses a local SQLite database (`~/.config/aptsync/packages.db`) to distinguish baseline packages from those actively managed by the configuration.
*   **Commands:** `init`, `update`, `list`.
*   **Safety:** Includes `--dry-run` to preview changes and prompts for confirmation before modifying the system (can be skipped with `-y`/`--yes`).

## Installation

### Prerequisites

*   **Rust Toolchain:** Install via [rustup](https://rustup.rs/).
*   **APT Tools:** `apt-get` and `apt-mark` (standard on Debian/Ubuntu).
*   **Build Essentials:** You might need `build-essential` or equivalent C compiler tools for `rusqlite`.
    ```bash
    sudo apt install build-essential
    ```

### Building from Source

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/0xKyran/AptSync.git
    cd aptsync
    ```
2.  **Build the release binary:**
    ```bash
    cargo build --release
    ```
3.  **Copy the binary to your PATH:**
    ```bash
    sudo cp target/release/aptsync /usr/local/bin/
    ```

## Usage

### 1. Initialize

Run this in the directory where you want to keep your main `configuration.apt` file.

```bash
aptsync init
```

This will:
*   Create a sample `configuration.apt` file in the current directory if it doesn't exist.
*   Ensure the database structure exists at `~/.config/aptsync/packages.db`.

### 2. Configure

Edit the `configuration.apt` file and add the packages you want `aptsync` to manage, one per line.


### 3. First Update (Establish Baseline)

Run the update command with `sudo`. The *very first time* you run this, it will populate the database with your system's current manually installed packages as the baseline.

```bash
sudo aptsync update
```

It will print a message indicating the baseline has been established and ask you to run the command again to perform the actual sync against your configuration file.

### 4. Subsequent Updates (Sync)

Run `update` with `sudo` anytime you want to synchronize your system with your `configuration.apt`.

```bash
sudo aptsync update
```

This will:
*   Read `configuration.apt`.
*   Check current manually installed packages (`apt-mark showmanual`).
*   Check the database (`~/.config/aptsync/packages.db`).
*   Propose installing packages from the config that are missing.
*   Propose removing packages that are manually installed, *not* in the config, and *not* part of the initial baseline.
*   Prompt for confirmation (unless `-y` is used).
*   Execute `apt-get install` and `apt-get remove` as needed.
*   Update the database to mark configured packages as managed.

### 5. List Managed Packages

To see which packages `aptsync` considers actively managed (i.e., were present in the config during the last successful sync):

```bash
aptsync list
```

For more details, including the date they were added/marked as managed:

```bash
aptsync list --detailed
```

### Options

*   `--help`: Show help information.
*   `--dry-run`: Show the changes that *would* be made without actually running `apt-get` or modifying the database.
*   `-y`, `--yes`: Skip the confirmation prompt before installing/removing packages. Use with caution!

## How it Works (Simplified)

1.  **`init`**: Creates `configuration.apt` and the database schema.
2.  **First `update`**: Runs `apt-mark showmanual`, stores these packages in the DB marked as "baseline" (`installed_by_aptsync=false`), then exits.
3.  **Subsequent `update`**:
    *   Gets desired packages from `configuration.apt`.
    *   Gets current manual packages from `apt-mark showmanual`.
    *   Gets package status (baseline/managed) from the DB.
    *   **Installs**: Any package in the config. `apt install` handles already-installed ones.
    *   **Removes**: Any package found by `apt-mark showmanual` that is **NOT** in the config **AND** is **NOT** marked as "baseline" in the DB.
    *   **Updates DB**: Marks all packages from the config as "managed" (`installed_by_aptsync=true`) and removes entries for packages that were removed from the system.
