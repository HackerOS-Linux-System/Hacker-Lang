use crate::ast::{AnalysisResult, LibType};
use crate::paths::get_libs_base;
use colored::*;
use std::path::PathBuf;
use std::process::{exit, Command};

// ─────────────────────────────────────────────────────────────
// Wykrywanie dostępnego linkera
//
// Hierarchia preferencji:
//   1. mold  — najszybszy, wspiera --icf=all, równoległy
//   2. lld   — LLVM linker, wspiera --icf=all, szybki
//   3. gold  — GNU gold, wspiera --icf=all
//   4. bfd   — domyślny GNU ld, NIE wspiera --icf, najwolniejszy
//
// Wykrywamy przez próbę uruchomienia z --version zamiast polegać
// na $PATH — niektóre systemy mają linker jako ld.gold, gold, itp.
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq)]
enum LinkerKind {
    Mold,
    Lld,
    Gold,
    Bfd,
}

impl LinkerKind {
    /// Zwraca czy ten linker obsługuje --icf=all
    fn supports_icf(&self) -> bool {
        matches!(self, LinkerKind::Mold | LinkerKind::Lld | LinkerKind::Gold)
    }

    /// Zwraca czy ten linker obsługuje -fuse-linker-plugin (LTO plugin)
    fn supports_lto_plugin(&self) -> bool {
        matches!(self, LinkerKind::Gold | LinkerKind::Lld | LinkerKind::Mold)
    }

    /// Nazwa do wyświetlenia
    fn name(&self) -> &'static str {
        match self {
            LinkerKind::Mold => "mold",
            LinkerKind::Lld  => "lld",
            LinkerKind::Gold => "gold",
            LinkerKind::Bfd  => "ld.bfd",
        }
    }
}

/// Wykryj dostępny linker sprawdzając czy binarne istnieje i odpowiada.
fn detect_linker() -> LinkerKind {
    // Sprawdzamy przez `ld.X --version` zamiast `which` —
    // bardziej niezawodne gdy mamy kilka wersji w $PATH.
    let candidates: &[(&str, LinkerKind)] = &[
        ("mold",    LinkerKind::Mold),
        ("ld.lld",  LinkerKind::Lld),
        ("ld.gold", LinkerKind::Gold),
    ];

    for (bin, kind) in candidates {
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

    LinkerKind::Bfd // fallback
}

/// Zlinkuj .o → binarny wykonywalny przez gcc.
///
/// Automatycznie wykrywa najlepszy dostępny linker i stosuje
/// odpowiednie flagi. Flagi specyficzne dla gold/lld/mold
/// (jak --icf=all) są pomijane gdy linker ich nie obsługuje.
pub fn link(
    obj_path:    &str,
    output_name: &str,
    ast:         &AnalysisResult,
    extern_libs: &[(String, bool)],
            pie:         bool,
            verbose:     bool,
) {
    if verbose {
        eprintln!("{} Linkuję: {} → {}", "[*]".green(), obj_path, output_name);
    }

    // ── Wykryj linker ─────────────────────────────────────────
    let linker_kind = detect_linker();
    if verbose {
        eprintln!("{} Linker: {}", "[i]".blue(), linker_kind.name());
    }

    let mut gcc = Command::new("gcc");
    gcc.arg(obj_path).arg("-o").arg(output_name);

    // ── Przekaż linker do gcc ─────────────────────────────────
    // gcc -fuse-ld=X pozwala wybrać linker bez zmiany $PATH
    match &linker_kind {
        LinkerKind::Mold => { gcc.arg("-fuse-ld=mold"); }
        LinkerKind::Lld  => { gcc.arg("-fuse-ld=lld");  }
        LinkerKind::Gold => { gcc.arg("-fuse-ld=gold"); }
        LinkerKind::Bfd  => { /* domyślny — nie przekazujemy flagi */ }
    }

    // ── Optymalizacje GCC ─────────────────────────────────────
    // Nie dodajemy -O3 — .o jest już zoptymalizowane przez LLVM.
    // -O2 na poziomie GCC dotyczy tylko glue code linkera.
    gcc.arg("-O2");
    gcc.arg("-march=native");
    gcc.arg("-mtune=native");
    gcc.arg("-fomit-frame-pointer");
    gcc.arg("-ffunction-sections");
    gcc.arg("-fdata-sections");

    // ── LTO ───────────────────────────────────────────────────
    // -flto=auto działa od GCC 10+.
    // -fuse-linker-plugin działa tylko z gold/lld/mold.
    // ld.bfd wspiera podstawowe LTO przez IR (bez plugin).
    if linker_kind.supports_lto_plugin() {
        gcc.arg("-flto=auto");
        gcc.arg("-fuse-linker-plugin");
    } else {
        // ld.bfd: podstawowe LTO bez plugin (wolniejsze ale działa)
        gcc.arg("-flto");
    }

    // ── PIE / no-PIE ─────────────────────────────────────────
    // LLVM emituje R_X86_64_32 z RelocMode::Default,
    // co jest niezgodne z Debian PIE → wyłączamy -no-pie.
    if !pie {
        gcc.arg("-no-pie");
    }

    // ── Flagi linkera (-Wl,...) ───────────────────────────────

    // -O2: linker-level optymalizacje (sort sekcji, relokacje)
    // Działa na każdym linkerze.
    gcc.arg("-Wl,-O2");

    // --gc-sections: usuń martwe sekcje kodu i danych.
    // Działa na bfd/gold/lld/mold.
    gcc.arg("-Wl,--gc-sections");

    // --icf=all: Identical Code Folding — merguj duplikaty funkcji.
    // TYLKO dla gold/lld/mold — ld.bfd tego NIE obsługuje!
    if linker_kind.supports_icf() {
        gcc.arg("-Wl,--icf=all");
    }

    // --as-needed: linkuj tylko faktycznie używane .so.
    // Działa na bfd/gold/lld/mold.
    gcc.arg("-Wl,--as-needed");

    // --relax: optymalizacje relokacji (call → short call itd.)
    // Działa na bfd/gold; lld/mold obsługują lub ignorują.
    gcc.arg("-Wl,--relax");

    // ── libgc.a ───────────────────────────────────────────────
    let gc_search_paths = gc_search_paths();
    let mut gc_found = false;

    for p in &gc_search_paths {
        let libgc = p.join("libgc.a");
        if libgc.exists() {
            // Bezwzględna ścieżka zamiast -L/-l — unikamy
            // konfliktu z systemowym libgc (Boehm GC).
            gcc.arg(&libgc);
            gc_found = true;
            if verbose {
                eprintln!("{} GC: {}", "[+]".blue(), libgc.display());
            }
            break;
        }
    }

    if !gc_found {
        eprintln!(
            "{} libgc.a nie znaleziona.\n  \
Uruchom raz: cargo build --release\n  \
lub: cp <build>/libgc.a ~/.hackeros/hacker-lang/lib/libgc.a",
"[!]".yellow()
        );
        gcc.arg("-lgc");
    }

    // ── Biblioteki z AST ─────────────────────────────────────
    link_ast_libs(&mut gcc, ast, verbose);

    // ── Extern libs ──────────────────────────────────────────
    link_extern_libs(&mut gcc, extern_libs);

    // ── Standardowe biblioteki systemowe ─────────────────────
    // -lm      — math (sin, cos, pow itd.)
    // -ldl     — dynamic linking (dlopen, dlsym)
    // -lpthread — wielowątkowość (potrzebna przez niektóre pluginy)
    gcc.arg("-lm").arg("-ldl").arg("-lpthread");

    if verbose {
        eprintln!("  {:?}", gcc);
    }

    let status = gcc.status().unwrap_or_else(|e| {
        eprintln!("{} Nie można uruchomić gcc: {}", "[x]".red(), e);
        exit(1);
    });

    // Usuń .o niezależnie od wyniku
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

fn gc_search_paths() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![];
    if let Some(home) = dirs::home_dir() {
        v.push(home.join(".hackeros/hacker-lang/lib"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.to_path_buf());
        }
    }
    v.push(PathBuf::from("/usr/local/lib/hacker-lang"));
    v
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
                    gcc.arg(format!("-L{}", lib_dir.display()));
                    gcc.arg(format!("-Wl,-rpath,{}", lib_dir.display()));
                    gcc.arg(format!("-l:{}.so", lib.name));
                } else if a.exists() {
                    gcc.arg(a.to_str().unwrap());
                } else if verbose {
                    eprintln!("{} Lib '{}' nie znaleziona", "[!]".yellow(), lib.name);
                }
            }
            LibType::Github => {
                let lib_dir = libs_base.join("github").join(&lib.name);
                if lib_dir.exists() {
                    gcc.arg(format!("-L{}", lib_dir.display()));
                    gcc.arg(format!("-Wl,-rpath,{}", lib_dir.display()));
                    gcc.arg(format!("-l:{}.so", lib.name));
                } else if verbose {
                    eprintln!(
                        "{} Github lib '{}' nie znaleziona",
                        "[!]".yellow(),
                              lib.name
                    );
                }
            }
            LibType::Source | LibType::Core | LibType::Vira => {}
        }
    }
}

fn link_extern_libs(gcc: &mut Command, extern_libs: &[(String, bool)]) {
    for (path, is_static) in extern_libs {
        let clean = path.trim_matches('"');
        if *is_static {
            gcc.arg(format!("-l:{}.a", clean));
        } else {
            gcc.arg(format!("-l:{}.so", clean));
        }
    }
}

/// strip(1) — usuń symbole debugowania z gotowego binarnego.
/// Zmniejsza rozmiar o 20–40% bez wpływu na wydajność runtime.
fn strip_binary(path: &str, verbose: bool) {
    let status = Command::new("strip")
    .arg("--strip-all")
    .arg(path)
    .status();

    match status {
        Ok(s) if s.success() => {
            if verbose {
                eprintln!("{} Strip: {}", "[+]".blue(), path);
            }
        }
        Ok(_) => {
            if verbose {
                eprintln!("{} Strip nieudany (niekrytyczny)", "[!]".yellow());
            }
        }
        Err(_) => {} // strip niedostępny — pomijamy bez błędu
    }
}
