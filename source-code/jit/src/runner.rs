use anyhow::Result;
use colored::Colorize;
use hl_compiler::{compile_to_cache, read_bc_file, HlModule};
use hl_core::{env::Env, Value};
use crate::interpreter::BytecodeInterpreter;
use std::path::Path;

/// Skrypty powyżej tego progu linii używają AST executor zamiast BC serializacji.
/// Podniesione z 300 → 2000, żeby duże skrypty jak bit.hl nie miały problemu z wstrzykiwaniem args.
/// Wszystkie skrypty uruchamiane przez `hl run` używają AST executor (bez BC compile).
/// BC compile jest dostępny przez `hl compile` + `hl run script.bc`.
/// To eliminuje 8s+ timeout dla dużych skryptów jak bit.hl.
const BC_LINE_THRESHOLD: usize = 0;

/// Uruchom plik .hl — kompiluj do cache jeśli potrzeba, potem wykonaj przez JIT
pub fn run_hl_file(path: &Path, args: &[String]) -> Result<i32> {
    if !path.exists() {
        anyhow::bail!("Plik nie istnieje: {:?}", path);
    }

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "bc" => {
            let module = read_bc_file(path)?;
            run_bc_module(&module, args)
        }
        _ => {
            let source = std::fs::read_to_string(path)?;
            run_hl_source(&source, path, args)
        }
    }
}

/// Uruchom kod źródłowy HL
pub fn run_hl_source(source: &str, source_path: &Path, args: &[String]) -> Result<i32> {
    tracing::debug!("run_hl_source: {:?}", source_path);

    // Zawsze ustawiamy zmienne procesu (dla kompatybilności z BytecodeInterpreter)
    inject_args_to_env(args);

    let line_count = source.lines().count();

    if line_count > BC_LINE_THRESHOLD {
        tracing::debug!(
            "Duży plik ({} linii > {}), używam AST executor",
            line_count, BC_LINE_THRESHOLD
        );
        return run_via_ast(source, source_path, args);
    }

    // Mały plik — kompiluj do .bc z timeoutem
    match compile_with_timeout(source, source_path, std::time::Duration::from_secs(30)) {
        Ok(bc_path) => {
            let module = read_bc_file(&bc_path)?;
            run_bc_module(&module, args)
        }
        Err(e) => {
            tracing::warn!("BC compile failed ({}), fallback do AST executor", e);
            run_via_ast(source, source_path, args)
        }
    }
}

/// Wykonaj przez AST executor (tree-walk) — szybki, bez serializacji
///
/// FIX: przyjmuje args i wstrzykuje je bezpośrednio do Env.vars
/// Wcześniej: inject_args_to_env ustawiało tylko zmienne procesu, a run_via_ast
/// tworzyło nowy pusty Env → @arg0, @argc były niedostępne przez env.vars
/// (działało przez std::env::var fallback, ale argc z poprzednich wywołań mogło być złe).
fn run_via_ast(source: &str, _source_path: &Path, args: &[String]) -> Result<i32> {
    use hl_core::run_source;
    let mut env = Env::new();

    // Wstrzyknij argumenty bezpośrednio do Env — niezawodne, nie zależy od process env
    env.set_var("argc", Value::Number(args.len() as f64));
    for (i, arg) in args.iter().enumerate() {
        env.set_var(&format!("arg{}", i), Value::String(arg.clone()));
    }

    match run_source(source, &mut env) {
        Ok(result) => Ok(result.exit_code),
        Err(e)     => Err(e),
    }
}

fn compile_with_timeout(
    source:      &str,
    source_path: &Path,
    timeout:     std::time::Duration,
) -> Result<std::path::PathBuf> {
    use std::time::Instant;

    let t0 = Instant::now();
    tracing::debug!("Kompilacja BC: {:?}", source_path);

    let source_owned = source.to_string();
    let path_owned   = source_path.to_path_buf();

    let handle = std::thread::spawn(move || {
        compile_to_cache(&source_owned, &path_owned)
    });

    let result = loop {
        if handle.is_finished() {
            break handle.join()
                .map_err(|_| anyhow::anyhow!("wątek kompilacji BC spanikował"))?;
        }
        if t0.elapsed() > timeout {
            anyhow::bail!(
                "Kompilacja BC przekroczyła limit czasu ({:.0}s)",
                timeout.as_secs_f64()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    };

    let elapsed = t0.elapsed();
    tracing::debug!("Kompilacja BC ukończona: {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    result
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
    let exit_code = interp.run()?;
    Ok(exit_code)
}

/// Ustaw zmienne procesu dla BytecodeInterpreter (który czyta std::env::var)
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
