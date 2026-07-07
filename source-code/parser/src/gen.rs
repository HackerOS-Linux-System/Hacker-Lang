use thiserror::Error;

pub const HL_MAX_GEN: u32 = 2;
pub const HL_DEFAULT_GEN: u32 = 2;

/// Specjalny sentinel oznaczający tryb ROLLING — zawsze najnowsze funkcje
pub const HL_ROLLING_GEN: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gen(pub u32);

impl Gen {
    pub fn new(n: u32) -> Result<Gen, GenError> {
        if n == 0 { return Err(GenError::InvalidGen(n)); }
        if n > HL_MAX_GEN && n != HL_ROLLING_GEN {
            return Err(GenError::UnsupportedGen { requested: n, max: HL_MAX_GEN });
        }
        Ok(Gen(n))
    }

    pub fn default() -> Gen { Gen(HL_DEFAULT_GEN) }

    /// Tryb ROLLING — wszystkie funkcje włączone
    pub fn rolling() -> Gen { Gen(HL_ROLLING_GEN) }

    pub fn number(&self) -> u32 {
        if self.0 == HL_ROLLING_GEN { HL_MAX_GEN } else { self.0 }
    }

    /// Czy to tryb ROLLING (zawsze najnowsze)
    pub fn is_rolling(&self) -> bool { self.0 == HL_ROLLING_GEN }

    pub fn supports(&self, feature: GenFeature) -> bool {
        // ROLLING obsługuje wszystko
        if self.is_rolling() { return true; }
        let g = self.0;
        match feature {
            // Gen 1
            GenFeature::BasicSyntax       => g >= 1,
            GenFeature::ExportOperator    => g >= 1,
            GenFeature::IsolatedCommands  => g >= 1,
            GenFeature::QuickFunctions    => g >= 1,
            GenFeature::Dependencies      => g >= 1,
            GenFeature::Imports           => g >= 1,
            GenFeature::Background        => g >= 1,
            GenFeature::RepeatN           => g >= 1,
            GenFeature::FileImport        => g >= 1,
            GenFeature::Goroutines        => g >= 1,
            GenFeature::HshCommands       => g >= 1,
            // Gen 2
            GenFeature::TypedVariables    => g >= 2,
            GenFeature::ForLoop           => g >= 2,
            GenFeature::WhileLoop         => g >= 2,
            GenFeature::MatchExpr         => g >= 2,
            GenFeature::NativeArithmetic  => g >= 2,
            GenFeature::PipeToVar         => g >= 2,
            GenFeature::MultilineString   => g >= 2,
            GenFeature::HackerOsApi       => g >= 2,
            // Extern system (ROLLING / gen 2+)
            GenFeature::ExternShell       => g >= 2,
            GenFeature::ExternPython      => g >= 2,
            GenFeature::ExternJava        => g >= 2,
            GenFeature::ExternElf         => g >= 2,
            GenFeature::ExternSo          => g >= 2,
            // Gen 3+ (future / ROLLING preview)
            GenFeature::AsyncCommands     => g >= 3,
            GenFeature::Closures          => g >= 3,
        }
    }
}

impl std::fmt::Display for Gen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_rolling() {
            write!(f, "ROLLING")
        } else {
            write!(f, "gen {}", self.0)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GenFeature {
    BasicSyntax, ExportOperator, IsolatedCommands, QuickFunctions,
    Dependencies, Imports, Background, RepeatN, FileImport, Goroutines, HshCommands,
    TypedVariables, ForLoop, WhileLoop, MatchExpr, NativeArithmetic,
    PipeToVar, MultilineString, HackerOsApi,
    // Extern system
    ExternShell, ExternPython, ExternJava, ExternElf, ExternSo,
    // Gen 3+ / ROLLING
    AsyncCommands, Closures,
}

#[derive(Debug, Error, Clone)]
pub enum GenError {
    #[error("Nieprawidlowy gen: {0} (geny sa numerowane od 1)")]
    InvalidGen(u32),
    #[error("Gen {requested} nie jest wspierany (max: gen {max})")]
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
    // Obsługa ROLLING
    if inner.eq_ignore_ascii_case("rolling") || inner.eq_ignore_ascii_case("ROLLING") {
        return Ok(Gen::rolling());
    }
    if let Some(n_str) = inner.strip_prefix("gen").map(|s| s.trim()) {
        match n_str.parse::<u32>() {
            Ok(n) => Gen::new(n),
            Err(_) => Err(GenError::ParseError(format!("oczekiwano liczby, dostano '{}'", n_str))),
        }
    } else {
        Err(GenError::ParseError(format!("oczekiwano 'gen N' lub 'ROLLING', dostano '{}'", inner)))
    }
}

pub fn extract_gen(source: &str) -> (Gen, Option<GenError>) {
    for line in source.lines().take(10) {
        let trimmed = line.trim();
        if trimmed.starts_with("#!") || trimmed.starts_with(";;") ||
           trimmed.starts_with("///") || trimmed.starts_with("//") ||
           trimmed.is_empty() { continue; }
        if trimmed.starts_with("using") {
            return match parse_gen_declaration(trimmed) {
                Ok(gen)  => (gen, None),
                Err(err) => (Gen::default(), Some(err)),
            };
        }
        break;
    }
    (Gen::default(), None)
}
