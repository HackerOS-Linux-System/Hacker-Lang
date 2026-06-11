use anyhow::Result;
use std::path::PathBuf;

pub const CACHE_MAX_FILES: usize = 30;
pub const CACHE_DIR_NAME: &str = ".hackeros/hacker-lang/cache";

pub fn cache_dir() -> PathBuf {
    dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("/tmp"))
    .join(CACHE_DIR_NAME)
}

pub fn bc_cache_path(hash: &str) -> PathBuf {
    cache_dir().join(format!("{}.bc", hash))
}

pub fn ensure_cache_dir() -> Result<()> {
    let dir = cache_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(())
}

/// Jeśli liczba plików .bc w cache > CACHE_MAX_FILES, usuń najstarsze
pub fn cache_cleanup_if_needed() -> Result<()> {
    let dir = cache_dir();
    if !dir.exists() { return Ok(()); }

    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)?
    .flatten()
    .filter_map(|e| {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("bc") { return None; }
        let mtime = e.metadata().ok()?.modified().ok()?;
        Some((mtime, path))
    })
    .collect();

    if entries.len() <= CACHE_MAX_FILES {
        return Ok(());
    }

    // Posortuj od najstarszych
    entries.sort_by_key(|(t, _)| *t);

    let to_remove = entries.len() - CACHE_MAX_FILES;
    for (_, path) in entries.iter().take(to_remove) {
        tracing::debug!("cache cleanup: usuwam {:?}", path);
        let _ = std::fs::remove_file(path);
    }

    tracing::info!("cache cleanup: usunięto {} starych plików .bc", to_remove);
    Ok(())
}

/// Wyczyść cały cache
pub fn cache_clean_all() -> Result<usize> {
    let dir = cache_dir();
    if !dir.exists() { return Ok(0); }

    let count = std::fs::read_dir(&dir)?
    .flatten()
    .filter(|e| {
        e.path().extension().and_then(|x| x.to_str()) == Some("bc")
    })
    .count();

    std::fs::remove_dir_all(&dir)?;
    Ok(count)
}

/// Wylistuj pliki cache ze statystykami
pub fn cache_list() -> Result<Vec<CacheEntry>> {
    let dir = cache_dir();
    if !dir.exists() { return Ok(vec![]); }

    let mut entries: Vec<CacheEntry> = std::fs::read_dir(&dir)?
    .flatten()
    .filter_map(|e| {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("bc") { return None; }
        let meta = e.metadata().ok()?;
        let mtime = meta.modified().ok()?;
        Some(CacheEntry {
            path,
            size: meta.len(),
             modified: mtime,
        })
    })
    .collect();

    entries.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(entries)
}

pub struct CacheEntry {
    pub path:     PathBuf,
    pub size:     u64,
    pub modified: std::time::SystemTime,
}
