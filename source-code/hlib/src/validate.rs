use crate::error::{HlibError, Result};

pub const MAX_NAME_LEN: usize = 64;
pub const MAX_PATH_LEN: usize = 512;

/// Nazwa biblioteki: `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(HlibError::InvalidName(name.to_string()))
    }
}

/// Triple docelowy: `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, ...
pub fn validate_target(target: &str) -> Result<()> {
    let ok = !target.is_empty()
        && target.len() <= 64
        && target.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.');
    if ok && !target.starts_with('.') {
        Ok(())
    } else {
        Err(HlibError::InvalidTarget(target.to_string()))
    }
}

/// Względna, znormalizowana ścieżka wpisu archiwum. Zwraca ścieżkę bez
/// wiodącego `./`. Odrzuca: absolutne, `..`, `\`, NUL, puste komponenty,
/// znaki sterujące.
pub fn normalize_entry_path(path: &str) -> Result<String> {
    let bad = || HlibError::UnsafePath(path.to_string());
    let mut p = path;
    while let Some(rest) = p.strip_prefix("./") {
        p = rest;
    }
    if p.is_empty() || p.len() > MAX_PATH_LEN || p.starts_with('/') || p.ends_with('/') {
        return Err(bad());
    }
    if p.bytes().any(|b| b == 0 || b == b'\\' || b < 0x20 || b == 0x7f) {
        return Err(bad());
    }
    // litera dysku w stylu Windows ("C:")
    if p.len() >= 2 && p.as_bytes()[1] == b':' && p.as_bytes()[0].is_ascii_alphabetic() {
        return Err(bad());
    }
    for comp in p.split('/') {
        if comp.is_empty() || comp == "." || comp == ".." {
            return Err(bad());
        }
    }
    Ok(p.to_string())
}

pub fn validate_entry_path(path: &str) -> Result<()> {
    normalize_entry_path(path).map(|_| ())
}

pub fn is_hex_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in ["a", "mylib", "my-lib_2", "A1"] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-x", "_x", "a/b", "../x", "a b", "a.b", &"x".repeat(65)] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn paths() {
        assert_eq!(normalize_entry_path("./native/x/y.so").unwrap(), "native/x/y.so");
        for bad in [
            "", "/etc/passwd", "../x", "a/../b", "a//b", "a/./b", "a\\b", "C:/x", "a/", "a\0b",
            "a\nb", ".",
        ] {
            assert!(normalize_entry_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn targets() {
        assert!(validate_target("x86_64-unknown-linux-gnu").is_ok());
        assert!(validate_target("../x").is_err());
        assert!(validate_target("").is_err());
    }
}
