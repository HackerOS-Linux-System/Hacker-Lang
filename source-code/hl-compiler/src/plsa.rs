use crate::ast::AnalysisResult;
use crate::paths::get_plsa_path;
use colored::*;
use std::process::{Command, exit};

/// Uruchamia hl-plsa i zwraca sparsowany AST.
pub fn run_plsa(file: &str, verbose: bool) -> AnalysisResult {
    let plsa = get_plsa_path();

    if verbose {
        eprintln!("{} Analizuję: {}", "[*]".green(), file);
    }

    let out = Command::new(&plsa)
    .arg(file)
    .arg("--json")
    .output()
    .unwrap_or_else(|e| {
        eprintln!("{} Nie można uruchomić hl-plsa: {}", "[x]".red(), e);
        exit(1);
    });

    if !out.status.success() {
        eprintln!(
            "{} hl-plsa błąd:\n{}",
            "[x]".red(),
                  String::from_utf8_lossy(&out.stderr)
        );
        exit(1);
    }

    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        let preview = &out.stdout[..out.stdout.len().min(512)];
        eprintln!(
            "{} JSON z PLSA nieprawidłowy: {}\n{}",
            "[x]".red(),
                  e,
                  String::from_utf8_lossy(preview)
        );
        exit(1);
    })
}
