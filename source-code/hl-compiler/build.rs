use std::path::PathBuf;
use std::env;
use std::fs;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let gc_c         = manifest_dir.join("..").join("gc.c");

    if !gc_c.exists() {
        panic!(
            "\n\n[build.rs] Nie znaleziono gc.c pod: {}\n\
Upewnij się że struktura katalogów wygląda tak:\n\
Hacker-Lang/\n\
gc.c          ← tu\n\
hl-compiler/\n\
build.rs\n\
Cargo.toml\n\
src/main.rs\n\n",
gc_c.display()
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

    // Zainstaluj libgc.a do ~/.hackeros/hacker-lang/lib/
    // żeby hl-compiler mógł linkować skompilowane programy z GC
    let out_dir  = PathBuf::from(env::var("OUT_DIR").unwrap());
    let libgc_src = out_dir.join("libgc.a");

    if let Some(home) = dirs::home_dir() {
        let install_dir = home.join(".hackeros/hacker-lang/lib");
        if fs::create_dir_all(&install_dir).is_ok() {
            let libgc_dst = install_dir.join("libgc.a");
            match fs::copy(&libgc_src, &libgc_dst) {
                Ok(_)  => println!("cargo:warning=libgc.a zainstalowany: {}", libgc_dst.display()),
                Err(e) => println!("cargo:warning=Nie można zainstalować libgc.a: {}", e),
            }
        }
    }

    println!(
        "cargo:rerun-if-changed={}",
        gc_c.canonicalize().unwrap_or(gc_c).display()
    );
    println!("cargo:rerun-if-changed=build.rs");
}
