use super::types::CapabilityClaims;
use crate::types::{Capability, CapabilityGrant};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{SigningKey, Verifier, VerifyingKey};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Verifies capability grants using only the orchestrator's public key.
///
/// This is the trust boundary for VEE workers and the executor daemon: they
/// never hold the signing key, so they cannot mint new grants.
#[derive(Clone)]
pub struct CapabilityVerifier {
    verifying_key: VerifyingKey,
    /// Shared revocation set, if available. Verifiers built from a public key
    /// alone cannot check revocation.
    revoked: Option<Arc<Mutex<HashSet<String>>>>,
}

impl CapabilityVerifier {
    /// Build a verifier from a hex-encoded Ed25519 public key.
    ///
    /// Revocation checking is unavailable with this constructor; use the issuer
    /// path or call `with_revoked_set` to supply a revocation set.
    pub fn from_public_key_hex(hex_key: &str) -> Result<Self, String> {
        let bytes = hex::decode(hex_key.trim()).map_err(|e| format!("decode pubkey: {}", e))?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "public key must be 32 bytes".to_string())?;
        let verifying_key = VerifyingKey::from_bytes(&bytes)
            .map_err(|_| "invalid Ed25519 public key".to_string())?;
        Ok(Self {
            verifying_key,
            revoked: None,
        })
    }

    /// Build a verifier from a signing key (useful when sharing a keypair in
    /// tests or local development).
    pub fn from_signing_key(signing_key: &SigningKey) -> Self {
        Self {
            verifying_key: signing_key.verifying_key(),
            revoked: None,
        }
    }

    /// Attach a shared revocation set to this verifier.
    pub fn with_revoked_set(mut self, revoked: Arc<Mutex<HashSet<String>>>) -> Self {
        self.revoked = Some(revoked);
        self
    }

    fn is_revoked(&self, jti: &str) -> bool {
        self.revoked
            .as_ref()
            .map(|r| r.lock().unwrap_or_else(|e| e.into_inner()).contains(jti))
            .unwrap_or(false)
    }

    /// Verify a set of grants covers every requested capability for a task.
    ///
    /// Checks each grant is bound to `task_execution_id`, has a valid signature,
    /// is not expired/revoked, and is marked as granted. Then ensures every
    /// capability in `required` has a matching grant.
    pub fn verify_grants_for_task(
        &self,
        task_execution_id: &str,
        grants: &[CapabilityGrant],
        required: &[Capability],
    ) -> Result<Vec<Capability>, String> {
        let mut granted_names = HashSet::new();
        let mut granted_caps = Vec::new();

        for grant in grants {
            if grant.execution_id != task_execution_id {
                return Err("capability grant execution ID mismatch".into());
            }
            self.verify_grant(grant)?;
            if !grant.granted {
                return Err(format!(
                    "capability '{}' was denied",
                    grant.capability.name()
                ));
            }
            granted_names.insert(grant.capability.name().to_string());
            granted_caps.push(grant.capability.clone());
        }

        for cap in required {
            if !granted_names.contains(cap.name()) {
                return Err(format!("missing capability grant for '{}'", cap.name()));
            }
        }

        Ok(granted_caps)
    }

    /// Verify the signature and claims of a capability grant.
    ///
    /// Checks:
    /// - Ed25519 signature over the JWT header + claims.
    /// - Token is not expired and not revoked.
    /// - Claims `sub` matches the grant's execution ID.
    /// - Claims `cap` matches the grant's capability name.
    pub fn verify_grant(&self, grant: &CapabilityGrant) -> Result<(), String> {
        let parts: Vec<&str> = grant.signature.split('.').collect();
        if parts.len() != 3 {
            return Err("invalid JWT format".into());
        }
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let signature_bytes = URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|e| format!("decode signature: {}", e))?;
        let signature_arr: [u8; 64] = signature_bytes
            .try_into()
            .map_err(|_| "signature length invalid".to_string())?;
        let signature = ed25519_dalek::Signature::from_bytes(&signature_arr);
        self.verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|e| format!("signature verification failed: {}", e))?;

        let claims_json = URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|e| format!("decode claims: {}", e))?;
        let claims: CapabilityClaims =
            serde_json::from_slice(&claims_json).map_err(|e| format!("parse claims: {}", e))?;

        if claims.exp < chrono::Utc::now().timestamp() {
            return Err("capability grant expired".into());
        }
        if self.is_revoked(&claims.jti) {
            return Err("capability grant revoked".into());
        }
        if claims.sub != grant.execution_id {
            return Err("execution ID mismatch".into());
        }
        if claims.cap != grant.capability.name() {
            return Err("capability name mismatch".into());
        }
        if !claims.grt {
            return Err("capability was denied".into());
        }
        Ok(())
    }
}
