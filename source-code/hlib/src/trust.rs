use std::path::Path;

use crate::error::{HlibError, Result};
use crate::read::HlibArchive;
use crate::sign;

/// Wynik oceny polityki.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustStatus {
    /// Brak polityki i archiwum niepodpisane (lub podpis nie był wymagany).
    Unsigned,
    /// Podpisane i zweryfikowane kluczem z polityki (odcisk klucza).
    Trusted(String),
    /// Brak polityki, ale archiwum ma podpis — niezweryfikowany względem żadnego klucza.
    SignedUnpinned,
}

#[derive(Debug, Clone, Default)]
pub struct TrustPolicy {
    /// Zaufane klucze publiczne (hex, 32 B).
    pub keys: Vec<String>,
    /// Jeśli true, niepodpisane archiwa są odrzucane.
    pub require_signed: bool,
}

impl TrustPolicy {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn with_key(mut self, hex: impl Into<String>) -> Result<Self> {
        let h = hex.into();
        sign::verifying_key_from_hex(&h)?; // walidacja od razu
        self.keys.push(h.trim().to_ascii_lowercase());
        Ok(self)
    }

    /// Wczytuje plik zaufanych kluczy: jeden hex na linię, `#` = komentarz.
    pub fn load_keys_file(&mut self, path: &Path) -> Result<()> {
        let text = std::fs::read_to_string(path).map_err(|e| HlibError::Io { path: path.to_path_buf(), source: e })?;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            sign::verifying_key_from_hex(line)?;
            self.keys.push(line.to_ascii_lowercase());
        }
        Ok(())
    }

    /// Z zmiennych środowiskowych:
    /// `HL_HLIB_TRUSTED_KEYS` (hex, oddzielone przecinkami),
    /// `HL_HLIB_TRUSTED_KEYS_FILE` (ścieżka do pliku),
    /// `HL_HLIB_REQUIRE_SIGNED=1`.
    pub fn from_env() -> Result<Self> {
        let mut p = TrustPolicy::default();
        if let Ok(v) = std::env::var("HL_HLIB_TRUSTED_KEYS") {
            for k in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                sign::verifying_key_from_hex(k)?;
                p.keys.push(k.to_ascii_lowercase());
            }
        }
        if let Ok(f) = std::env::var("HL_HLIB_TRUSTED_KEYS_FILE") {
            if !f.is_empty() {
                p.load_keys_file(Path::new(&f))?;
            }
        }
        p.require_signed = matches!(
            std::env::var("HL_HLIB_REQUIRE_SIGNED").as_deref(),
            Ok("1") | Ok("true") | Ok("yes")
        );
        Ok(p)
    }

    pub fn is_enforcing(&self) -> bool {
        self.require_signed || !self.keys.is_empty()
    }

    /// Ocenia archiwum. Gdy polityka coś egzekwuje — błąd zamiast cichego
    /// przepuszczenia (fail closed).
    pub fn check(&self, archive: &HlibArchive) -> Result<TrustStatus> {
        if !self.is_enforcing() {
            return Ok(if archive.is_signed() { TrustStatus::SignedUnpinned } else { TrustStatus::Unsigned });
        }
        if self.keys.is_empty() {
            return Err(HlibError::NoTrustedKeys);
        }
        if !archive.is_signed() {
            return Err(HlibError::Unsigned);
        }
        let mut last = HlibError::BadSignature;
        for k in &self.keys {
            match archive.verify_signature(k) {
                Ok(()) => return Ok(TrustStatus::Trusted(sign::fingerprint(k)?)),
                Err(e) => last = e,
            }
        }
        // podpis istnieje, ale żaden przypięty klucz go nie potwierdza
        let fp = archive
            .manifest
            .signature
            .as_ref()
            .and_then(|s| sign::fingerprint(&s.public_key).ok())
            .unwrap_or_else(|| "unknown".into());
        match last {
            HlibError::BadSignature => Err(HlibError::UntrustedKey(fp)),
            other => Err(other),
        }
    }
}
