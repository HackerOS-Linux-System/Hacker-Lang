use std::path::PathBuf;
use std::env;

fn main() {
    // Katalog w którym leży build.rs = katalog projektu hl-runtime
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // gc.c jest jeden poziom wyżej
    let gc_c = manifest_dir.join("..").join("gc.c");

    // Sprawdź czy plik istnieje — daj czytelny błąd
    if !gc_c.exists() {
        panic!(
            "\n\n[build.rs] Nie znaleziono gc.c pod: {}\n\
Upewnij się że struktura katalogów wygląda tak:\n\
Hacker-Lang/\n\
gc.c          ← tu\n\
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

    // Przebuduj jeśli gc.c się zmienił
    println!(
        "cargo:rerun-if-changed={}",
        gc_c.canonicalize().unwrap_or(gc_c).display()
    );
    println!("cargo:rerun-if-changed=build.rs");
}
