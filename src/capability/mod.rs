//! Capability Registry
//!
//! Cryptographically-signed capability grants. The orchestrator signs
//! capabilities before vicod accepts them, preventing privilege escalation.
//! Signing keys are derived from the OS keyring (Secret Service / Keychain /
//! Windows Credential Manager) with an encrypted on-disk fallback. Grants are
//! issued as short-lived Ed25519-signed JWTs and can be revoked.

pub mod keys;
pub mod registry;
pub mod revocation;
pub mod types;
pub mod verifier;

pub use registry::CapabilityRegistry;
pub use verifier::CapabilityVerifier;

// Re-export so callers that historically imported these from `capability` keep working.
pub use crate::types::{CapabilityGrant, GrantAuthority};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
