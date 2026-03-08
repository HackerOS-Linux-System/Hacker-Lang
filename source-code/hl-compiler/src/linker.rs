use crate::ast::AnalysisResult;
use colored::*;
use std::path::PathBuf;
use std::process::{exit, Command};

// ─────────────────────────────────────────────────────────────
// Wykrywanie linkera
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum LinkerKind {
    Mold,
    Lld,
    Gold,
    Bfd,
}

impl LinkerKind {
    fn supports_icf(&self) -> bool {
        matches!(self, Self::Mold | Self::Lld | Self::Gold)
    }
    fn supports_lto_plugin(&self) -> bool {
        matches!(self, Self::Gold | Self::Lld | Self::Mold)
    }
    fn name(&self) -> &'static str {
        match self {
            Self::Mold => "mold",
            Self::Lld  => "lld",
            Self::Gold => "gold",
            Self::Bfd  => "ld.bfd",
        }
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
        if ok {
            return kind.clone();
        }
    }
    LinkerKind::Bfd
}

// ─────────────────────────────────────────────────────────────
// Ścieżka do bibliotek runtime HL
// ~/.hackeros/hacker-lang/libs/
// ─────────────────────────────────────────────────────────────

fn hl_libs_dir() -> PathBuf {
    dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("/tmp"))
    .join(".hackeros")
    .join("hacker-lang")
    .join("libs")
}

// ─────────────────────────────────────────────────────────────
// link — główna funkcja linkowania
//
// Sygnatura zgodna z wywołaniem w main.rs:
//   linker::link(&obj_path, &output_name, &ast, args.pie, args.verbose)
// ─────────────────────────────────────────────────────────────

pub fn link(
    obj_path:    &str,
    output_name: &str,
    ast:         &AnalysisResult,
    pie:         bool,
    verbose:     bool,
) {
    if verbose {
        eprintln!("{} Linkuje: {} → {}", "[*]".green(), obj_path, output_name);
    }

    let linker   = detect_linker();
    let libs_dir = hl_libs_dir();

    if verbose {
        eprintln!("{} Linker: {}", "[i]".blue(), linker.name());
        eprintln!("{} Libs:   {}", "[i]".blue(), libs_dir.display());
    }

    let mut gcc = Command::new("gcc");

    // Plik obiektowy (musi być PRZED -l*)
    gcc.arg(obj_path);

    // Plik wyjściowy
    gcc.arg("-o").arg(output_name);

    // Wybrany linker
    if let Some(f) = linker.gcc_flag() {
        gcc.arg(f);
    }

    // ── Optymalizacje ─────────────────────────────────────────
    gcc.arg("-O2")
    .arg("-march=native")
    .arg("-mtune=native")
    .arg("-fomit-frame-pointer")
    .arg("-ffunction-sections")
    .arg("-fdata-sections");

    // ── LTO ──────────────────────────────────────────────────
    if linker.supports_lto_plugin() {
        gcc.arg("-flto=auto").arg("-fuse-linker-plugin");
    } else {
        gcc.arg("-flto");
    }

    // ── PIE ──────────────────────────────────────────────────
    if !pie {
        gcc.arg("-no-pie");
    }

    // ── Flagi linkera ─────────────────────────────────────────
    gcc.arg("-Wl,-O2")
    .arg("-Wl,--gc-sections")
    .arg("-Wl,--as-needed")
    .arg("-Wl,--relax");
    if linker.supports_icf() {
        gcc.arg("-Wl,--icf=all");
    }

    // ── Ścieżka wyszukiwania bibliotek HL ─────────────────────
    gcc.arg(format!("-L{}", libs_dir.display()));

    // ── Biblioteki runtime HL (kolejność KRYTYCZNA!) ──────────
    //
    // Przy static linking ld.bfd przetwarza archiwum .a tylko raz,
    // od lewej do prawej. Symbol musi być zażądany PRZED archiwum
    // które go zawiera.
    //
    // Łańcuch zależności:
    //   hl_runtime  ──► gc_malloc  (libgc.a)
    //   hl_string   ──► gc_malloc  (libgc.a)
    //   hl_collections ─► gc_malloc (libgc.a)
    //   libaa.a     ──► (brak zależności od innych HL libs)
    //   libgc.a     ──► (niezależna)
    //
    // Poprawna kolejność:
    //   -lhl_runtime -lhl_string -lhl_collections -lgc [-laa]
    //
    // PRZED naprawą: brak tych flag → 30+ "undefined reference" błędów
    gcc.arg("-lhl_runtime");
    gcc.arg("-lhl_string");
    gcc.arg("-lhl_collections");
    gcc.arg("-lgc");

    // ── libaa.a — tylko gdy program używa :: bloków ───────────
    if ast.uses_arena() {
        if verbose {
            eprintln!("{} Arena: libaa.a (HL_ARENA_MODE_JIT)", "[i]".blue());
        }
        gcc.arg("-laa");
    }

    // ── Biblioteki z AST (#<bytes/...>, #<github/...>) ────────
    link_ast_libs(&mut gcc, ast, &libs_dir, verbose);

    // ── Extern libs (-- [static] path) ────────────────────────
    for (path, is_static) in ast.extern_libs() {
        let clean = path.trim_matches('"');
        if is_static {
            gcc.arg("-Wl,-Bstatic");
            gcc.arg(format!("-l:{}.a", clean));
            gcc.arg("-Wl,-Bdynamic");
        } else {
            gcc.arg(format!("-l:{}.so", clean));
        }
    }

    // ── Biblioteki systemowe ──────────────────────────────────
    gcc.arg("-lm").arg("-ldl");
    if ast.uses_async() {
        gcc.arg("-lpthread");
    }

    if verbose {
        let args: Vec<_> = gcc
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
        eprintln!("{} gcc {}", "[cmd]".dimmed(), args.join(" "));
    }

    let status = gcc.status().unwrap_or_else(|e| {
        eprintln!("{} Nie można uruchomić gcc: {}", "[x]".red(), e);
        exit(1);
    });

    // Zawsze usuń .o (niezależnie od wyniku)
    let _ = std::fs::remove_file(obj_path);

    if status.success() {
        strip_binary(output_name, verbose);
        eprintln!("{} Skompilowano: {}", "[+]".green(), output_name);
    } else {
        eprintln!("{} Linkowanie nieudane", "[x]".red());
        diagnose_libs(&libs_dir, verbose);
        exit(1);
    }
}

// ─────────────────────────────────────────────────────────────
// link_ast_libs — biblioteki zadeklarowane w kodzie .hl
// ─────────────────────────────────────────────────────────────

fn link_ast_libs(
    gcc:      &mut Command,
    ast:      &AnalysisResult,
    libs_dir: &std::path::Path,
    verbose:  bool,
) {
    use crate::ast::LibType;

    for lib in &ast.libs {
        match lib.lib_type {
            LibType::Bytes | LibType::Virus | LibType::Vira => {
                let lib_dir = libs_dir.join("bytes").join(&lib.name);
                let so = lib_dir.join(format!("{}.so", lib.name));
                let a  = lib_dir.join(format!("{}.a",  lib.name));
                if so.exists() {
                    gcc.arg(format!("-L{}", lib_dir.display()))
                    .arg(format!("-Wl,-rpath,{}", lib_dir.display()))
                    .arg(format!("-l:{}.so", lib.name));
                    if verbose {
                        eprintln!("{} lib (dyn): {}", "[+]".blue(), so.display());
                    }
                } else if a.exists() {
                    gcc.arg(a.to_str().unwrap());
                    if verbose {
                        eprintln!("{} lib (sta): {}", "[+]".blue(), a.display());
                    }
                } else if verbose {
                    eprintln!("{} Lib '{}' nie znaleziona", "[!]".yellow(), lib.name);
                }
            }
            LibType::Github => {
                let lib_dir = libs_dir.join("github").join(&lib.name);
                if lib_dir.exists() {
                    gcc.arg(format!("-L{}", lib_dir.display()))
                    .arg(format!("-Wl,-rpath,{}", lib_dir.display()))
                    .arg(format!("-l:{}.so", lib.name));
                    if verbose {
                        eprintln!("{} lib (gh): {}", "[+]".blue(), lib_dir.display());
                    }
                } else if verbose {
                    eprintln!("{} Github lib '{}' nie znaleziona", "[!]".yellow(), lib.name);
                }
            }
            LibType::Source | LibType::Core => {}
        }
    }
}

// ─────────────────────────────────────────────────────────────
// diagnose_libs — pokaż które .a brakuje gdy linkowanie padło
// ─────────────────────────────────────────────────────────────

fn diagnose_libs(libs_dir: &std::path::Path, verbose: bool) {
    let required = [
        "libhl_runtime.a",
        "libhl_string.a",
        "libhl_collections.a",
        "libgc.a",
        "libaa.a",
    ];

    let mut any_missing = false;
    for lib in &required {
        let path = libs_dir.join(lib);
        if !path.exists() {
            eprintln!("{} BRAK: {} ({})", "[!]".red(), lib, path.display());
            any_missing = true;
        } else if verbose {
            eprintln!("{} OK:   {}", "[i]".blue(), path.display());
        }
    }

    if any_missing {
        eprintln!(
            "{} Uruchom 'cargo build' w hl-compiler aby zainstalować biblioteki.",
            "[!]".yellow()
        );
    }
}

// ─────────────────────────────────────────────────────────────
// strip_binary — usuń symbole debug z finalnej binarki
// ─────────────────────────────────────────────────────────────

fn strip_binary(path: &str, verbose: bool) {
    match Command::new("strip")
    .arg("--strip-all")
    .arg(path)
    .status()
    {
        Ok(s) if s.success() => {
            if verbose {
                eprintln!("{} Strip: {}", "[+]".blue(), path);
            }
        }
        _ => {}
    }
}
