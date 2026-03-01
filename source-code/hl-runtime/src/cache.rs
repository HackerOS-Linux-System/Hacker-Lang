use crate::bytecode::{BytecodeProgram, CACHE_SCHEMA_VERSION};
use colored::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

// ─────────────────────────────────────────────────────────────
// Metadane cache
// ─────────────────────────────────────────────────────────────
#[derive(Serialize, Deserialize)]
struct CacheMeta {
    sha256:         String,
    mtime:          u64,
    file_size:      u64,
    schema_version: u32,
}

// ─────────────────────────────────────────────────────────────
// Ścieżki cache
// ─────────────────────────────────────────────────────────────
fn cache_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        let p = PathBuf::from(xdg).join("hacker-lang");
        if fs::create_dir_all(&p).is_ok() {
            return p;
        }
    }
    let p = dirs::home_dir()
    .expect("HOME not set")
    .join(".cache")
    .join("hacker-lang");
    fs::create_dir_all(&p).ok();
    p
}

/// Para ścieżek (.bc, .meta) dla danego pliku źródłowego
pub fn cache_paths(src: &str) -> (PathBuf, PathBuf) {
    let mut h = Sha256::new();
    h.update(src.as_bytes());
    let key = format!("{:x}", h.finalize());
    let d   = cache_dir();
    (d.join(format!("{}.bc", key)), d.join(format!("{}.meta", key)))
}

// ─────────────────────────────────────────────────────────────
// Pomocniki I/O
// ─────────────────────────────────────────────────────────────
fn file_mtime_size(path: &str) -> Option<(u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    let mt   = meta
    .modified()
    .ok()?
    .duration_since(UNIX_EPOCH)
    .ok()?
    .as_secs();
    Some((mt, meta.len()))
}

fn file_sha256(path: &str) -> String {
    let mut h = Sha256::new();
    h.update(fs::read(path).unwrap_or_default());
    format!("{:x}", h.finalize())
}

// ─────────────────────────────────────────────────────────────
// Ładowanie cache
// ─────────────────────────────────────────────────────────────
pub fn cache_load(src_path: &str, verbose: bool) -> Option<BytecodeProgram> {
    let (bc_path, meta_path) = cache_paths(src_path);

    let meta_bytes = fs::read(&meta_path).ok()?;
    let meta: CacheMeta = bincode::deserialize(&meta_bytes).ok()?;

    // Sprawdź wersję schematu — stary format = regeneruj
    if meta.schema_version != CACHE_SCHEMA_VERSION {
        if verbose {
            eprintln!(
                "{} Cache: stary schemat v{} (aktualny: v{}), regeneruję.",
                      "[*]".yellow(),
                      meta.schema_version,
                      CACHE_SCHEMA_VERSION
            );
        }
        return None;
    }

    // Szybka ścieżka: mtime + size
    if let Some((mtime, size)) = file_mtime_size(src_path) {
        if mtime == meta.mtime && size == meta.file_size {
            if verbose {
                eprintln!("{} Cache hit (mtime+size): {}", "[*]".green(), bc_path.display());
            }
            return load_bc(&bc_path, verbose);
        }

        // Wolna ścieżka: SHA-256 (plik mógł być tylko touched)
        let sha = file_sha256(src_path);
        if sha == meta.sha256 {
            if verbose {
                eprintln!("{} Cache hit (sha256): {}", "[*]".green(), bc_path.display());
            }
            // Zaktualizuj mtime w metadanych
            let new_meta = CacheMeta {
                sha256: sha,
                mtime,
                file_size: size,
                schema_version: CACHE_SCHEMA_VERSION,
            };
            if let Ok(d) = bincode::serialize(&new_meta) {
                let _ = fs::write(&meta_path, d);
            }
            return load_bc(&bc_path, verbose);
        }

        if verbose {
            eprintln!("{} Cache miss (plik zmieniony): {}", "[!]".yellow(), src_path);
        }
    }

    None
}

fn load_bc(path: &PathBuf, verbose: bool) -> Option<BytecodeProgram> {
    let data = fs::read(path).ok()?;
    match bincode::deserialize::<BytecodeProgram>(&data) {
        Ok(mut p) if p.schema_version == CACHE_SCHEMA_VERSION => {
            // Odbuduj StringPool.index (skip during serialization)
            p.rebuild_pool_index();
            Some(p)
        }
        Ok(_) => {
            if verbose {
                eprintln!("{} Cache: niezgodna wersja .bc", "[!]".yellow());
            }
            None
        }
        Err(e) => {
            if verbose {
                eprintln!("{} Cache deserializacja błąd: {}", "[!]".yellow(), e);
            }
            None
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Zapisywanie cache
// ─────────────────────────────────────────────────────────────
pub fn cache_save(src_path: &str, prog: &BytecodeProgram, verbose: bool) {
    let (bc_path, meta_path) = cache_paths(src_path);
    let sha256               = file_sha256(src_path);
    let (mtime, file_size)   = file_mtime_size(src_path).unwrap_or((0, 0));

    let meta = CacheMeta {
        sha256,
        mtime,
        file_size,
        schema_version: CACHE_SCHEMA_VERSION,
    };

    // Zapisz .bc
    match bincode::serialize(prog) {
        Ok(data) => {
            if let Err(e) = fs::write(&bc_path, data) {
                if verbose {
                    eprintln!("{} Błąd zapisu .bc: {}", "[!]".yellow(), e);
                }
                return;
            }
        }
        Err(e) => {
            if verbose {
                eprintln!("{} Serializacja .bc błąd: {}", "[!]".yellow(), e);
            }
            return;
        }
    }

    // Zapisz .meta
    match bincode::serialize(&meta) {
        Ok(data) => {
            if let Err(e) = fs::write(&meta_path, data) {
                if verbose {
                    eprintln!("{} Błąd zapisu .meta: {}", "[!]".yellow(), e);
                }
            } else if verbose {
                eprintln!("{} Cache zapisany: {}", "[*]".green(), cache_dir().display());
            }
        }
        Err(e) => {
            if verbose {
                eprintln!("{} Serializacja .meta błąd: {}", "[!]".yellow(), e);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Czyszczenie cache
// ─────────────────────────────────────────────────────────────

/// Usuń cache dla konkretnego pliku źródłowego
pub fn cache_invalidate(src_path: &str, verbose: bool) {
    let (bc_path, meta_path) = cache_paths(src_path);
    for p in [&bc_path, &meta_path] {
        if p.exists() {
            match fs::remove_file(p) {
                Ok(_) => {
                    if verbose {
                        eprintln!("{} Cache usunięty: {}", "[*]".yellow(), p.display());
                    }
                }
                Err(e) => {
                    eprintln!("{} Błąd usuwania cache {}: {}", "[!]".red(), p.display(), e);
                }
            }
        }
    }
}

/// Usuń cały katalog cache
pub fn cache_clean_all(verbose: bool) {
    let dir = cache_dir();
    match fs::remove_dir_all(&dir) {
        Ok(_) => {
            if verbose {
                eprintln!("{} Cache wyczyszczony: {}", "[*]".green(), dir.display());
            }
        }
        Err(e) => {
            eprintln!("{} Błąd czyszczenia cache: {}", "[!]".red(), e);
        }
    }
}

/// Zwróć rozmiar katalogu cache w bajtach
pub fn cache_size_bytes() -> u64 {
    let dir = cache_dir();
    fs::read_dir(&dir)
    .map(|entries| {
        entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
    })
    .unwrap_or(0)
}
