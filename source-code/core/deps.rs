use anyhow::{Context, Result};
use std::process::Command;
use tracing::{info, warn};

/// Check if a binary is available on PATH
pub fn is_installed(name: &str) -> bool {
    which::which(name).is_ok()
}

/// Try to install a package using available package manager
pub fn install_package(name: &str) -> Result<bool> {
    // Try apt first (Debian/HackerOS)
    if which::which("apt-get").is_ok() {
        info!("Installing '{}' via apt-get...", name);
        let status = Command::new("apt-get")
        .args(["-y", "install", name])
        .status()
        .context("Failed to run apt-get")?;
        if status.success() {
            return Ok(true);
        }
        // Try with sudo
        let status = Command::new("sudo")
        .args(["apt-get", "-y", "install", name])
        .status()
        .context("Failed to run sudo apt-get")?;
        if status.success() {
            return Ok(true);
        }
    }

    // Fallback: try lpm (HackerOS package manager)
    if which::which("lpm").is_ok() {
        info!("Installing '{}' via lpm...", name);
        let status = Command::new("sudo")
        .args(["lpm", "install", name])
        .status()
        .context("Failed to run sudo lpm install")?;
        if status.success() {
            return Ok(true);
        }
    }

    warn!("Could not install '{}': no suitable package manager found", name);
    Ok(false)
}

/// Resolve a dependency: check if installed, install if not
pub fn resolve_dependency(dep_spec: &str) -> Result<DependencyResult> {
    let dep_spec = dep_spec.trim();

    // Parse "name [version_constraint]" - for now just use the first word as the binary
    let parts: Vec<&str> = dep_spec.splitn(2, ' ').collect();
    let bin_name = parts[0].trim();

    if is_installed(bin_name) {
        info!("Dependency '{}' is already installed.", bin_name);
        return Ok(DependencyResult::AlreadyInstalled(bin_name.to_string()));
    }

    eprintln!(
        "\x1b[33m[hl dep]\x1b[0m '{}' not found. Attempting installation...",
        bin_name
    );

    match install_package(bin_name) {
        Ok(true) => {
            eprintln!("\x1b[32m[hl dep]\x1b[0m '{}' installed successfully.", bin_name);
            Ok(DependencyResult::Installed(bin_name.to_string()))
        }
        Ok(false) => {
            eprintln!(
                "\x1b[31m[hl dep]\x1b[0m Failed to install '{}'. Please install it manually.",
                bin_name
            );
            Ok(DependencyResult::Failed(bin_name.to_string()))
        }
        Err(e) => Err(e),
    }
}

#[derive(Debug)]
pub enum DependencyResult {
    AlreadyInstalled(String),
    Installed(String),
    Failed(String),
}

impl DependencyResult {
    pub fn is_available(&self) -> bool {
        matches!(self, DependencyResult::AlreadyInstalled(_) | DependencyResult::Installed(_))
    }
}
