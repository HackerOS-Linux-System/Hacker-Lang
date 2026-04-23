use thiserror::Error;

/// Aktualny maksymalny wspierany gen
pub const HL_MAX_GEN: u32 = 1;

/// Domyslny gen (gdy brak `using`)
pub const HL_DEFAULT_GEN: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gen(pub u32);

impl Gen {
    pub fn new(n: u32) -> Result<Gen, GenError> {
        if n == 0 {
            return Err(GenError::InvalidGen(n));
        }
        if n > HL_MAX_GEN {
            return Err(GenError::UnsupportedGen { requested: n, max: HL_MAX_GEN });
        }
        Ok(Gen(n))
    }

    pub fn default() -> Gen { Gen(HL_DEFAULT_GEN) }

    pub fn number(&self) -> u32 { self.0 }

    /// Sprawdz czy dana funkcjonalnosc jest dostepna w tym genie
    pub fn supports(&self, feature: GenFeature) -> bool {
        match feature {
            // Gen 1: pelna skladnia podstawowa
            GenFeature::BasicSyntax       => self.0 >= 1,
            GenFeature::ExportOperator    => self.0 >= 1,
            GenFeature::IsolatedCommands  => self.0 >= 1,
            GenFeature::QuickFunctions    => self.0 >= 1,
            GenFeature::Dependencies      => self.0 >= 1,
            GenFeature::Imports           => self.0 >= 1,

            // Gen 2+: zarezerwowane na przyszlosc
            GenFeature::AsyncCommands     => self.0 >= 2,
            GenFeature::TypedVariables    => self.0 >= 2,
            GenFeature::Closures          => self.0 >= 3,
        }
    }
}

impl std::fmt::Display for Gen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "gen {}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GenFeature {
    BasicSyntax,
    ExportOperator,
    IsolatedCommands,
    QuickFunctions,
    Dependencies,
    Imports,
    // Zarezerwowane na przyszlosc
    AsyncCommands,
    TypedVariables,
    Closures,
}

#[derive(Debug, Error, Clone)]
pub enum GenError {
    #[error("Nieprawidlowy gen: {0} (geny sa numerowane od 1)")]
    InvalidGen(u32),
    #[error("Gen {requested} nie jest wspierany przez ta wersje HL (max: gen {max})")]
    UnsupportedGen { requested: u32, max: u32 },
    #[error("Blad parsowania deklaracji gena: '{0}'")]
    ParseError(String),
}

/// Parsuj deklaracje `using <gen N>` z linii
///
/// Przyklad wejscia: "using <gen 1>"
/// Zwraca: Ok(Gen(1)) lub Err(GenError)
pub fn parse_gen_declaration(line: &str) -> Result<Gen, GenError> {
    let line = line.trim();

    // Obsluz: "using <gen N>" i "using gen N"
    let inner = if line.starts_with("using") {
        let rest = line["using".len()..].trim();
        // Usun opcjonalne < >
        let rest = rest.trim_start_matches('<').trim_end_matches('>').trim();
        rest
    } else {
        return Err(GenError::ParseError(line.to_string()));
    };

    // Parsuj "gen N"
    if let Some(n_str) = inner.strip_prefix("gen").map(|s| s.trim()) {
        match n_str.parse::<u32>() {
            Ok(n) => Gen::new(n),
            Err(_) => Err(GenError::ParseError(format!("oczekiwano liczby, dostano '{}'", n_str))),
        }
    } else {
        Err(GenError::ParseError(format!("oczekiwano 'gen N', dostano '{}'", inner)))
    }
}

/// Wyodrebnij deklaracje gena ze zrodla HL
///
/// Skanuje pierwsze linie (ignorujac komentarze i shebang)
/// i zwraca znaleziony gen lub domyslny.
pub fn extract_gen(source: &str) -> (Gen, Option<GenError>) {
    for line in source.lines().take(10) {
        let trimmed = line.trim();

        // Ignoruj shebang
        if trimmed.starts_with('#') && trimmed.starts_with("#!") { continue; }

        // Ignoruj komentarze
        if trimmed.starts_with(";;") || trimmed.starts_with("///") || trimmed.starts_with("//") { continue; }

        // Ignoruj puste linie
        if trimmed.is_empty() { continue; }

        // Sprawdz czy to deklaracja gena
        if trimmed.starts_with("using") {
            match parse_gen_declaration(trimmed) {
                Ok(gen)  => return (gen, None),
                Err(err) => return (Gen::default(), Some(err)),
            }
        }

        // Pierwsza niepusta, niekomentarzowa linia ktora nie jest "using" — koniec szukania
        break;
    }

    (Gen::default(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gen1() {
        let g = parse_gen_declaration("using <gen 1>").unwrap();
        assert_eq!(g.0, 1);
    }

    #[test]
    fn test_parse_gen_no_brackets() {
        let g = parse_gen_declaration("using gen 1").unwrap();
        assert_eq!(g.0, 1);
    }

    #[test]
    fn test_unsupported_gen() {
        let err = parse_gen_declaration("using <gen 99>").unwrap_err();
        assert!(matches!(err, GenError::UnsupportedGen { .. }));
    }

    #[test]
    fn test_extract_gen_from_source() {
        let src = "/// Skrypt\nusing <gen 1>\n\n> ls";
        let (gen, err) = extract_gen(src);
        assert!(err.is_none());
        assert_eq!(gen.0, 1);
    }

    #[test]
    fn test_extract_gen_default() {
        let src = "/// Skrypt bez gena\n> ls";
        let (gen, err) = extract_gen(src);
        assert!(err.is_none());
        assert_eq!(gen.0, HL_DEFAULT_GEN);
    }

    #[test]
    fn test_extract_gen_after_shebang() {
        let src = "#!/usr/bin/env hl\n/// doc\nusing <gen 1>\n> ls";
        let (gen, err) = extract_gen(src);
        assert!(err.is_none());
        assert_eq!(gen.0, 1);
    }
}
