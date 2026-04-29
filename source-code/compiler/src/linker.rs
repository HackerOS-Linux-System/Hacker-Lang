use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;
use crate::CompileMode;

pub fn link(hl_obj: &Path, rt_obj: &Path, output: &Path, mode: &CompileMode, verbose: bool) -> Result<()> {
    let cc = find_c_compiler()?;
    let output_str = output.to_str().unwrap_or("a.out");
    let hl_str     = hl_obj.to_str().unwrap_or("hl.o");
    let rt_str     = rt_obj.to_str().unwrap_or("rt.o");

    let mut args: Vec<String> = Vec::new();
    match mode {
        CompileMode::Binary | CompileMode::Bytecode => {
            args.push("-static".into());
            args.push("-o".into()); args.push(output_str.into());
            args.push(hl_str.into()); args.push(rt_str.into());
            args.push("-lgcc".into()); args.push("-lgcc_eh".into());
            args.push("-lc".into());
        }
        CompileMode::Shared => {
            args.push("-shared".into()); args.push("-fPIC".into());
            args.push("-o".into()); args.push(output_str.into());
            args.push(hl_str.into()); args.push(rt_str.into());
            args.push("-lc".into());
        }
    }

    if verbose { eprintln!("  LINK: {} {}", cc, args.join(" ")); }

    let status = Command::new(&cc).args(&args).status()
        .map_err(|e| anyhow::anyhow!("Nie mozna uruchomic {}: {}", cc, e))?;

    if !status.success() {
        return link_fallback(hl_obj, rt_obj, output, mode, &cc, verbose);
    }

    if matches!(mode, CompileMode::Binary | CompileMode::Bytecode) { set_executable(output); }
    Ok(())
}

fn link_fallback(hl_obj: &Path, rt_obj: &Path, output: &Path, mode: &CompileMode, cc: &str, verbose: bool) -> Result<()> {
    let output_str = output.to_str().unwrap_or("a.out");
    let hl_str     = hl_obj.to_str().unwrap_or("hl.o");
    let rt_str     = rt_obj.to_str().unwrap_or("rt.o");

    let mut args: Vec<String> = Vec::new();
    match mode {
        CompileMode::Binary | CompileMode::Bytecode => {
            args.push("-static".into());
            args.push("-o".into()); args.push(output_str.into());
            args.push(hl_str.into()); args.push(rt_str.into());
            args.push("-lgcc".into()); args.push("-lc".into());
        }
        CompileMode::Shared => {
            args.push("-shared".into()); args.push("-fPIC".into());
            args.push("-o".into()); args.push(output_str.into());
            args.push(hl_str.into()); args.push(rt_str.into());
            args.push("-lc".into());
        }
    }

    if verbose { eprintln!("  LINK (fallback): {} {}", cc, args.join(" ")); }

    let status = Command::new(cc).args(&args).status()
        .map_err(|e| anyhow::anyhow!("Nie mozna uruchomic {}: {}", cc, e))?;

    if !status.success() {
        if which::which("musl-gcc").is_ok() {
            return link_musl(hl_obj, rt_obj, output, mode, verbose);
        }
        bail!("Linkowanie zakonczone bledem.\nZainstaluj: sudo apt install gcc binutils libc6-dev");
    }

    if matches!(mode, CompileMode::Binary | CompileMode::Bytecode) { set_executable(output); }
    Ok(())
}

fn link_musl(hl_obj: &Path, rt_obj: &Path, output: &Path, mode: &CompileMode, verbose: bool) -> Result<()> {
    let output_str = output.to_str().unwrap_or("a.out");
    let hl_str     = hl_obj.to_str().unwrap_or("hl.o");
    let rt_str     = rt_obj.to_str().unwrap_or("rt.o");

    let args = match mode {
        CompileMode::Binary | CompileMode::Bytecode => vec!["-static", "-o", output_str, hl_str, rt_str],
        CompileMode::Shared => vec!["-shared", "-fPIC", "-o", output_str, hl_str, rt_str],
    };

    if verbose { eprintln!("  LINK (musl): musl-gcc {}", args.join(" ")); }

    let status = Command::new("musl-gcc").args(&args).status()
        .map_err(|e| anyhow::anyhow!("musl-gcc: {}", e))?;

    if !status.success() { bail!("Linkowanie przez musl-gcc takze zakonczone bledem."); }
    if matches!(mode, CompileMode::Binary | CompileMode::Bytecode) { set_executable(output); }
    Ok(())
}

fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        let _ = std::fs::set_permissions(path, perms);
    }
}

fn find_c_compiler() -> Result<String> {
    for cc in &["gcc", "cc", "clang"] {
        if which::which(cc).is_ok() { return Ok(cc.to_string()); }
    }
    bail!("Nie znaleziono kompilatora C. Zainstaluj: sudo apt install gcc");
}
