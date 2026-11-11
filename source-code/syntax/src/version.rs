use std::cmp::Ordering;
use std::fmt;

/// Wersja numeryczna z 1–3 składników (`5`, `5.1`, `4.7.2`). Brakujące
/// składniki liczą się jak 0, więc `5` == `5.0` == `5.0.0`.
#[derive(Debug, Clone, Copy)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// Ile składników podał użytkownik (do ładnego wypisywania).
    parts: u8,
}

impl Version {
    pub const fn new(major: u32, minor: u32) -> Self {
        Version { major, minor, patch: 0, parts: 2 }
    }

    /// Parsuje `5.1`, `5`, `4.7.2` (bez prefiksu `V`).
    pub fn parse_numeric(s: &str) -> Result<Version, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("pusta wersja".into());
        }
        let mut nums = [0u32; 3];
        let mut count = 0usize;
        for part in s.split('.') {
            if count >= 3 {
                return Err(format!("za dużo składników w wersji `{}` (max major.minor.patch)", s));
            }
            if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("niepoprawna wersja `{}` — oczekiwano liczb rozdzielonych kropkami", s));
            }
            nums[count] = part
                .parse::<u32>()
                .map_err(|_| format!("zbyt duża liczba w wersji `{}`", s))?;
            count += 1;
        }
        Ok(Version { major: nums[0], minor: nums[1], patch: nums[2], parts: count as u8 })
    }

    /// Parsuje wersję składni HackerOS: `V5.5`, `v5.5`, `5.5`.
    pub fn parse_hackeros(s: &str) -> Result<Version, String> {
        let s = s.trim();
        let digits = s.strip_prefix('V').or_else(|| s.strip_prefix('v')).unwrap_or(s);
        Version::parse_numeric(digits)
    }

    fn key(&self) -> (u32, u32, u32) {
        (self.major, self.minor, self.patch)
    }
}

// Równość wg wartości (`V5` == `V5.0`), spójna z Ord — `parts` służy tylko do wypisywania.
impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}
impl Eq for Version {}
impl std::hash::Hash for Version {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl fmt::Display for Version {
    /// `V5.1` — zawsze z prefiksem `V` i tyloma składnikami, ile podano.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "V{}", self.major)?;
        if self.parts >= 2 {
            write!(f, ".{}", self.minor)?;
        }
        if self.parts >= 3 {
            write!(f, ".{}", self.patch)?;
        }
        Ok(())
    }
}

/// Wersja narzędzia `hl` z dyrektywy `gen <..>`. To czysto informacyjny
/// znacznik — nie wybiera składni (od tego jest `using`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolVersion(pub Version);

impl ToolVersion {
    pub fn parse(s: &str) -> Result<ToolVersion, String> {
        Version::parse_numeric(s).map(ToolVersion)
    }
}

impl PartialOrd for ToolVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.0.cmp(&other.0))
    }
}

impl fmt::Display for ToolVersion {
    /// `1`, `1.1` — bez prefiksu `V` (tak jak w `gen <1.1>`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.major)?;
        if self.0.parts >= 2 {
            write!(f, ".{}", self.0.minor)?;
        }
        if self.0.parts >= 3 {
            write!(f, ".{}", self.0.patch)?;
        }
        Ok(())
    }
}
