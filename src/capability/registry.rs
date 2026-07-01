use super::keys::{create_and_persist_keypair, load_or_create_keypair};
use super::revocation::{load_revoked, persist_revocation, REVOCATION_DB_FILENAME};
use super::types::{CapabilityClaims, JwtHeader};
use super::verifier::CapabilityVerifier;
use crate::types::{Capability, CapabilityGrant, GrantAuthority};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const CAPABILITY_TOKEN_TTL_SECONDS: i64 = 3600;

/// Registry of all capability grants per execution.
pub struct CapabilityRegistry {
    pub(crate) grants: HashMap<String, Vec<CapabilityGrant>>,
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    /// Revoked JTIs, persisted to a SQLite table.
    revoked: Arc<Mutex<HashSet<String>>>,
    revoked_db_path: Option<PathBuf>,
}

impl CapabilityRegistry {
    /// Create a registry backed by the OS keyring, falling back to an
    /// encrypted on-disk seed. Returns an error only if neither keyring nor
    /// fallback can be established.
    pub fn try_new() -> Result<Self, String> {
        let data_dir = crate::paths::vee_data_dir();
        Self::try_new_with_paths(&data_dir, &data_dir)
    }

    /// Create a registry with explicit storage paths (for testing).
    pub fn try_new_with_paths(key_dir: &Path, revocation_dir: &Path) -> Result<Self, String> {
        Self::try_new_with_key_dir(key_dir, revocation_dir)
    }

    /// Create a registry backed by the OS keyring, falling back to an
    /// encrypted on-disk seed stored in the provided key directory.
    pub fn try_new_with_key_dir(key_dir: &Path, revocation_dir: &Path) -> Result<Self, String> {
        let (signing_key, verifying_key) = load_or_create_keypair(key_dir)?;
        let revoked_db_path = revocation_dir.join(REVOCATION_DB_FILENAME);
        let revoked = Arc::new(Mutex::new(load_revoked(&revoked_db_path)?));
        Ok(Self {
            grants: HashMap::new(),
            signing_key,
            verifying_key,
            revoked,
            revoked_db_path: Some(revoked_db_path),
        })
    }

    /// Create a registry with a deterministic in-memory key (for tests).
    pub fn new_with_seed(seed: [u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        Self {
            grants: HashMap::new(),
            signing_key,
            verifying_key,
            revoked: Arc::new(Mutex::new(HashSet::new())),
            revoked_db_path: None,
        }
    }

    /// Rotate the signing key and invalidate existing grants.
    /// The new key is persisted to the keyring / encrypted fallback.
    pub fn rotate_key(&mut self) -> Result<(), String> {
        let data_dir = crate::paths::vee_data_dir();
        let (signing_key, verifying_key) = create_and_persist_keypair(&data_dir)?;
        self.signing_key = signing_key;
        self.verifying_key = verifying_key;
        self.grants.clear();
        Ok(())
    }

    /// Rotate to a deterministic in-memory key (for tests).
    pub fn rotate_key_with_seed(&mut self, seed: [u8; 32]) {
        self.signing_key = SigningKey::from_bytes(&seed);
        self.verifying_key = self.signing_key.verifying_key();
        self.grants.clear();
    }

    /// Grant a capability for an execution.
    pub fn grant(
        &mut self,
        execution_id: &str,
        capability: Capability,
        granted_by: GrantAuthority,
        reason: Option<String>,
    ) -> CapabilityGrant {
        let token = self.issue_token(execution_id, capability.name(), true, &granted_by, &reason);
        let grant = CapabilityGrant {
            execution_id: execution_id.to_string(),
            capability,
            granted: true,
            granted_by,
            reason,
            timestamp: chrono::Utc::now(),
            signature: token,
        };
        self.grants
            .entry(execution_id.to_string())
            .or_default()
            .push(grant.clone());
        grant
    }

    /// Deny a capability.
    pub fn deny(
        &mut self,
        execution_id: &str,
        capability: Capability,
        granted_by: GrantAuthority,
        reason: String,
    ) -> CapabilityGrant {
        let token = self.issue_token(
            execution_id,
            capability.name(),
            false,
            &granted_by,
            &Some(reason.clone()),
        );
        let grant = CapabilityGrant {
            execution_id: execution_id.to_string(),
            capability,
            granted: false,
            granted_by,
            reason: Some(reason),
            timestamp: chrono::Utc::now(),
            signature: token,
        };
        self.grants
            .entry(execution_id.to_string())
            .or_default()
            .push(grant.clone());
        grant
    }

    /// Return a verifier backed by this registry's public key and revocation set.
    pub fn verifier(&self) -> CapabilityVerifier {
        CapabilityVerifier::from_signing_key(&self.signing_key)
            .with_revoked_set(self.revoked.clone())
    }

    /// Return the hex-encoded public key used to verify grants.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.verifying_key.to_bytes())
    }

    /// Verify that a capability is granted and signed.
    pub fn verify(&self, execution_id: &str, capability_name: &str) -> bool {
        let Some(grants) = self.grants.get(execution_id) else {
            return false;
        };
        let verifier = self.verifier();
        grants.iter().any(|g| {
            g.granted && g.capability.name() == capability_name && verifier.verify_grant(g).is_ok()
        })
    }

    /// Get all grants for an execution.
    pub fn get_grants(&self, execution_id: &str) -> Vec<CapabilityGrant> {
        self.grants.get(execution_id).cloned().unwrap_or_default()
    }

    /// Revoke a capability token by its JTI. Persisted to the revocation table.
    pub fn revoke(&mut self, jti: &str) -> Result<(), String> {
        self.revoked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(jti.to_string());
        if let Some(path) = &self.revoked_db_path {
            persist_revocation(path, jti)?;
        }
        Ok(())
    }

    /// Check whether a token JTI has been revoked.
    pub fn is_revoked(&self, jti: &str) -> bool {
        self.revoked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(jti)
    }

    /// Parse string capabilities into typed capabilities.
    pub fn parse_capabilities(caps: &[String]) -> Vec<Capability> {
        caps.iter()
            .filter_map(|s| {
                let (name, value) = s
                    .split_once(':')
                    .map(|(name, value)| (name.trim(), Some(value.trim())))
                    .unwrap_or((s.trim(), None));

                match name {
                    "filesystem_read" => Some(Capability::FilesystemRead {
                        paths: parse_csv(value),
                    }),
                    "filesystem_write" => Some(Capability::FilesystemWrite {
                        paths: parse_csv(value),
                    }),
                    "filesystem_create" => Some(Capability::FilesystemCreate {
                        paths: parse_csv(value),
                    }),
                    "network_access" => Some(Capability::NetworkAccess {
                        hosts: parse_csv(value),
                        ports: vec![],
                    }),
                    "network_dns" => Some(Capability::NetworkDns),
                    "process_spawn" => Some(Capability::ProcessSpawn),
                    "environment_read" => Some(Capability::EnvironmentRead),
                    "environment_write" => Some(Capability::EnvironmentWrite),
                    "gpu_compute" => Some(Capability::GpuCompute { device: None }),
                    "browser_automation" => Some(Capability::BrowserAutomation),
                    "ssme_read" => Some(Capability::SsmeRead),
                    "ssme_write" => Some(Capability::SsmeWrite),
                    "belief_graph_query" => Some(Capability::BeliefGraphQuery),
                    "inference_provider" => value.map(|provider| Capability::InferenceProvider {
                        provider: provider.to_string(),
                    }),
                    _ => None,
                }
            })
            .collect()
    }

    fn issue_token(
        &self,
        execution_id: &str,
        capability_name: &str,
        granted: bool,
        authority: &GrantAuthority,
        reason: &Option<String>,
    ) -> String {
        let now = chrono::Utc::now().timestamp();
        let jti = format!("{}-{}", execution_id, uuid::Uuid::new_v4());
        let header = JwtHeader {
            alg: "EdDSA".to_string(),
            typ: "JWT".to_string(),
        };
        let claims = CapabilityClaims {
            sub: execution_id.to_string(),
            cap: capability_name.to_string(),
            grt: granted,
            aud: format!("{:?}", authority).to_lowercase(),
            rsn: reason.clone(),
            jti,
            iat: now,
            exp: now + CAPABILITY_TOKEN_TTL_SECONDS,
        };

        let header_json = serde_json::to_string(&header).unwrap_or_default();
        let claims_json = serde_json::to_string(&claims).unwrap_or_default();
        let header_b64 = URL_SAFE_NO_PAD.encode(header_json.as_bytes());
        let claims_b64 = URL_SAFE_NO_PAD.encode(claims_json.as_bytes());
        let signing_input = format!("{}.{}", header_b64, claims_b64);
        let signature = self.signing_key.sign(signing_input.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());
        format!("{}.{}.{}", header_b64, claims_b64, sig_b64)
    }
}

fn parse_csv(value: Option<&str>) -> Vec<String> {
    value
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
