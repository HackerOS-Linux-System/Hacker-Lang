use thiserror::Error;

pub const HL_MAX_GEN: u32 = 1;
pub const HL_DEFAULT_GEN: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gen(pub u32);

impl Gen {
    pub fn new(n: u32) -> Result<Gen, GenError> {
        if n == 0 { return Err(GenError::InvalidGen(n)); }
        if n > HL_MAX_GEN {
            return Err(GenError::UnsupportedGen { requested: n, max: HL_MAX_GEN });
        }
        Ok(Gen(n))
    }

    pub fn default() -> Gen { Gen(HL_DEFAULT_GEN) }
    pub fn number(&self) -> u32 { self.0 }

    pub fn supports(&self, feature: GenFeature) -> bool {
        match feature {
            GenFeature::BasicSyntax       => self.0 >= 1,
            GenFeature::ExportOperator    => self.0 >= 1,
            GenFeature::IsolatedCommands  => self.0 >= 1,
            GenFeature::QuickFunctions    => self.0 >= 1,
            GenFeature::Dependencies      => self.0 >= 1,
            GenFeature::Imports           => self.0 >= 1,
            GenFeature::Background        => self.0 >= 1,
            GenFeature::RepeatN           => self.0 >= 1,
            GenFeature::FileImport        => self.0 >= 1,
            GenFeature::Goroutines        => self.0 >= 1,
            GenFeature::HshCommands       => self.0 >= 1,
            // Zarezerwowane
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
    Background,
    RepeatN,
    FileImport,
    Goroutines,
    HshCommands,
    // Zarezerwowane
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

pub fn parse_gen_declaration(line: &str) -> Result<Gen, GenError> {
    let line = line.trim();
    let inner = if line.starts_with("using") {
        let rest = line["using".len()..].trim();
        rest.trim_start_matches('<').trim_end_matches('>').trim()
    } else {
        return Err(GenError::ParseError(line.to_string()));
    };

    if let Some(n_str) = inner.strip_prefix("gen").map(|s| s.trim()) {
        match n_str.parse::<u32>() {
            Ok(n) => Gen::new(n),
            Err(_) => Err(GenError::ParseError(format!("oczekiwano liczby, dostano '{}'", n_str))),
        }
    } else {
        Err(GenError::ParseError(format!("oczekiwano 'gen N', dostano '{}'", inner)))
    }
}

pub fn extract_gen(source: &str) -> (Gen, Option<GenError>) {
    for line in source.lines().take(10) {
        let trimmed = line.trim();
        if trimmed.starts_with('#') && trimmed.starts_with("#!") { continue; }
        if trimmed.starts_with(";;") || trimmed.starts_with("///") || trimmed.starts_with("//") { continue; }
        if trimmed.is_empty() { continue; }
        if trimmed.starts_with("using") {
            match parse_gen_declaration(trimmed) {
                Ok(gen)  => return (gen, None),
                Err(err) => return (Gen::default(), Some(err)),
            }
        }
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
    fn test_unsupported_gen() {
        let err = parse_gen_declaration("using <gen 99>").unwrap_err();
        assert!(matches!(err, GenError::UnsupportedGen { .. }));
    }
}
