#[derive(Debug, Clone, PartialEq)]
pub struct ShebangInfo {
    /// Oryginalna linia shebang (z #!)
    pub raw:         String,
    /// Interpreter (np. "/usr/bin/env hl" lub "/usr/bin/hl")
    pub interpreter: String,
    /// Argumenty po interpreterze
    pub args:        Vec<String>,
}

impl ShebangInfo {
    /// Sprawdz czy linia to shebang
    pub fn is_shebang(line: &str) -> bool {
        line.starts_with("#!")
    }

    /// Parsuj linie shebang
    pub fn parse(line: &str) -> Option<ShebangInfo> {
        if !line.starts_with("#!") { return None; }
        let rest = line[2..].trim();
        let mut parts = rest.split_whitespace();
        let interpreter = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(|s| s.to_string()).collect();
        if interpreter.is_empty() { return None; }
        Some(ShebangInfo { raw: line.to_string(), interpreter, args })
    }

    /// Sprawdz czy shebang wskazuje na hl
    pub fn is_hl_shebang(&self) -> bool {
        self.interpreter.ends_with("hl")
            || self.interpreter.ends_with("env") && self.args.first().map(|s| s == "hl").unwrap_or(false)
    }
}

/// Wynik pre-processingu zrodla
#[derive(Debug, Clone)]
pub struct PreprocessResult {
    /// Zrodlo bez linii shebang (gotowe do parsowania)
    pub source:  String,
    /// Informacje o shebangu (jesli byl)
    pub shebang: Option<ShebangInfo>,
    /// Numer linii od ktorego zaczyna sie prawdziwe zrodlo (1 lub 2)
    pub offset:  usize,
}

/// Pre-procesuj zrodlo: wyciagnij shebang jesli istnieje
///
/// Shebang jest usuwany z pierwszej linii i zastapiony pustą linia
/// (zeby zachowac numery linii w diagnostykach).
pub fn preprocess(source: &str) -> PreprocessResult {
    let mut lines = source.splitn(2, '\n');
    let first = lines.next().unwrap_or("");
    let rest  = lines.next().unwrap_or("");

    if ShebangInfo::is_shebang(first) {
        let shebang = ShebangInfo::parse(first);
        // Zastap pierwsza linie pustą (zachowuje numery linii)
        let new_source = format!("\n{}", rest);
        PreprocessResult {
            source:  new_source,
            shebang,
            offset:  1,
        }
    } else {
        PreprocessResult {
            source:  source.to_string(),
            shebang: None,
            offset:  0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shebang_env_hl() {
        let si = ShebangInfo::parse("#!/usr/bin/env hl").unwrap();
        assert_eq!(si.interpreter, "/usr/bin/env");
        assert_eq!(si.args, vec!["hl"]);
        assert!(si.is_hl_shebang());
    }

    #[test]
    fn test_shebang_direct() {
        let si = ShebangInfo::parse("#!/usr/bin/hl").unwrap();
        assert_eq!(si.interpreter, "/usr/bin/hl");
        assert!(si.is_hl_shebang());
    }

    #[test]
    fn test_shebang_with_args() {
        let si = ShebangInfo::parse("#!/usr/bin/env hl --verbose").unwrap();
        assert!(si.is_hl_shebang());
        assert!(si.args.contains(&"--verbose".to_string()));
    }

    #[test]
    fn test_preprocess_with_shebang() {
        let src = "#!/usr/bin/env hl\n/// doc\n> ls";
        let r = preprocess(src);
        assert!(r.shebang.is_some());
        assert_eq!(r.offset, 1);
        assert!(r.source.starts_with('\n'));
        assert!(r.source.contains("/// doc"));
    }

    #[test]
    fn test_preprocess_without_shebang() {
        let src = "/// doc\n> ls";
        let r = preprocess(src);
        assert!(r.shebang.is_none());
        assert_eq!(r.offset, 0);
        assert_eq!(r.source, src);
    }

    #[test]
    fn test_no_false_positive_import() {
        // # <std/net> to import, nie shebang
        assert!(!ShebangInfo::is_shebang("# <std/net>"));
        assert!(!ShebangInfo::is_shebang("#! /usr/bin/env hl")); // spacja po #!
    }
}
