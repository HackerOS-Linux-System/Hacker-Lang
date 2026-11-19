use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::error::{HlibError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Version { major, minor, patch, pre: None }
    }

    pub fn parse(s: &str) -> Result<Self> {
        let bad = || HlibError::InvalidVersion(s.to_string());
        let s0 = s.trim();
        let s0 = s0.strip_prefix('v').unwrap_or(s0);
        // metadane budowania (`+...`) nie wpływają na kolejność
        let core_and_pre = s0.split('+').next().ok_or_else(bad)?;
        let (core, pre) = match core_and_pre.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (core_and_pre, None),
        };
        let mut it = core.split('.');
        let num = |p: Option<&str>| -> Result<u64> {
            let p = p.ok_or_else(bad)?;
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || (p.len() > 1 && p.starts_with('0')) {
                return Err(bad());
            }
            p.parse::<u64>().map_err(|_| bad())
        };
        let major = num(it.next())?;
        let minor = num(it.next())?;
        let patch = num(it.next())?;
        if it.next().is_some() {
            return Err(bad());
        }
        let pre = match pre {
            Some(p) => {
                if p.is_empty()
                    || !p.split('.').all(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
                {
                    return Err(bad());
                }
                Some(p.to_string())
            }
            None => None,
        };
        Ok(Version { major, minor, patch, pre })
    }
}

impl FromStr for Version {
    type Err = HlibError;
    fn from_str(s: &str) -> Result<Self> {
        Version::parse(s)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(p) = &self.pre {
            write!(f, "-{}", p)?;
        }
        Ok(())
    }
}

fn cmp_pre(a: &Option<String>, b: &Option<String>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (Some(_), None) => Ordering::Less, // 1.0.0-rc < 1.0.0
        (None, Some(_)) => Ordering::Greater,
        (Some(x), Some(y)) => {
            let (mut xi, mut yi) = (x.split('.'), y.split('.'));
            loop {
                match (xi.next(), yi.next()) {
                    (None, None) => return Ordering::Equal,
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (Some(p), Some(q)) => {
                        let o = match (p.parse::<u64>(), q.parse::<u64>()) {
                            (Ok(a), Ok(b)) => a.cmp(&b),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => p.cmp(q),
                        };
                        if o != Ordering::Equal {
                            return o;
                        }
                    }
                }
            }
        }
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        self.major
            .cmp(&o.major)
            .then(self.minor.cmp(&o.minor))
            .then(self.patch.cmp(&o.patch))
            .then_with(|| cmp_pre(&self.pre, &o.pre))
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
    Caret,
    Tilde,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Comparator {
    op: Op,
    v: Version,
    /// ile składników podano (1 = `1`, 2 = `1.2`, 3 = `1.2.3`)
    parts: u8,
}

/// Wymaganie wersji: `*`, `1.2.3` (jak `^1.2.3`), `^1.2`, `~1.2.3`, `=1.2.3`,
/// `>=1.0, <2.0`. Pusty ciąg i `*` pasują do wszystkiego.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VersionReq {
    comps: Vec<Comparator>,
    raw: String,
}

impl VersionReq {
    pub fn any() -> Self {
        VersionReq { comps: Vec::new(), raw: "*".into() }
    }

    /// Dokładnie ta wersja.
    pub fn exact(v: &Version) -> Self {
        VersionReq {
            comps: vec![Comparator { op: Op::Eq, v: v.clone(), parts: 3 }],
            raw: format!("={}", v),
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        let bad = || HlibError::InvalidVersionReq(s.to_string());
        let t = s.trim();
        if t.is_empty() || t == "*" {
            return Ok(VersionReq { comps: Vec::new(), raw: "*".into() });
        }
        let mut comps = Vec::new();
        for part in t.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err(bad());
            }
            let (op, rest) = if let Some(r) = part.strip_prefix(">=") {
                (Op::Ge, r)
            } else if let Some(r) = part.strip_prefix("<=") {
                (Op::Le, r)
            } else if let Some(r) = part.strip_prefix('>') {
                (Op::Gt, r)
            } else if let Some(r) = part.strip_prefix('<') {
                (Op::Lt, r)
            } else if let Some(r) = part.strip_prefix('=') {
                (Op::Eq, r)
            } else if let Some(r) = part.strip_prefix('^') {
                (Op::Caret, r)
            } else if let Some(r) = part.strip_prefix('~') {
                (Op::Tilde, r)
            } else {
                (Op::Caret, part)
            };
            let rest = rest.trim();
            // częściowe wersje: "1", "1.2"
            let core = rest.split(['-', '+']).next().unwrap_or("");
            let parts = core.split('.').count();
            if !(1..=3).contains(&parts) {
                return Err(bad());
            }
            let (c, tail) = match rest.find(['-', '+']) {
                Some(i) => (&rest[..i], &rest[i..]),
                None => (rest, ""),
            };
            let mut c = c.to_string();
            for _ in parts..3 {
                c.push_str(".0");
            }
            let full = format!("{}{}", c, tail);
            let v = Version::parse(&full).map_err(|_| bad())?;
            comps.push(Comparator { op, v, parts: parts as u8 });
        }
        Ok(VersionReq { comps, raw: t.to_string() })
    }

    /// Import `# <lib:1.2.3>`: pełna wersja = dokładne dopasowanie,
    /// w przeciwnym razie zwykłe wymaganie.
    pub fn parse_import(s: &str) -> Result<Self> {
        match Version::parse(s) {
            Ok(v) => Ok(VersionReq::exact(&v)),
            Err(_) => VersionReq::parse(s),
        }
    }

    pub fn matches(&self, v: &Version) -> bool {
        // wersje wstępne tylko gdy wymaganie samo o nie prosi
        if v.pre.is_some() && !self.comps.iter().any(|c| c.v.pre.is_some()) {
            return false;
        }
        self.comps.iter().all(|c| comp_matches(c, v))
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl FromStr for VersionReq {
    type Err = HlibError;
    fn from_str(s: &str) -> Result<Self> {
        VersionReq::parse(s)
    }
}

fn comp_matches(c: &Comparator, v: &Version) -> bool {
    let base = &c.v;
    match c.op {
        Op::Eq => {
            if c.parts == 3 {
                v == base
            } else {
                v.major == base.major && (c.parts < 2 || v.minor == base.minor)
            }
        }
        Op::Gt => v > base,
        Op::Ge => v >= base,
        Op::Lt => v < base,
        Op::Le => v <= base,
        Op::Tilde => {
            if v < base {
                return false;
            }
            if c.parts == 1 {
                v.major == base.major
            } else {
                v.major == base.major && v.minor == base.minor
            }
        }
        Op::Caret => {
            if v < base {
                return false;
            }
            if base.major > 0 || c.parts == 1 {
                v.major == base.major
            } else if base.minor > 0 || c.parts == 2 {
                v.major == 0 && v.minor == base.minor
            } else {
                v.major == 0 && v.minor == 0 && v.patch == base.patch
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }
    fn r(s: &str) -> VersionReq {
        VersionReq::parse(s).unwrap()
    }

    #[test]
    fn parses_and_orders() {
        assert!(v("1.2.3") < v("1.10.0"));
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("1.0.0-alpha") < v("1.0.0-beta"));
        assert!(v("1.0.0-rc.2") < v("1.0.0-rc.10"));
        assert_eq!(v("v2.0.1+build5"), v("2.0.1"));
        assert_eq!(v("1.2.3-rc.1").to_string(), "1.2.3-rc.1");
        for bad in ["", "1", "1.2", "1.2.3.4", "01.2.3", "a.b.c", "1.2.3-", "1.2.x"] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn requirements() {
        assert!(r("*").matches(&v("9.9.9")));
        assert!(r("^1.2.3").matches(&v("1.9.0")));
        assert!(!r("^1.2.3").matches(&v("2.0.0")));
        assert!(!r("^1.2.3").matches(&v("1.2.2")));
        assert!(r("^0.2.3").matches(&v("0.2.9")));
        assert!(!r("^0.2.3").matches(&v("0.3.0")));
        assert!(r("^0.0.3").matches(&v("0.0.3")));
        assert!(!r("^0.0.3").matches(&v("0.0.4")));
        assert!(r("~1.2.3").matches(&v("1.2.9")));
        assert!(!r("~1.2.3").matches(&v("1.3.0")));
        assert!(r("1").matches(&v("1.5.0")));
        assert!(r(">=1.0, <2.0").matches(&v("1.5.0")));
        assert!(!r(">=1.0, <2.0").matches(&v("2.0.0")));
        assert!(r("=1.2.3").matches(&v("1.2.3")));
        assert!(!r("=1.2.3").matches(&v("1.2.4")));
        assert!(!r("*").matches(&v("1.0.0-rc.1")));
        assert!(r(">=1.0.0-rc.1").matches(&v("1.0.0-rc.2")));
        assert!(VersionReq::parse(">=").is_err());
        assert!(VersionReq::parse("1.2.3.4").is_err());
        assert!(VersionReq::parse_import("1.2.3").unwrap().matches(&v("1.2.3")));
        assert!(!VersionReq::parse_import("1.2.3").unwrap().matches(&v("1.2.4")));
    }
}
