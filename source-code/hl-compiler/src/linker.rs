use crate::ast::{AnalysisResult, LibType};
use crate::paths::get_libs_base;
use colored::*;
use std::path::PathBuf;
use std::process::{exit, Command};

// ─────────────────────────────────────────────────────────────
// Wykrywanie linkera
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum LinkerKind { Mold, Lld, Gold, Bfd }

impl LinkerKind {
    fn supports_icf(&self) -> bool {
        matches!(self, Self::Mold | Self::Lld | Self::Gold)
    }
    fn supports_lto_plugin(&self) -> bool {
        matches!(self, Self::Gold | Self::Lld | Self::Mold)
    }
    fn name(&self) -> &'static str {
        match self { Self::Mold=>"mold", Self::Lld=>"lld", Self::Gold=>"gold", Self::Bfd=>"ld.bfd" }
    }
    fn gcc_flag(&self) -> Option<&'static str> {
        match self {
            Self::Mold => Some("-fuse-ld=mold"),
            Self::Lld  => Some("-fuse-ld=lld"),
            Self::Gold => Some("-fuse-ld=gold"),
            Self::Bfd  => None,
        }
    }
}

fn detect_linker() -> LinkerKind {
    for (bin, kind) in &[
        ("mold",    LinkerKind::Mold),
        ("ld.lld",  LinkerKind::Lld),
        ("ld.gold", LinkerKind::Gold),
    ] {
        let ok = Command::new(bin)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
        if ok { return kind.clone(); }
    }
    LinkerKind::Bfd
}

// ─────────────────────────────────────────────────────────────
// Sciezki do bibliotek
// ─────────────────────────────────────────────────────────────

/// ~/.hackeros/hacker-lang/libs/ — jedyna lokalizacja libgc.a i libaa.a
fn hl_libs_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".hackeros/hacker-lang/libs"))
}

// ─────────────────────────────────────────────────────────────
// Glowna funkcja linkowania
// ─────────────────────────────────────────────────────────────

pub fn link(
    obj_path:    &str,
    output_name: &str,
    ast:         &AnalysisResult,
    pie:         bool,
    verbose:     bool,
) {
    if verbose {
        eprintln!("{} Linkuje: {} -> {}", "[*]".green(), obj_path, output_name);
    }

    let linker = detect_linker();
    if verbose {
        eprintln!("{} Linker: {}", "[i]".blue(), linker.name());
    }

    let mut gcc = Command::new("gcc");
    gcc.arg(obj_path).arg("-o").arg(output_name);

    if let Some(f) = linker.gcc_flag() { gcc.arg(f); }

    // -- Optymalizacje -------------------------------------------
    gcc.arg("-O2")
    .arg("-march=native")
    .arg("-mtune=native")
    .arg("-fomit-frame-pointer")
    .arg("-ffunction-sections")
    .arg("-fdata-sections");

    // -- LTO -----------------------------------------------------
    if linker.supports_lto_plugin() {
        gcc.arg("-flto=auto").arg("-fuse-linker-plugin");
    } else {
        gcc.arg("-flto");
    }

    // -- PIE -----------------------------------------------------
    if !pie { gcc.arg("-no-pie"); }

    // -- Flagi linkera -------------------------------------------
    gcc.arg("-Wl,-O2")
    .arg("-Wl,--gc-sections")
    .arg("-Wl,--as-needed")
    .arg("-Wl,--relax");
    if linker.supports_icf() {
        gcc.arg("-Wl,--icf=all");
    }

    // -- libgc.a — zawsze ----------------------------------------
    link_hl_lib(&mut gcc, "libgc.a", verbose);

    // -- libaa.a — tylko gdy :: bloki ----------------------------
    // Wariant JIT (aa.c -DHL_ARENA_MODE_JIT):
    //   HlJitArenaScope, hl_jit_arena_enter/exit/alloc/reset/cleanup
    //   + wspolne API: hl_arena_new/alloc/reset/free/used/capacity
    if ast.uses_arena() {
        if verbose {
            eprintln!("{} Arena: libaa.a (HL_ARENA_MODE_JIT)", "[i]".blue());
        }
        link_hl_lib(&mut gcc, "libaa.a", verbose);
    }

    // -- Biblioteki z AST (#<bytes/...>) -------------------------
    link_ast_libs(&mut gcc, ast, verbose);

    // -- Extern libs (-- [static] path) --------------------------
    for (path, is_static) in ast.extern_libs() {
        let clean = path.trim_matches('"');
        if is_static {
            gcc.arg(format!("-l:{}.a", clean));
        } else {
            gcc.arg(format!("-l:{}.so", clean));
        }
    }

    // -- Systemowe -----------------------------------------------
    gcc.arg("-lm").arg("-ldl");
    if ast.uses_async() { gcc.arg("-lpthread"); }

    if verbose { eprintln!("  cmd: {:?}", gcc); }

    let status = gcc.status().unwrap_or_else(|e| {
        eprintln!("{} Nie mozna uruchomic gcc: {}", "[x]".red(), e);
        exit(1);
    });

    let _ = std::fs::remove_file(obj_path);

    if status.success() {
        strip_binary(output_name, verbose);
        eprintln!("{} Skompilowano: {}", "[+]".green(), output_name);
    } else {
        eprintln!("{} Linkowanie nieudane", "[x]".red());
        exit(1);
    }
}

// ─────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────

/// Szuka `lib_name` w ~/.hackeros/hacker-lang/libs/ i dodaje do gcc.
/// Dla libgc.a jako ostatni ratunek probuje systemowego -lgc.
fn link_hl_lib(gcc: &mut Command, lib_name: &str, verbose: bool) {
    if let Some(libs_dir) = hl_libs_dir() {
        let lib = libs_dir.join(lib_name);
        if lib.exists() {
            gcc.arg(&lib);
            if verbose {
                eprintln!("{} {}", "[+]".blue(), lib.display());
            }
            return;
        }
        eprintln!(
            "{} {} nie znaleziona w: {}\n  Uruchom: cargo build --release",
            "[!]".yellow(), lib_name, libs_dir.display()
        );
    } else {
        eprintln!("{} Nie mozna ustalic HOME", "[!]".yellow());
    }

    // Ostatni ratunek tylko dla libgc (ma odpowiednik systemowy)
    if lib_name == "libgc.a" {
        eprintln!("  Probuje systemowego -lgc (ostrzezenie: moze byc Boehm GC).");
        gcc.arg("-lgc");
    }
}

fn link_ast_libs(gcc: &mut Command, ast: &AnalysisResult, verbose: bool) {
    let libs_base = get_libs_base();

    for lib in &ast.libs {
        match lib.lib_type {
            LibType::Bytes | LibType::Virus => {
                let lib_dir = libs_base.join("bytes").join(&lib.name);
                let so = lib_dir.join(format!("{}.so", lib.name));
                let a  = lib_dir.join(format!("{}.a",  lib.name));
                if so.exists() {
                    gcc.arg(format!("-L{}", lib_dir.display()))
                    .arg(format!("-Wl,-rpath,{}", lib_dir.display()))
                    .arg(format!("-l:{}.so", lib.name));
                    if verbose { eprintln!("{} lib (dyn): {}", "[+]".blue(), so.display()); }
                } else if a.exists() {
                    gcc.arg(a.to_str().unwrap());
                    if verbose { eprintln!("{} lib (sta): {}", "[+]".blue(), a.display()); }
                } else if verbose {
                    eprintln!("{} Lib '{}' nie znaleziona", "[!]".yellow(), lib.name);
                }
            }
            LibType::Github => {
                let lib_dir = libs_base.join("github").join(&lib.name);
                if lib_dir.exists() {
                    gcc.arg(format!("-L{}", lib_dir.display()))
                    .arg(format!("-Wl,-rpath,{}", lib_dir.display()))
                    .arg(format!("-l:{}.so", lib.name));
                    if verbose { eprintln!("{} lib (gh): {}", "[+]".blue(), lib_dir.display()); }
                } else if verbose {
                    eprintln!("{} Github lib '{}' nie znaleziona", "[!]".yellow(), lib.name);
                }
            }
            LibType::Vira => {
                if verbose {
                    eprintln!("{} lib/vira: {} (vira package manager)", "[i]".blue(), lib.name);
                }
            }
            LibType::Source | LibType::Core => {}
        }
    }
}

fn strip_binary(path: &str, verbose: bool) {
    match Command::new("strip").arg("--strip-all").arg(path).status() {
        Ok(s) if s.success() => {
            if verbose { eprintln!("{} Strip: {}", "[+]".blue(), path); }
        }
        _ => {}
    }
}
