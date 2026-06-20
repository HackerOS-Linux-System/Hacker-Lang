use anyhow::Result;
use colored::Colorize;
use hl_compiler::{compile_to_cache, read_bc_file, HlModule};
use crate::interpreter::BytecodeInterpreter;
use std::path::Path;

/// Uruchom plik .hl — kompiluj do cache jeśli potrzeba, potem wykonaj przez JIT
pub fn run_hl_file(path: &Path, args: &[String]) -> Result<i32> {
    if !path.exists() {
        anyhow::bail!("Plik nie istnieje: {:?}", path);
    }

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "bc" => {
            // Bezpośrednie wykonanie .bc
            let module = read_bc_file(path)?;
            run_bc_module(&module, args)
        }
        _ => {
            // .hl → cache → .bc → JIT
            let source = std::fs::read_to_string(path)?;
            run_hl_source(&source, path, args)
        }
    }
}

/// Uruchom kod źródłowy HL — kompiluj do cache i wykonaj
pub fn run_hl_source(source: &str, source_path: &Path, args: &[String]) -> Result<i32> {
    tracing::debug!("run_hl_source: {:?}", source_path);

    // Kompiluj do cache (szybko jeśli cache hit)
    let bc_path = compile_to_cache(source, source_path)?;

    // Wczytaj .bc
    let module = read_bc_file(&bc_path)?;

    run_bc_module(&module, args)
}

/// Uruchom plik .bc
pub fn run_bc_file(path: &Path, args: &[String]) -> Result<i32> {
    let module = read_bc_file(path)?;
    run_bc_module(&module, args)
}

/// Uruchom załadowany moduł bytecode przez interpreter + JIT
pub fn run_bc_module(module: &HlModule, args: &[String]) -> Result<i32> {
    inject_args_to_env(args);

    let mut interp = BytecodeInterpreter::new(module);
    // HL_VERSION i inne zmienne bazowe inicjalizuje BytecodeInterpreter::init_hl_vars()
    // wywoływane wewnątrz interp.run()

    let exit_code = interp.run()?;
    Ok(exit_code)
}

fn inject_args_to_env(args: &[String]) {
    std::env::set_var("argc", args.len().to_string());
    for (i, arg) in args.iter().enumerate() {
        std::env::set_var(format!("arg{}", i), arg);
    }
}

/// Wypisz statystyki cache (dla `hl cache-info`)
pub fn print_cache_stats() {
    use hl_compiler::cache::{cache_dir, cache_list, CACHE_MAX_FILES};
    let dir = cache_dir();
    println!("{}", "=== Cache bytecode ===".bright_cyan().bold());
    println!("  Katalog: {}", dir.display().to_string().bright_white());
    println!("  Limit:   {} plików .bc", CACHE_MAX_FILES);
    println!();

    match cache_list() {
        Ok(entries) => {
            if entries.is_empty() {
                println!("  {}", "Cache jest pusty.".bright_black());
                return;
            }
            let total_size: u64 = entries.iter().map(|e| e.size).sum();
            println!("  Pliki: {}", entries.len().to_string().bright_white());
            println!("  Rozmiar łączny: {} KB",
                     (total_size / 1024).to_string().bright_yellow());
            println!();
            for entry in entries.iter().take(10) {
                let name = entry.path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?");
                println!("  {} ({} B)", name.bright_white(), entry.size);
            }
            if entries.len() > 10 {
                println!("  {} ... i {} więcej",
                         "".bright_black(),
                         entries.len() - 10);
            }
        }
        Err(e) => eprintln!("  Błąd odczytu cache: {}", e),
    }
}
