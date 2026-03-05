// build.rs — hl-runtime
//
// Kompiluje dwie biblioteki C:
//   gc.c   → libgc.a   (generacyjny GC, używany przez cały runtime)
//   aa.c   → libaa.a   (arena allocator, używany tylko przez :: bloki)
//
// Struktura projektu:
//   Hacker-Lang/
//     gc.c              ← tutaj
//     aa.c              ← tutaj
//     hl-runtime/
//       build.rs        ← ten plik
//       Cargo.toml
//       src/main.rs
//
// aa.c jest kompilowane z -DHL_ARENA_MODE_JIT (tryb interpreter/runtime).
// Tryb HL_ARENA_MODE_COMPILER jest używany tylko przez hl-compiler AOT.
//
// libaa.a jest instalowana do:
//   ~/.hackeros/hacker-lang/libs/libaa.a
// ale build.rs kompiluje ją lokalnie do OUT_DIR i linkuje statycznie.

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // ── gc.c ──────────────────────────────────────────────────────────────
    let gc_c = manifest_dir.join("..").join("gc.c");

    if !gc_c.exists() {
        panic!(
            "\n\n[build.rs] Nie znaleziono gc.c pod: {}\n\
Upewnij się że struktura katalogów wygląda tak:\n\
Hacker-Lang/\n\
gc.c          ← tu\n\
aa.c          ← tu\n\
hl-runtime/\n\
build.rs\n\
Cargo.toml\n\
src/main.rs\n\n",
gc_c.canonicalize().unwrap_or(gc_c.clone()).display()
        );
    }

    cc::Build::new()
    .file(&gc_c)
    .opt_level(3)
    .flag("-march=native")
    .flag("-mtune=native")
    .flag("-fomit-frame-pointer")
    .flag("-funroll-loops")
    .flag("-fno-stack-protector")
    .flag_if_supported("-ffunction-sections")
    .flag_if_supported("-fdata-sections")
    .flag_if_supported("-fvisibility=hidden")
    .define(
        "GC_DEBUG",
        if cfg!(debug_assertions) { Some("1") } else { None },
    )
    .compile("gc");

    println!("cargo:rustc-link-lib=static=gc");
    println!("cargo:rustc-link-lib=c");
    println!(
        "cargo:rerun-if-changed={}",
        gc_c.canonicalize().unwrap_or(gc_c).display()
    );

    // ── aa.c ──────────────────────────────────────────────────────────────
    // Kompilujemy z -DHL_ARENA_MODE_JIT — runtime używa stosu aren per ramka.
    // Tryb COMPILER jest wyłączony — używa go tylko hl-compiler AOT.
    //
    // WAŻNE: aa.c NIE zastępuje GC. Cały runtime nadal używa gc.c.
    //        Arena jest używana wyłącznie wewnątrz bloków :: name [size] def.
    //        Po wyjściu z bloku (done) arena jest zwalniana jednym free().
    //        Obiekty poza :: blokami są zarządzane normalnie przez GC.

    let aa_c = manifest_dir.join("..").join("aa.c");

    if !aa_c.exists() {
        panic!(
            "\n\n[build.rs] Nie znaleziono aa.c pod: {}\n\
aa.c (arena allocator) jest wymagany przez runtime dla obsługi :: bloków.\n\
Upewnij się że struktura katalogów wygląda tak:\n\
Hacker-Lang/\n\
gc.c          ← tu\n\
aa.c          ← tu\n\
hl-runtime/\n\
build.rs\n\n",
aa_c.canonicalize().unwrap_or(aa_c.clone()).display()
        );
    }

    cc::Build::new()
    .file(&aa_c)
    .opt_level(3)
    .flag("-march=native")
    .flag("-mtune=native")
    .flag("-fomit-frame-pointer")
    .flag("-funroll-loops")
    .flag("-fno-stack-protector")
    .flag_if_supported("-ffunction-sections")
    .flag_if_supported("-fdata-sections")
    .flag_if_supported("-fvisibility=hidden")
    // Tryb JIT — stos aren per ramka wywołania, hl_jit_arena_* API
    .define("HL_ARENA_MODE_JIT", None)
    .define(
        "HL_ARENA_DEBUG",
        if cfg!(debug_assertions) { Some("1") } else { None },
    )
    .compile("aa");

    println!("cargo:rustc-link-lib=static=aa");
    println!(
        "cargo:rerun-if-changed={}",
        aa_c.canonicalize().unwrap_or(aa_c).display()
    );

    // ── Wspólne ───────────────────────────────────────────────────────────
    println!("cargo:rerun-if-changed=build.rs");
}
