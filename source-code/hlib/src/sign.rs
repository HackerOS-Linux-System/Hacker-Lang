use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;

use crate::error::{HlibError, Result};

/// Generates a fresh Ed25519 keypair for signing `.hlib` archives.
/// Returns `(signing_key_hex, verifying_key_hex)` — both lowercase hex,
/// 64 and 32 bytes respectively before encoding. The signing key is
/// secret; only the verifying (public) key should ever be distributed
/// or committed to a trust store.
pub fn generate_keypair() -> (String, String) {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();
    (
        hex::encode(signing_key.to_bytes()),
        hex::encode(verifying_key.to_bytes()),
    )
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

/// Signs `message` (in practice, the raw bytes of `CHECKSUMS.sha256`)
/// and returns the raw 64-byte Ed25519 signature.
pub fn sign(signing_key: &SigningKey, message: &[u8]) -> [u8; 64] {
    let sig: Signature = signing_key.sign(message);
    sig.to_bytes()
}

/// Verifies a raw 64-byte Ed25519 signature over `message`.
pub fn verify(verifying_key: &VerifyingKey, message: &[u8], signature_bytes: &[u8]) -> Result<bool> {
    let arr: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| HlibError::BadKey("signature must be exactly 64 bytes".into()))?;
    let sig = Signature::from_bytes(&arr);
    Ok(verifying_key.verify(message, &sig).is_ok())
}
