use anyhow::{Context, Result};
use std::process::Command;
use tracing::{info, warn};

pub fn is_installed(name: &str) -> bool { which::which(name).is_ok() }

/// Zainstaluj pakiet przez apt-get lub lpm.
fn install_package(apt_name: &str) -> Result<bool> {
    if which::which("apt-get").is_ok() {
        info!("Installing '{}' via apt-get...", apt_name);
        let s = Command::new("sudo")
            .args(["apt-get", "-y", "install", apt_name])
            .status()
            .context("Failed to run sudo apt-get")?;
        if s.success() { return Ok(true); }
    }
    if which::which("lpm").is_ok() {
        let s = Command::new("sudo")
            .args(["lpm", "install", apt_name])
            .status()
            .context("Failed lpm")?;
        if s.success() { return Ok(true); }
    }
    warn!("Could not install '{}'", apt_name);
    Ok(false)
}

/// Rozwiąż zależność narzędzia.
///
/// `auto_install` — czy automatycznie instalować przez apt.
///   false (domyślnie) → tylko sprawdź i ostrzeż jeśli brak
///   true  (flaga --install-deps) → apt install jeśli brak
///
/// Składnia .hl:
///   // curl              → apt install curl
///   // ninja [ninja-build] → apt install ninja-build
pub fn resolve_dependency(bin_name: &str, apt_package: Option<&str>, auto_install: bool) -> Result<DependencyResult> {
    let bin = bin_name.trim();

    if is_installed(bin) {
        return Ok(DependencyResult::AlreadyInstalled(bin.to_string()));
    }

    let pkg = apt_package.unwrap_or(bin);

    if !auto_install {
        // Tryb domyślny: tylko ostrzeżenie, nie blokuje wykonania
        eprintln!(
            "\x1b[33m[hl dep]\x1b[0m brak '{}'  (pakiet: {})  →  apt install {}",
            bin, pkg, pkg
        );
        return Ok(DependencyResult::Missing(bin.to_string()));
    }

    // Tryb auto-install (--install-deps)
    eprintln!(
        "\x1b[33m[hl dep]\x1b[0m '{bin}' nie znalezione — instaluję: apt install {pkg}..."
    );

    match install_package(pkg) {
        Ok(true) => {
            if is_installed(bin) {
                eprintln!("\x1b[32m[hl dep]\x1b[0m '{bin}' zainstalowane.");
                Ok(DependencyResult::Installed(bin.to_string()))
            } else {
                eprintln!(
                    "\x1b[33m[hl dep]\x1b[0m pakiet '{pkg}' zainstalowany, \
                    ale binarka '{bin}' nadal nie widoczna."
                );
                Ok(DependencyResult::Installed(bin.to_string()))
            }
        }
        Ok(false) => {
            eprintln!("\x1b[31m[hl dep]\x1b[0m nie udało się zainstalować '{pkg}'.");
            Ok(DependencyResult::Failed(bin.to_string()))
        }
        Err(e) => Err(e),
    }
}

#[derive(Debug)]
pub enum DependencyResult {
    AlreadyInstalled(String),
    Installed(String),
    Missing(String),   // nie ma, ale auto_install=false → nie blokuje
    Failed(String),
}

impl DependencyResult {
    pub fn is_available(&self) -> bool {
        matches!(self,
            DependencyResult::AlreadyInstalled(_)
            | DependencyResult::Installed(_)
            | DependencyResult::Missing(_)  // Missing nie blokuje wykonania
        )
    }
}
