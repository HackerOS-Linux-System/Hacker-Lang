use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

use crate::error::{HlibError, Result};

/// Generuje parę kluczy Ed25519: `(signing_key_hex, verifying_key_hex)`.
pub fn generate_keypair() -> (String, String) {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();
    (hex::encode(signing_key.to_bytes()), hex::encode(verifying_key.to_bytes()))
}

pub fn signing_key_from_hex(hex_str: &str) -> Result<SigningKey> {
    let bytes = hex::decode(hex_str.trim()).map_err(|e| HlibError::BadKey(e.to_string()))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| HlibError::BadKey("signing key must be exactly 32 bytes (64 hex chars)".into()))?;
    Ok(SigningKey::from_bytes(&arr))
}

pub fn verifying_key_from_hex(hex_str: &str) -> Result<VerifyingKey> {
    let bytes = hex::decode(hex_str.trim()).map_err(|e| HlibError::BadKey(e.to_string()))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| HlibError::BadKey("public key must be exactly 32 bytes (64 hex chars)".into()))?;
    VerifyingKey::from_bytes(&arr).map_err(|e| HlibError::BadKey(e.to_string()))
}

/// Klucz publiczny odpowiadający kluczowi prywatnemu (hex → hex).
pub fn public_key_of(signing_key_hex: &str) -> Result<String> {
    Ok(hex::encode(signing_key_from_hex(signing_key_hex)?.verifying_key().to_bytes()))
}

/// Odcisk klucza publicznego: `SHA256:<hex całego skrótu>` — do wyświetlania,
/// pinowania i porównywania w logach.
pub fn fingerprint(public_key_hex: &str) -> Result<String> {
    let key = verifying_key_from_hex(public_key_hex)?;
    Ok(format!("SHA256:{}", hex::encode(Sha256::digest(key.to_bytes()))))
}

/// Podpisuje `message` (w praktyce bajty `CHECKSUMS.sha256`).
pub fn sign(signing_key: &SigningKey, message: &[u8]) -> [u8; 64] {
    let sig: Signature = signing_key.sign(message);
    sig.to_bytes()
}

/// Weryfikuje 64-bajtowy podpis. Używa `verify_strict` (odrzuca podpisy
/// o małym rzędzie / niekanoniczne — brak „malleability”).
pub fn verify(verifying_key: &VerifyingKey, message: &[u8], signature_bytes: &[u8]) -> Result<bool> {
    let arr: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| HlibError::BadKey("signature must be exactly 64 bytes".into()))?;
    let sig = Signature::from_bytes(&arr);
    Ok(verifying_key.verify_strict(message, &sig).is_ok())
}
