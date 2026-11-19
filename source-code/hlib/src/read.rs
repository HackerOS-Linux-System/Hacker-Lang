use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{HlibError, Result};
use crate::fsutil;
use crate::limits::Limits;
use crate::manifest::{Artifact, ArtifactKind, Manifest};
use crate::sign;
use crate::trust::{TrustPolicy, TrustStatus};
use crate::validate::{is_hex_sha256, normalize_entry_path};

pub(crate) const CHECKSUMS_ENTRY: &str = "CHECKSUMS.sha256";
pub(crate) const SIGNATURE_ENTRY: &str = "SIGNATURE.ed25519";
pub(crate) const MANIFEST_ENTRY: &str = "manifest.json";

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn is_meta(path: &str) -> bool {
    path == MANIFEST_ENTRY || path == CHECKSUMS_ENTRY || path == SIGNATURE_ENTRY
}

/// Otwarte archiwum `.hlib`, w pełni zdekompresowane i zindeksowane w pamięci.
///
/// `open*` waliduje strukturę (ścieżki, typy wpisów, limity, manifest), ale
/// **nie** sprawdza sum ani podpisu — wołaj `verify_checksums()` /
/// `verify_signature()` / `verify(&TrustPolicy)` zanim zaufasz zawartości
/// (zwłaszcza przed `dlopen` lub wykonaniem AST).
pub struct HlibArchive {
    pub manifest: Manifest,
    entries: BTreeMap<String, Vec<u8>>,
    /// SHA-256 całego pliku `.hlib` (do cache i pinowania).
    archive_sha256: String,
    packed_size: u64,
}

impl HlibArchive {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_limits(path, &Limits::DEFAULT)
    }

    pub fn open_with_limits(path: &Path, limits: &Limits) -> Result<Self> {
        let meta = std::fs::metadata(path).map_err(|e| HlibError::Io { path: path.to_path_buf(), source: e })?;
        if meta.len() > limits.max_archive_bytes {
            return Err(HlibError::LimitExceeded {
                what: "archive size",
                actual: meta.len(),
                limit: limits.max_archive_bytes,
            });
        }
        let compressed = std::fs::read(path).map_err(|e| HlibError::Io { path: path.to_path_buf(), source: e })?;
        Self::from_bytes_with_limits(&compressed, path, limits)
    }

    /// Z bajtów w pamięci; `origin` służy tylko do komunikatów błędów.
    pub fn from_bytes(compressed: &[u8], origin: &Path) -> Result<Self> {
        Self::from_bytes_with_limits(compressed, origin, &Limits::DEFAULT)
    }

    pub fn from_bytes_with_limits(compressed: &[u8], origin: &Path, limits: &Limits) -> Result<Self> {
        let not_archive = || HlibError::NotAnArchive(origin.to_path_buf());
        if compressed.len() as u64 > limits.max_archive_bytes {
            return Err(HlibError::LimitExceeded {
                what: "archive size",
                actual: compressed.len() as u64,
                limit: limits.max_archive_bytes,
            });
        }

        // dekompresja z twardym limitem (ochrona przed zip-bombą)
        let mut decoder = zstd::stream::read::Decoder::new(compressed).map_err(|_| not_archive())?;
        let mut tar_bytes = Vec::new();
        let cap = limits.max_unpacked_bytes.saturating_add(1);
        (&mut decoder).take(cap).read_to_end(&mut tar_bytes).map_err(|_| not_archive())?;
        if tar_bytes.len() as u64 > limits.max_unpacked_bytes {
            return Err(HlibError::LimitExceeded {
                what: "unpacked size",
                actual: tar_bytes.len() as u64,
                limit: limits.max_unpacked_bytes,
            });
        }

        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut total: u64 = 0;
        let mut archive = tar::Archive::new(tar_bytes.as_slice());
        for file in archive.entries().map_err(|_| not_archive())? {
            let mut file = file.map_err(|_| not_archive())?;
            let raw_path = file.path().map_err(|_| not_archive())?.to_string_lossy().to_string();
            let etype = file.header().entry_type();
            if etype.is_dir() {
                continue;
            }
            if !(etype.is_file() || etype.is_contiguous()) {
                return Err(HlibError::UnsupportedEntryType(raw_path));
            }
            let path = normalize_entry_path(&raw_path)?;
            if entries.len() >= limits.max_entries {
                return Err(HlibError::LimitExceeded {
                    what: "entry count",
                    actual: entries.len() as u64 + 1,
                    limit: limits.max_entries as u64,
                });
            }
            let size = file.header().size().map_err(|_| not_archive())?;
            if size > limits.max_entry_bytes {
                return Err(HlibError::LimitExceeded { what: "entry size", actual: size, limit: limits.max_entry_bytes });
            }
            total = total.saturating_add(size);
            if total > limits.max_unpacked_bytes {
                return Err(HlibError::LimitExceeded {
                    what: "unpacked size",
                    actual: total,
                    limit: limits.max_unpacked_bytes,
                });
            }
            let mut buf = Vec::with_capacity(size.min(1 << 20) as usize);
            file.read_to_end(&mut buf).map_err(|_| not_archive())?;
            if entries.insert(path.clone(), buf).is_some() {
                // drugi wpis o tej samej ścieżce = próba podmiany zawartości
                return Err(HlibError::DuplicatePath(path));
            }
        }

        let manifest_bytes = entries.get(MANIFEST_ENTRY).ok_or_else(|| HlibError::MissingEntry(MANIFEST_ENTRY.into()))?;
        let manifest = Manifest::from_json(manifest_bytes)?;

        Ok(Self {
            manifest,
            entries,
            archive_sha256: sha256_hex(compressed),
            packed_size: compressed.len() as u64,
        })
    }

    // ── dostęp do danych ────────────────────────────────────────────────────

    pub fn entry_bytes(&self, path: &str) -> Option<&[u8]> {
        self.entries.get(path).map(|v| v.as_slice())
    }

    pub fn entry_paths(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// SHA-256 całego pliku `.hlib` (lowercase hex).
    pub fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }

    pub fn packed_size(&self) -> u64 {
        self.packed_size
    }

    pub fn unpacked_size(&self) -> u64 {
        self.entries.values().map(|v| v.len() as u64).sum()
    }

    pub fn is_signed(&self) -> bool {
        self.entries.contains_key(SIGNATURE_ENTRY)
    }

    /// Bajty artefaktu po ścieżce z manifestu; błąd, jeśli go brak.
    pub fn artifact_bytes(&self, artifact: &Artifact) -> Result<&[u8]> {
        self.entry_bytes(&artifact.path).ok_or_else(|| HlibError::ArtifactMissing(artifact.path.clone()))
    }

    /// Pierwszy artefakt danego rodzaju wraz z bajtami.
    pub fn first_of_kind(&self, kind: ArtifactKind) -> Result<Option<(&Artifact, &[u8])>> {
        match self.manifest.artifacts_of_kind(kind).next() {
            Some(a) => Ok(Some((a, self.artifact_bytes(a)?))),
            None => Ok(None),
        }
    }

    // ── weryfikacja ─────────────────────────────────────────────────────────

    /// Ścisła weryfikacja integralności (spec §4):
    /// * każda linia `CHECKSUMS.sha256` wskazuje istniejący wpis o zgodnym skrócie,
    /// * każdy artefakt z manifestu jest w sumach, a jego `sha256`/`size` zgadzają się z faktycznymi,
    /// * żaden wpis poza `manifest.json`/`CHECKSUMS.sha256`/`SIGNATURE.ed25519` nie jest „bez pokrycia”,
    /// * jeśli jest `SIGNATURE.ed25519`, manifest deklaruje blok `signature` (i odwrotnie).
    ///
    /// To **nie** dowodzi autentyczności — do tego `verify_signature`.
    pub fn verify_checksums(&self) -> Result<()> {
        let checksums_bytes = self
            .entries
            .get(CHECKSUMS_ENTRY)
            .ok_or_else(|| HlibError::MissingEntry(CHECKSUMS_ENTRY.to_string()))?;
        let checksums_text =
            std::str::from_utf8(checksums_bytes).map_err(|_| HlibError::MalformedChecksumLine("<non-utf8 data>".into()))?;

        let mut listed: BTreeMap<String, String> = BTreeMap::new();
        for line in checksums_text.lines() {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() {
                continue;
            }
            let (expected, entry_path) =
                line.split_once("  ").ok_or_else(|| HlibError::MalformedChecksumLine(line.to_string()))?;
            if !is_hex_sha256(expected) {
                return Err(HlibError::MalformedChecksumLine(line.to_string()));
            }
            let entry_path = normalize_entry_path(entry_path)?;
            if is_meta(&entry_path) {
                return Err(HlibError::MalformedChecksumLine(line.to_string()));
            }
            if listed.insert(entry_path.clone(), expected.to_string()).is_some() {
                return Err(HlibError::DuplicatePath(entry_path));
            }
        }

        for (path, expected) in &listed {
            let data = self.entries.get(path).ok_or_else(|| HlibError::MissingEntry(path.clone()))?;
            let actual = sha256_hex(data);
            if &actual != expected {
                return Err(HlibError::ChecksumMismatch { path: path.clone(), expected: expected.clone(), actual });
            }
        }

        for path in self.entries.keys() {
            if !is_meta(path) && !listed.contains_key(path) {
                return Err(HlibError::UntrackedEntry(path.clone()));
            }
        }

        let manifest_paths: BTreeSet<&str> = self.manifest.artifacts.iter().map(|a| a.path.as_str()).collect();
        for a in &self.manifest.artifacts {
            let data = self.entries.get(&a.path).ok_or_else(|| HlibError::ArtifactMissing(a.path.clone()))?;
            let listed_sum = listed
                .get(&a.path)
                .ok_or_else(|| HlibError::ManifestMismatch(format!("artifact `{}` is absent from CHECKSUMS.sha256", a.path)))?;
            if listed_sum != &a.sha256 {
                return Err(HlibError::ManifestMismatch(format!(
                    "artifact `{}`: manifest sha256 differs from CHECKSUMS.sha256",
                    a.path
                )));
            }
            if a.size != data.len() as u64 {
                return Err(HlibError::ManifestMismatch(format!(
                    "artifact `{}`: manifest size {} != actual {}",
                    a.path,
                    a.size,
                    data.len()
                )));
            }
        }
        for path in listed.keys() {
            if !manifest_paths.contains(path.as_str()) {
                return Err(HlibError::ManifestMismatch(format!("entry `{}` is not declared in manifest.artifacts", path)));
            }
        }

        if self.is_signed() != self.manifest.signature.is_some() {
            return Err(HlibError::ManifestMismatch(
                "SIGNATURE.ed25519 presence disagrees with manifest.signature".into(),
            ));
        }
        Ok(())
    }

    /// Weryfikuje podpis względem **jawnie podanego** klucza publicznego
    /// (nigdy względem klucza zawartego w manifeście — ten może podstawić atakujący).
    pub fn verify_signature(&self, expected_public_key_hex: &str) -> Result<()> {
        let sig_bytes = self.entries.get(SIGNATURE_ENTRY).ok_or(HlibError::Unsigned)?;
        let checksums_bytes = self
            .entries
            .get(CHECKSUMS_ENTRY)
            .ok_or_else(|| HlibError::MissingEntry(CHECKSUMS_ENTRY.to_string()))?;
        if let Some(info) = &self.manifest.signature {
            if info.algorithm != "ed25519" || info.signed_entry != CHECKSUMS_ENTRY {
                return Err(HlibError::ManifestMismatch("unsupported signature algorithm / signed_entry".into()));
            }
        }
        let verifying_key = sign::verifying_key_from_hex(expected_public_key_hex)?;
        if sign::verify(&verifying_key, checksums_bytes, sig_bytes)? {
            Ok(())
        } else {
            Err(HlibError::BadSignature)
        }
    }

    /// Weryfikacja „na produkcję”: sumy (ścisłe) + polityka zaufania.
    pub fn verify(&self, policy: &TrustPolicy) -> Result<TrustStatus> {
        self.verify_checksums()?;
        policy.check(self)
    }

    // ── ekstrakcja ──────────────────────────────────────────────────────────

    /// Wypakowuje jeden wpis do `dir/<ścieżka>`; zwraca ścieżkę docelową.
    /// Odmawia przejścia przez symlink, zapis atomowy.
    pub fn extract_entry(&self, entry_path: &str, dir: &Path) -> Result<PathBuf> {
        let rel = normalize_entry_path(entry_path)?;
        let data = self.entries.get(&rel).ok_or_else(|| HlibError::MissingEntry(rel.clone()))?;
        let rel_path = Path::new(&rel);
        let parent_rel = rel_path.parent().unwrap_or(Path::new(""));
        let parent = fsutil::create_dirs_no_symlink(dir, parent_rel)?;
        let dest = parent.join(rel_path.file_name().expect("validated path has a file name"));
        fsutil::write_atomic(&dest, data, 0o644)?;
        Ok(dest)
    }

    /// Wypakowuje wszystkie wpisy (z zachowaniem układu ścieżek).
    pub fn extract_to_dir(&self, dir: &Path) -> Result<()> {
        for path in self.entries.keys() {
            self.extract_entry(path, dir)?;
        }
        Ok(())
    }
}
