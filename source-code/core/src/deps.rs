use anyhow::{Context, Result};
use std::process::Command;
use tracing::{info, warn};

pub fn is_installed(name: &str) -> bool { which::which(name).is_ok() }

pub fn install_package(name: &str) -> Result<bool> {
    if which::which("apt-get").is_ok() {
        info!("Installing '{}' via apt-get...", name);
        let s = Command::new("sudo").args(["apt-get", "-y", "install", name]).status().context("Failed to run sudo apt-get")?;
        if s.success() { return Ok(true); }
    }
    if which::which("lpm").is_ok() {
        let s = Command::new("sudo").args(["lpm", "install", name]).status().context("Failed lpm")?;
        if s.success() { return Ok(true); }
    }
    warn!("Could not install '{}'", name);
    Ok(false)
}

pub fn resolve_dependency(dep_spec: &str) -> Result<DependencyResult> {
    let bin_name = dep_spec.trim().splitn(2, ' ').next().unwrap_or(dep_spec.trim());
    if is_installed(bin_name) {
        return Ok(DependencyResult::AlreadyInstalled(bin_name.to_string()));
    }
    eprintln!("\x1b[33m[hl dep]\x1b[0m '{}' not found. Attempting installation...", bin_name);
    match install_package(bin_name) {
        Ok(true)  => { eprintln!("\x1b[32m[hl dep]\x1b[0m '{}' installed.", bin_name); Ok(DependencyResult::Installed(bin_name.to_string())) }
        Ok(false) => { eprintln!("\x1b[31m[hl dep]\x1b[0m Failed to install '{}'.", bin_name); Ok(DependencyResult::Failed(bin_name.to_string())) }
        Err(e)    => Err(e),
    }
}

#[derive(Debug)]
pub enum DependencyResult { AlreadyInstalled(String), Installed(String), Failed(String) }
impl DependencyResult {
    pub fn is_available(&self) -> bool { matches!(self, DependencyResult::AlreadyInstalled(_) | DependencyResult::Installed(_)) }
}
