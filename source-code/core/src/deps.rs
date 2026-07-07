use anyhow::{Context, Result};
use std::process::Command;
use tracing::{info, warn};

pub fn is_installed(name: &str) -> bool { which::which(name).is_ok() }

/// Zainstaluj pakiet przez apt-get lub lpm.
/// `apt_name` — co zainstalować (może się różnić od nazwy binارki).
pub fn install_package(apt_name: &str) -> Result<bool> {
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

/// Rozwiąż zależność narzędzia:
///   bin_name   — nazwa binarki do sprawdzenia (np. "ninja")
///   apt_package — opcjonalny pakiet apt (np. "ninja-build"); jeśli None → używa bin_name
///
/// Przykłady składni w .hl:
///   // curl              → bin_name="curl", apt_package=None   → apt install curl
///   // ninja [ninja-build] → bin_name="ninja", apt_package=Some("ninja-build") → apt install ninja-build
///   // python3 [python3] → jawne (oba nazwy takie same)
pub fn resolve_dependency(bin_name: &str, apt_package: Option<&str>) -> Result<DependencyResult> {
    let bin = bin_name.trim();
    
    // Binarki już zainstalowana → OK bez instalacji
    if is_installed(bin) {
        return Ok(DependencyResult::AlreadyInstalled(bin.to_string()));
    }

    // Wybierz nazwę pakietu apt: jawna [pakiet] lub fallback = nazwa binarki
    let pkg = apt_package.unwrap_or(bin);

    eprintln!(
        "\x1b[33m[hl dep]\x1b[0m '{bin}' nie znalezione. \
        Próbuję: apt install {pkg}..."
    );

    match install_package(pkg) {
        Ok(true) => {
            // Sprawdź ponownie czy binarka teraz dostępna
            if is_installed(bin) {
                eprintln!("\x1b[32m[hl dep]\x1b[0m '{bin}' zainstalowane ({pkg}).");
                Ok(DependencyResult::Installed(bin.to_string()))
            } else {
                // Pakiet zainstalowany ale binarka wciąż nie widoczna (np. inna nazwa)
                eprintln!(
                    "\x1b[33m[hl dep]\x1b[0m Pakiet '{pkg}' zainstalowany, \
                    ale binarka '{bin}' nadal nie widoczna. \
                    Może wymaga innej ścieżki lub restartu powłoki."
                );
                Ok(DependencyResult::Installed(bin.to_string()))
            }
        }
        Ok(false) => {
            eprintln!("\x1b[31m[hl dep]\x1b[0m Nie udało się zainstalować '{pkg}'.");
            Ok(DependencyResult::Failed(bin.to_string()))
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
