use super::*;

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::types::CapabilityClaims;
    use super::*;
    use crate::types::Capability;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    #[test]
    fn test_registry_grant_and_verify_with_jwt() {
        let mut registry = CapabilityRegistry::new_with_seed([7u8; 32]);
        let cap = Capability::FilesystemRead {
            paths: vec!["/workspace".into()],
        };
        registry.grant(
            "exec-001",
            cap.clone(),
            GrantAuthority::Orchestrator,
            Some("test".into()),
        );
        assert!(registry.verify("exec-001", "filesystem_read"));
        assert!(!registry.verify("exec-001", "network_access"));
        assert!(!registry.verify("exec-002", "filesystem_read"));
    }

    #[test]
    fn test_capability_registry_parse() {
        let caps = vec![
            "filesystem_read".into(),
            "network_access".into(),
            "unknown_cap".into(),
        ];
        let parsed = CapabilityRegistry::parse_capabilities(&caps);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name(), "filesystem_read");
        assert_eq!(parsed[1].name(), "network_access");
    }

    #[test]
    fn test_capability_registry_parse_scoped_values() {
        let caps = vec![
            "filesystem_read:/home/sal/goten,/home/sal/egor".into(),
            "filesystem_write:/tmp/vico-out".into(),
            "inference_provider:ollama".into(),
        ];
        let parsed = CapabilityRegistry::parse_capabilities(&caps);

        assert_eq!(
            parsed[0],
            Capability::FilesystemRead {
                paths: vec!["/home/sal/goten".into(), "/home/sal/egor".into()]
            }
        );
        assert_eq!(
            parsed[1],
            Capability::FilesystemWrite {
                paths: vec!["/tmp/vico-out".into()]
            }
        );
        assert_eq!(
            parsed[2],
            Capability::InferenceProvider {
                provider: "ollama".into()
            }
        );
    }

    #[test]
    fn test_jwt_tamper_detection() {
        let mut registry = CapabilityRegistry::new_with_seed([9u8; 32]);
        let cap = Capability::NetworkAccess {
            hosts: vec!["example.com".into()],
            ports: vec![443],
        };
        let grant = registry.grant("exec-003", cap, GrantAuthority::System, None);
        assert!(registry.verify("exec-003", "network_access"));

        // Tamper with the signed JWT so the signature no longer verifies.
        let mut tampered = grant;
        tampered.signature =
            tampered.signature[..tampered.signature.len().saturating_sub(1)].to_string();
        registry
            .grants
            .insert("exec-003".into(), vec![tampered.clone()]);
        assert!(!registry.verify("exec-003", "network_access"));

        // A token with mismatched claims (different capability) should also fail.
        let parts: Vec<&str> = tampered.signature.split('.').collect();
        if parts.len() == 3 {
            let claims_bytes = URL_SAFE_NO_PAD.decode(parts[1]).expect("decode claims");
            let mut claims: CapabilityClaims =
                serde_json::from_slice(&claims_bytes).expect("parse claims");
            claims.cap = "filesystem_read".to_string();
            let tampered_claims_b64 =
                URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims).unwrap().as_bytes());
            let forged_signature = format!("{}.{}.{}", parts[0], tampered_claims_b64, parts[2]);
            let mut forged = tampered.clone();
            forged.signature = forged_signature;
            forged.capability = Capability::FilesystemRead { paths: vec![] };
            registry.grants.insert("exec-003".into(), vec![forged]);
            assert!(!registry.verify("exec-003", "filesystem_read"));
        }
    }

    #[test]
    fn test_revocation() {
        let mut registry = CapabilityRegistry::new_with_seed([11u8; 32]);
        let cap = Capability::ProcessSpawn;
        let grant = registry.grant("exec-004", cap, GrantAuthority::Human, None);
        assert!(registry.verify("exec-004", "process_spawn"));

        let jti = extract_jti(&grant.signature).expect("jti present");
        registry.revoke(&jti).unwrap();
        assert!(registry.is_revoked(&jti));
        assert!(!registry.verify("exec-004", "process_spawn"));
    }

    #[test]
    fn test_rotate_key_invalidate_grants() {
        let mut registry = CapabilityRegistry::new_with_seed([13u8; 32]);
        registry.grant(
            "exec-005",
            Capability::EnvironmentRead,
            GrantAuthority::Orchestrator,
            None,
        );
        assert!(registry.verify("exec-005", "environment_read"));

        registry.rotate_key_with_seed([17u8; 32]);
        assert!(registry.grants.get("exec-005").is_none_or(|v| v.is_empty()));
        assert!(!registry.verify("exec-005", "environment_read"));
    }

    fn extract_jti(token: &str) -> Option<String> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        let bytes = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
        let claims: CapabilityClaims = serde_json::from_slice(&bytes).ok()?;
        Some(claims.jti)
    }
}
