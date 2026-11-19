use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{HlibError, Result};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn io_err(path: &Path, source: std::io::Error) -> HlibError {
    HlibError::Io { path: path.to_path_buf(), source }
}

/// Zapis atomowy: plik tymczasowy w tym samym katalogu → fsync → rename.
/// Czytelnik nigdy nie zobaczy połowicznie zapisanego archiwum, a przerwany
/// zapis nie niszczy poprzedniej wersji. Odmawia zapisu przez symlink.
pub(crate) fn write_atomic(dest: &Path, data: &[u8], mode: u32) -> Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(dest) {
        if meta.file_type().is_symlink() {
            return Err(HlibError::SymlinkRefused(dest.to_path_buf()));
        }
    }
    let parent = match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&parent).map_err(|e| io_err(&parent, e))?;
    let fname = dest.file_name().and_then(|f| f.to_str()).unwrap_or("hlib");
    let tmp = parent.join(format!(
        ".{}.tmp-{}-{}",
        fname,
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let result = (|| -> std::io::Result<()> {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;
        let mut f = opts.open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, dest)
    })();

    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_err(dest, e));
    }
    Ok(())
}

/// Tworzy katalogi pośrednie, odmawiając przejścia przez symlink
/// (ochrona przed ucieczką z katalogu docelowego przez wcześniej
/// podstawiony symlink).
pub(crate) fn create_dirs_no_symlink(root: &Path, rel_parent: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(root).map_err(|e| io_err(root, e))?;
    let mut cur = root.to_path_buf();
    for comp in rel_parent.components() {
        cur.push(comp);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => return Err(HlibError::SymlinkRefused(cur)),
            Ok(m) if !m.is_dir() => {
                return Err(io_err(&cur, std::io::Error::new(std::io::ErrorKind::AlreadyExists, "not a directory")))
            }
            Ok(_) => {}
            Err(_) => std::fs::create_dir(&cur).map_err(|e| io_err(&cur, e))?,
        }
    }
    Ok(cur)
}
