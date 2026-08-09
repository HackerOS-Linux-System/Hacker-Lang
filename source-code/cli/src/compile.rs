use anyhow::Result;
use colored::Colorize;
use std::path::Path;

pub fn cmd_compile(file: &Path, output: Option<&Path>) -> Result<()> {
    if !file.exists() {
        eprintln!("{} Plik nie istnieje: {}", "BŁĄD".red().bold(), file.display());
        std::process::exit(1);
    }

    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "hl" => {
            eprintln!("{} {} → .bc",
                      "hl compile:".bright_magenta().bold(),
                      file.display().to_string().bright_white());

            let t0 = std::time::Instant::now();
            match hl_compiler::compile_hl_to_bc(file, output) {
                Ok(bc_path) => {
                    let elapsed = t0.elapsed();
                    println!("{} {} ({:.1}ms)",
                             "✓".green().bold(),
                             bc_path.display().to_string().bright_white(),
                             elapsed.as_secs_f64() * 1000.0);
                }
                Err(e) => {
                    eprintln!("{} {}", "BŁĄD kompilacji:".red().bold(), e);
                    std::process::exit(1);
                }
            }
        }
        "bc" => {
            eprintln!("{} Kompilacja .bc → ELF nie jest jeszcze dostępna w gen 2.",
                      "hl compile:".bright_magenta().bold());
            eprintln!("  Użyj {} aby uruchomić bytecode.",
                      "hl run plik.bc".bright_cyan());
            std::process::exit(1);
        }
        other => {
            eprintln!("{} Nieznane rozszerzenie: .{}", "BŁĄD".red().bold(), other);
            std::process::exit(1);
        }
    }

    Ok(())
}
