use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::{Rng, RngCore};
use std::path::Path;

const KEYRING_SERVICE: &str = "vico-vee";
const KEYRING_USERNAME: &str = "capability-signing-key";
const FALLBACK_KEY_FILENAME: &str = "vee-signing-key.enc";

pub fn load_or_create_keypair(key_dir: &Path) -> Result<(SigningKey, VerifyingKey), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USERNAME)
        .map_err(|e| format!("keyring entry: {}", e))?;

    if let Ok(seed_hex) = entry.get_password() {
        let bytes = hex::decode(seed_hex).map_err(|e| format!("decode keyring seed: {}", e))?;
        let seed: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "keyring seed length invalid".to_string())?;
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        return Ok((signing_key, verifying_key));
    }

    // Fallback: try encrypted file.
    if let Some(seed) = load_encrypted_seed(key_dir)? {
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        return Ok((signing_key, verifying_key));
    }

    // Generate new key and persist.
    create_and_persist_keypair(key_dir)
}

pub fn create_and_persist_keypair(key_dir: &Path) -> Result<(SigningKey, VerifyingKey), String> {
    let mut seed = [0u8; 32];
    rand::rng().fill_bytes(&mut seed);
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();

    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USERNAME)
        .map_err(|e| format!("keyring entry: {}", e))?;
    let seed_hex = hex::encode(seed);
    if entry.set_password(&seed_hex).is_ok() {
        return Ok((signing_key, verifying_key));
    }

    tracing::warn!("keyring unavailable; falling back to encrypted file for VEE signing key");
    store_encrypted_seed(key_dir, &seed)?;
    Ok((signing_key, verifying_key))
}

fn derive_encryption_key(key_dir: &Path) -> [u8; 32] {
    // Derive a deterministic encryption key for the fallback encrypted seed file.
    // In production deployments this should be overridden by setting
    // VICO_VEE_KEY_PEPPER to a stable, host-specific secret.
    let pepper = std::env::var("VICO_VEE_KEY_PEPPER").unwrap_or_default();
    let material = format!("{}:{}", key_dir.display(), pepper);
    *blake3::hash(material.as_bytes()).as_bytes()
}

fn store_encrypted_seed(key_dir: &Path, seed: &[u8; 32]) -> Result<(), String> {
    use chacha20poly1305::{
        aead::{Aead, KeyInit},
        ChaCha20Poly1305, Nonce,
    };
    std::fs::create_dir_all(key_dir).map_err(|e| format!("create key dir: {}", e))?;
    let key = derive_encryption_key(key_dir);
    let cipher =
        ChaCha20Poly1305::new_from_slice(&key).map_err(|e| format!("init cipher: {}", e))?;
    let nonce_bytes = rand::rng().random::<[u8; 12]>();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, seed.as_ref())
        .map_err(|e| format!("encrypt seed: {}", e))?;
    let mut payload = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    let path = key_dir.join(FALLBACK_KEY_FILENAME);
    std::fs::write(&path, payload).map_err(|e| format!("write encrypted seed: {}", e))?;
    Ok(())
}

fn load_encrypted_seed(key_dir: &Path) -> Result<Option<[u8; 32]>, String> {
    use chacha20poly1305::{
        aead::{Aead, KeyInit},
        ChaCha20Poly1305, Nonce,
    };
    let path = key_dir.join(FALLBACK_KEY_FILENAME);
    let payload = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };
    if payload.len() < 12 + 16 {
        return Err("encrypted seed file is too short".to_string());
    }
    let (nonce_bytes, ciphertext) = payload.split_at(12);
    let key = derive_encryption_key(key_dir);
    let cipher =
        ChaCha20Poly1305::new_from_slice(&key).map_err(|e| format!("init cipher: {}", e))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("decrypt seed: {}", e))?;
    let seed: [u8; 32] = plaintext
        .try_into()
        .map_err(|_| "decrypted seed length invalid".to_string())?;
    Ok(Some(seed))
}
