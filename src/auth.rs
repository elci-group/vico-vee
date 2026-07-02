//! API-key authentication and authorization middleware.
//!
//! Keys are loaded from `api_keys.toml` (or the `VICO_VEE_API_KEYS` env var)
//! and carry an optional scope set. Routes declare a required scope and the
//! middleware rejects requests with `401` / `403` when appropriate.

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// A single API key entry as stored in `api_keys.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub token: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// On-disk format for the API-keys file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiKeysFile {
    pub keys: HashMap<String, ApiKey>,
}

/// Runtime representation of loaded API keys.
#[derive(Debug, Clone)]
pub struct AuthKeys {
    keys: HashMap<String, ApiKeyEntry>,
    pub require_auth: bool,
}

#[derive(Debug, Clone)]
struct ApiKeyEntry {
    #[allow(dead_code)]
    name: String,
    scopes: HashSet<String>,
}

/// Errors returned by [`AuthKeys::check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    Missing,
    Invalid,
    Forbidden,
}

impl AuthKeys {
    /// Load keys from config: env override first, then the keys file.
    ///
    /// When `require_auth` is `true` and no keys are configured, this returns
    /// an error so the service fails closed instead of silently disabling auth.
    pub fn load(config: &crate::config::ApiKeysConfig) -> Result<Self, String> {
        let file = if let Some(env) = &config.env_override {
            let file: ApiKeysFile =
                toml::from_str(env).map_err(|e| format!("VICO_VEE_API_KEYS: {e}"))?;
            Some(file)
        } else if config.file.exists() {
            let text = std::fs::read_to_string(&config.file)
                .map_err(|e| format!("read {}: {}", config.file.display(), e))?;
            let file: ApiKeysFile = toml::from_str(&text)
                .map_err(|e| format!("parse {}: {}", config.file.display(), e))?;
            Some(file)
        } else {
            None
        };

        if let Some(file) = file {
            Self::from_file(file, config.require_auth)
        } else if config.require_auth {
            Err(format!(
                "authentication is required but no API keys were found at {}. \
                 Generate a key with: vico-vee --generate-admin-key {}",
                config.file.display(),
                config.file.display()
            ))
        } else {
            Ok(Self {
                keys: HashMap::new(),
                require_auth: false,
            })
        }
    }

    /// Build a set from an in-memory map. Useful for tests.
    pub fn from_map(keys: HashMap<String, ApiKey>, require_auth: bool) -> Self {
        Self::from_file(ApiKeysFile { keys }, require_auth)
            .expect("in-memory API keys are assumed valid")
    }

    fn from_file(file: ApiKeysFile, require_auth: bool) -> Result<Self, String> {
        let mut map = HashMap::with_capacity(file.keys.len());
        for (name, key) in file.keys {
            if key.token.is_empty() {
                return Err(format!("API key '{name}' has empty token"));
            }
            map.insert(
                key.token.clone(),
                ApiKeyEntry {
                    name,
                    scopes: key.scopes.iter().map(|s| s.to_lowercase()).collect(),
                },
            );
        }
        if require_auth && map.is_empty() {
            return Err(
                "authentication is required but the API-keys file contains no keys".to_string(),
            );
        }
        Ok(Self {
            keys: map,
            require_auth,
        })
    }

    /// Validate an `Authorization` header value against a required scope.
    pub fn check(&self, authorization: &str, required_scope: &str) -> Result<(), AuthError> {
        if !self.require_auth && self.keys.is_empty() {
            return Ok(());
        }

        let token = authorization
            .strip_prefix("Bearer ")
            .or_else(|| authorization.strip_prefix("bearer "))
            .unwrap_or(authorization)
            .trim();

        let entry = self.keys.get(token).ok_or(AuthError::Invalid)?;
        if entry.scopes.contains("admin") || entry.scopes.contains(required_scope) {
            Ok(())
        } else {
            Err(AuthError::Forbidden)
        }
    }
}

/// Returns the required scope for a route, or `None` if the route is public.
pub fn required_scope_for_path(path: &str) -> Option<&'static str> {
    match path {
        // Public metadata / health endpoints.
        "/health" | "/openapi.json" | "/docs" | "/ready" | "/metrics" => None,
        // Task submission and mutation.
        "/vee/submit" | "/vee/cancel" | "/vee/diff" | "/vee/merge" | "/vee/reject" => {
            Some("submit")
        }
        // Read-only queries and dashboards.
        "/vee/status" | "/vee/list" | "/vee/artifacts" | "/vee/dashboard" | "/vee/patterns"
        | "/vee/audit" | "/vee/checkpoints" => Some("read"),
        // Administrative ODIN control.
        "/vee/odin/health" | "/vee/odin/model" => Some("admin"),
        // Capability signing-key rotation.
        "/vee/admin/rotate-key" => Some("admin"),
        // Data lifecycle administration.
        "/admin/backup" | "/admin/restore" => Some("admin"),
        _ => Some("read"),
    }
}

/// axum middleware that enforces API-key scope requirements.
pub async fn auth_middleware(
    State(state): State<crate::server::AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_owned();
    let Some(scope) = required_scope_for_path(&path) else {
        return Ok(next.run(req).await);
    };

    let header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match header {
        None => {
            if state.auth_keys.require_auth {
                Err(StatusCode::UNAUTHORIZED)
            } else {
                Ok(next.run(req).await)
            }
        }
        Some(value) => {
            state.auth_keys.check(value, scope).map_err(|e| match e {
                AuthError::Missing | AuthError::Invalid => StatusCode::UNAUTHORIZED,
                AuthError::Forbidden => StatusCode::FORBIDDEN,
            })?;
            Ok(next.run(req).await)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_keys() -> AuthKeys {
        let mut keys = HashMap::new();
        keys.insert(
            "submit-only".to_string(),
            ApiKey {
                token: "submit-token".to_string(),
                scopes: vec!["submit".to_string()],
            },
        );
        keys.insert(
            "read-only".to_string(),
            ApiKey {
                token: "read-token".to_string(),
                scopes: vec!["read".to_string()],
            },
        );
        keys.insert(
            "admin-key".to_string(),
            ApiKey {
                token: "admin-token".to_string(),
                scopes: vec!["admin".to_string()],
            },
        );
        AuthKeys::from_map(keys, true)
    }

    #[test]
    fn auth_accepts_valid_scope() {
        let keys = test_keys();
        assert!(keys.check("Bearer submit-token", "submit").is_ok());
        assert!(keys.check("Bearer read-token", "read").is_ok());
        assert!(keys.check("Bearer admin-token", "submit").is_ok());
    }

    #[test]
    fn auth_rejects_missing_scope() {
        let keys = test_keys();
        assert_eq!(
            keys.check("Bearer read-token", "submit"),
            Err(AuthError::Forbidden)
        );
    }

    #[test]
    fn auth_rejects_invalid_token() {
        let keys = test_keys();
        assert_eq!(
            keys.check("Bearer bad-token", "read"),
            Err(AuthError::Invalid)
        );
    }

    #[test]
    fn auth_rejects_missing_header_when_required() {
        let keys = test_keys();
        assert_eq!(keys.check("", "read"), Err(AuthError::Invalid));
    }

    #[test]
    fn public_routes_have_no_scope() {
        assert!(required_scope_for_path("/health").is_none());
        assert!(required_scope_for_path("/openapi.json").is_none());
        assert!(required_scope_for_path("/docs").is_none());
    }

    #[test]
    fn scoped_routes_require_scope() {
        assert_eq!(required_scope_for_path("/vee/submit"), Some("submit"));
        assert_eq!(required_scope_for_path("/vee/dashboard"), Some("read"));
        assert_eq!(required_scope_for_path("/vee/odin/model"), Some("admin"));
    }

    #[test]
    fn load_fails_closed_when_auth_required_and_no_keys() {
        let config = crate::config::ApiKeysConfig {
            file: PathBuf::from("/does/not/exist/api_keys.toml"),
            env_override: None,
            require_auth: true,
        };
        assert!(AuthKeys::load(&config).is_err());
    }

    #[test]
    fn load_allows_no_keys_when_auth_not_required() {
        let config = crate::config::ApiKeysConfig {
            file: PathBuf::from("/does/not/exist/api_keys.toml"),
            env_override: None,
            require_auth: false,
        };
        let keys = AuthKeys::load(&config).unwrap();
        assert!(!keys.require_auth);
        assert!(keys.keys.is_empty());
    }

    #[test]
    fn load_fails_when_keys_file_is_empty_and_auth_required() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("api_keys.toml");
        std::fs::write(&path, "[keys]\n").unwrap();
        let config = crate::config::ApiKeysConfig {
            file: path,
            env_override: None,
            require_auth: true,
        };
        assert!(AuthKeys::load(&config).is_err());
    }
}
