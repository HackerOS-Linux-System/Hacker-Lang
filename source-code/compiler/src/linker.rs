use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;
use crate::CompileMode;

/// Zlinkuj obiekt HL + runtime C do binarki lub .so
///
/// UWAGA: Zawsze uzywamy gcc/clang jako driver linkera, NIGDY raw `ld`.
/// Raw `ld` z `-static -lc` wymaga recznego podania libgcc_eh, crt*.o, itp.
/// GCC jako driver automatycznie dodaje wszystkie wymagane obiekty CRT.
pub fn link(
    hl_obj:  &Path,
    rt_obj:  &Path,
    output:  &Path,
    mode:    &CompileMode,
    verbose: bool,
) -> Result<()> {
    // Zawsze uzywaj gcc/clang — nigdy raw ld bezposrednio
    let cc = find_c_compiler()?;

    let output_str = output.to_str().unwrap_or("a.out");
    let hl_str     = hl_obj.to_str().unwrap_or("hl.o");
    let rt_str     = rt_obj.to_str().unwrap_or("rt.o");

    let mut args: Vec<String> = Vec::new();

    match mode {
        CompileMode::Binary => {
            // Statyczna binarka przez gcc -static
            // gcc automatycznie dodaje crt1.o, crti.o, crtn.o, libgcc_eh, itp.
            args.push("-static".into());
            args.push("-o".into());
            args.push(output_str.into());
            args.push(hl_str.into());
            args.push(rt_str.into());
            // Dodaj libgcc dla _Unwind_Resume i __gcc_personality_v0
            args.push("-lgcc".into());
            args.push("-lgcc_eh".into());
            // lc na koncu
            args.push("-lc".into());
        }
        CompileMode::Shared => {
            args.push("-shared".into());
            args.push("-fPIC".into());
            args.push("-o".into());
            args.push(output_str.into());
            args.push(hl_str.into());
            args.push(rt_str.into());
            args.push("-lc".into());
        }
    }

    if verbose {
        eprintln!("  LINK: {} {}", cc, args.join(" "));
    }

    let status = Command::new(&cc)
        .args(&args)
        .status()
        .map_err(|e| anyhow::anyhow!("Nie mozna uruchomic {}: {}", cc, e))?;

    if !status.success() {
        // Fallback: sprobuj bez -lgcc_eh (starsze systemy)
        return link_fallback(hl_obj, rt_obj, output, mode, &cc, verbose);
    }

    if *mode == CompileMode::Binary {
        set_executable(output);
    }

    Ok(())
}

/// Fallback — bez -lgcc_eh (kompatybilnosc ze starszymi systemami i musl)
fn link_fallback(
    hl_obj:  &Path,
    rt_obj:  &Path,
    output:  &Path,
    mode:    &CompileMode,
    cc:      &str,
    verbose: bool,
) -> Result<()> {
    let output_str = output.to_str().unwrap_or("a.out");
    let hl_str     = hl_obj.to_str().unwrap_or("hl.o");
    let rt_str     = rt_obj.to_str().unwrap_or("rt.o");

    let mut args: Vec<String> = Vec::new();

    match mode {
        CompileMode::Binary => {
            args.push("-static".into());
            args.push("-o".into());
            args.push(output_str.into());
            args.push(hl_str.into());
            args.push(rt_str.into());
            // Bez -lgcc_eh — probuj z samym -lgcc
            args.push("-lgcc".into());
            args.push("-lc".into());
        }
        CompileMode::Shared => {
            args.push("-shared".into());
            args.push("-fPIC".into());
            args.push("-o".into());
            args.push(output_str.into());
            args.push(hl_str.into());
            args.push(rt_str.into());
            args.push("-lc".into());
        }
    }

    if verbose {
        eprintln!("  LINK (fallback): {} {}", cc, args.join(" "));
    }

    let status = Command::new(cc)
        .args(&args)
        .status()
        .map_err(|e| anyhow::anyhow!("Nie mozna uruchomic {}: {}", cc, e))?;

    if !status.success() {
        // Ostatni fallback — musl-gcc jesli dostepny
        if which::which("musl-gcc").is_ok() {
            return link_musl(hl_obj, rt_obj, output, mode, verbose);
        }
        bail!(
            "Linkowanie zakonczone bledem.\n\
Zainstaluj wymagane narzedzia:\n\
  sudo apt install gcc binutils libc6-dev\n\
Albo uzyj musl dla pelnej statyki:\n\
  sudo apt install musl-tools"
        );
    }

    if *mode == CompileMode::Binary {
        set_executable(output);
    }

    Ok(())
}

/// Linkowanie przez musl-gcc — pelna statyka bez problemow z libgcc
fn link_musl(
    hl_obj:  &Path,
    rt_obj:  &Path,
    output:  &Path,
    mode:    &CompileMode,
    verbose: bool,
) -> Result<()> {
    let output_str = output.to_str().unwrap_or("a.out");
    let hl_str     = hl_obj.to_str().unwrap_or("hl.o");
    let rt_str     = rt_obj.to_str().unwrap_or("rt.o");

    let args = match mode {
        CompileMode::Binary => vec![
            "-static", "-o", output_str, hl_str, rt_str,
        ],
        CompileMode::Shared => vec![
            "-shared", "-fPIC", "-o", output_str, hl_str, rt_str,
        ],
    };

    if verbose {
        eprintln!("  LINK (musl): musl-gcc {}", args.join(" "));
    }

    let status = Command::new("musl-gcc")
        .args(&args)
        .status()
        .map_err(|e| anyhow::anyhow!("musl-gcc: {}", e))?;

    if !status.success() {
        bail!("Linkowanie przez musl-gcc takze zakonczone bledem.");
    }

    if *mode == CompileMode::Binary {
        set_executable(output);
    }

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
    // Preferuj gcc — najlepsza kompatybilnosc dla -static z glibc
    for cc in &["gcc", "cc", "clang"] {
        if which::which(cc).is_ok() {
            return Ok(cc.to_string());
        }
    }
    bail!("Nie znaleziono kompilatora C. Zainstaluj: sudo apt install gcc");
}
