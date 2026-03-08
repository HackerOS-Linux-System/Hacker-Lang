// build.rs — hl-compiler
//
// Kompiluje statycznie:
//   1. gc.c                    → libgc.a
//   2. aa.c                    → libaa.a (dwa warianty: COMPILER + JIT)
//   3. modules/hl_runtime.c    → libhl_runtime.a
//   4. modules/hl_string.c     → libhl_string.a
//   5. modules/hl_collections.c → libhl_collections.a
//
// Instalacja: ~/.hackeros/hacker-lang/libs/
//
// Struktura projektu:
//   Hacker-Lang/
//     gc.c
//     aa.c
//     hl-compiler/
//       build.rs          ← ten plik
//       Cargo.toml
//       modules/
//         hl_runtime.h
//         hl_runtime.c
//         hl_string.c
//         hl_collections.c
//       src/

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root         = manifest_dir.join("..");
    let modules_dir  = manifest_dir.join("modules");

    // ── Ścieżki źródłowe ────────────────────────────────────
    let gc_c            = root.join("gc.c");
    let aa_c            = root.join("aa.c");
    let hl_runtime_c    = modules_dir.join("hl_runtime.c");
    let hl_string_c     = modules_dir.join("hl_string.c");
    let hl_collections_c = modules_dir.join("hl_collections.c");
    let hl_runtime_h    = modules_dir.join("hl_runtime.h");

    check_file(&gc_c,             "gc.c",              "../gc.c");
    check_file(&aa_c,             "aa.c",              "../aa.c");
    check_file(&hl_runtime_c,    "hl_runtime.c",      "modules/hl_runtime.c");
    check_file(&hl_string_c,     "hl_string.c",       "modules/hl_string.c");
    check_file(&hl_collections_c,"hl_collections.c",  "modules/hl_collections.c");

    // ── Wspólna konfiguracja kompilatora ────────────────────
    let is_debug = cfg!(debug_assertions);

    let mut base = cc::Build::new();
    base
    .opt_level(3)
    .flag("-march=native")
    .flag("-mtune=native")
    .flag("-fomit-frame-pointer")
    .flag("-funroll-loops")
    .flag("-fno-stack-protector")
    .flag_if_supported("-ffunction-sections")
    .flag_if_supported("-fdata-sections")
    .flag_if_supported("-fvisibility=hidden")
    // include modules/ żeby hl_string.c i hl_collections.c
    // mogły #include "hl_runtime.h"
    .include(&modules_dir)
    .define("GC_DEBUG",        if is_debug { Some("1") } else { None })
    .define("HL_ARENA_DEBUG",  if is_debug { Some("1") } else { None });

    // ── 1. gc.c → libgc.a ───────────────────────────────────
    base.clone()
    .file(&gc_c)
    .compile("gc");
    println!("cargo:rustc-link-lib=static=gc");

    // ── 2a. aa.c → libaa_compiler_mode.a (dla hl-compiler) ──
    base.clone()
    .file(&aa_c)
    .define("HL_ARENA_MODE_COMPILER", None)
    .compile("aa_compiler_mode");
    println!("cargo:rustc-link-lib=static=aa_compiler_mode");

    // ── 2b. aa.c → libaa.a (JIT — linkowane do output .hl) ──
    base.clone()
    .file(&aa_c)
    .define("HL_ARENA_MODE_JIT", None)
    .compile("aa");
    println!("cargo:rustc-link-lib=static=aa");

    // ── 3. hl_runtime.c → libhl_runtime.a ───────────────────
    base.clone()
    .file(&hl_runtime_c)
    .include(&modules_dir)
    .compile("hl_runtime");
    println!("cargo:rustc-link-lib=static=hl_runtime");

    // ── 4. hl_string.c → libhl_string.a ─────────────────────
    base.clone()
    .file(&hl_string_c)
    .include(&modules_dir)
    .compile("hl_string");
    println!("cargo:rustc-link-lib=static=hl_string");

    // ── 5. hl_collections.c → libhl_collections.a ───────────
    base.clone()
    .file(&hl_collections_c)
    .include(&modules_dir)
    .compile("hl_collections");
    println!("cargo:rustc-link-lib=static=hl_collections");

    // ── libc ─────────────────────────────────────────────────
    println!("cargo:rustc-link-lib=c");

    // ── Instalacja do ~/.hackeros/hacker-lang/libs/ ──────────
    // Instalujemy wersje JIT libaa.a i wszystkie runtime libs
    // bo do nich linkują skompilowane binarki .hl
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    if let Some(home) = dirs::home_dir() {
        let libs_dir = home.join(".hackeros/hacker-lang/libs");
        if fs::create_dir_all(&libs_dir).is_ok() {
            install_lib(&out_dir, &libs_dir, "libgc.a");
            install_lib(&out_dir, &libs_dir, "libaa.a");
            install_lib(&out_dir, &libs_dir, "libhl_runtime.a");
            install_lib(&out_dir, &libs_dir, "libhl_string.a");
            install_lib(&out_dir, &libs_dir, "libhl_collections.a");
        }
    }

    // ── Instalacja hl_runtime.h do include/ ─────────────────
    if let Some(home) = dirs::home_dir() {
        let inc_dir = home.join(".hackeros/hacker-lang/include");
        if fs::create_dir_all(&inc_dir).is_ok() {
            let dst = inc_dir.join("hl_runtime.h");
            match fs::copy(&hl_runtime_h, &dst) {
                Ok(_)  => println!("cargo:warning=hl_runtime.h → {}", dst.display()),
                Err(e) => println!("cargo:warning=Nie można zainstalować hl_runtime.h: {}", e),
            }
        }
    }

    // ── rerun-if-changed ─────────────────────────────────────
    for src in &[&gc_c, &aa_c, &hl_runtime_c, &hl_string_c, &hl_collections_c, &hl_runtime_h] {
        println!(
            "cargo:rerun-if-changed={}",
            src.canonicalize().unwrap_or_else(|_| src.to_path_buf()).display()
        );
    }
    println!("cargo:rerun-if-changed=build.rs");
}

fn check_file(path: &PathBuf, name: &str, hint: &str) {
    if !path.exists() {
        panic!(
            "\n\n[build.rs] Nie znaleziono '{}' pod: {}\nOczekiwana lokalizacja: {}\n\n",
            name,
            path.display(),
               hint
        );
    }
}

fn install_lib(out_dir: &PathBuf, libs_dir: &PathBuf, name: &str) {
    let src = out_dir.join(name);
    let dst = libs_dir.join(name);
    match fs::copy(&src, &dst) {
        Ok(_)  => println!("cargo:warning={} → {}", name, dst.display()),
        Err(e) => println!("cargo:warning=Nie można zainstalować {}: {}", name, e),
    }
}
