use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;

/// Zakodowany runtime C (embedded w binarce kompilatora)
/// Uzytkownik nigdy nie widzi tych plikow — sa ekstrahowane do tmpdir
const RUNTIME_H: &str = include_str!("runtime_c/hl_runtime.h");
const RUNTIME_C: &str = include_str!("runtime_c/hl_runtime.c");

/// Skompiluj runtime C do pliku obiektowego
///
/// Kroki:
///   1. Ekstrahuj hl_runtime.h i hl_runtime.c do tmpdir
///   2. Kompiluj cc -O2 -c hl_runtime.c -o output
pub fn compile_runtime(output: &Path, verbose: bool) -> Result<()> {
    // Ekstrahuj pliki runtime do katalogu tymczasowego
    let tmp_dir = output.parent()
        .unwrap_or(Path::new("/tmp"));

    let h_path = tmp_dir.join("hl_runtime.h");
    let c_path = tmp_dir.join("hl_runtime.c");

    std::fs::write(&h_path, RUNTIME_H)
        .map_err(|e| anyhow::anyhow!("Write hl_runtime.h: {}", e))?;
    std::fs::write(&c_path, RUNTIME_C)
        .map_err(|e| anyhow::anyhow!("Write hl_runtime.c: {}", e))?;

    // Znajdz kompilator C
    let cc = find_c_compiler()?;

    if verbose {
        eprintln!("  CC: {} -O2 -c {} -o {}", cc, c_path.display(), output.display());
    }

    let status = Command::new(&cc)
        .args([
            "-O2",
            "-Wall",
            "-fPIC",
            "-fno-exceptions",      // eliminuje _Unwind_Resume / __gcc_personality_v0
            "-fno-unwind-tables",   // nie generuj tabel unwind (zbedne dla HL runtime)
            "-fno-asynchronous-unwind-tables",
            "-c",
            c_path.to_str().unwrap_or("hl_runtime.c"),
            "-o",
            output.to_str().unwrap_or("hl_runtime.o"),
        ])
        .status()
        .map_err(|e| anyhow::anyhow!("Nie mozna uruchomic {}: {}", cc, e))?;

    // Sprzatanie plikow tymczasowych
    let _ = std::fs::remove_file(&h_path);
    let _ = std::fs::remove_file(&c_path);

    if !status.success() {
        bail!("Kompilacja runtime C zakonczona bledem (kod: {})", status.code().unwrap_or(-1));
    }

    Ok(())
}

fn find_c_compiler() -> Result<String> {
    for cc in &["cc", "gcc", "clang"] {
        if which::which(cc).is_ok() {
            return Ok(cc.to_string());
        }
    }
    bail!("Nie znaleziono kompilatora C (cc/gcc/clang). Zainstaluj: sudo apt install gcc");
}
