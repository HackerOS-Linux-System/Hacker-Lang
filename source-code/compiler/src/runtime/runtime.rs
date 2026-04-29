use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;

const RUNTIME_H: &str = include_str!("runtime_c/hl_runtime.h");
const RUNTIME_C: &str = include_str!("runtime_c/hl_runtime.c");

pub fn compile_runtime(output: &Path, verbose: bool) -> Result<()> {
    let tmp_dir = output.parent().unwrap_or(Path::new("/tmp"));
    let h_path = tmp_dir.join("hl_runtime.h");
    let c_path = tmp_dir.join("hl_runtime.c");

    std::fs::write(&h_path, RUNTIME_H)
        .map_err(|e| anyhow::anyhow!("Write hl_runtime.h: {}", e))?;
    std::fs::write(&c_path, RUNTIME_C)
        .map_err(|e| anyhow::anyhow!("Write hl_runtime.c: {}", e))?;

    let cc = find_c_compiler()?;

    if verbose {
        eprintln!("  CC: {} -O2 -c {} -o {}", cc, c_path.display(), output.display());
    }

    let status = Command::new(&cc)
        .args([
            "-O2", "-Wall", "-fPIC",
            "-fno-exceptions",
            "-fno-unwind-tables",
            "-fno-asynchronous-unwind-tables",
            "-c",
            c_path.to_str().unwrap_or("hl_runtime.c"),
            "-o",
            output.to_str().unwrap_or("hl_runtime.o"),
        ])
        .status()
        .map_err(|e| anyhow::anyhow!("Nie mozna uruchomic {}: {}", cc, e))?;

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
    bail!("Nie znaleziono kompilatora C. Zainstaluj: sudo apt install gcc");
}
