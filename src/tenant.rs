//! Multi-tenancy — project isolation for the standalone `vico-vee` service.
//!
//! Every request belongs to a project identified by the `x-vee-project` header.
//! When the header is absent the request falls back to the `default` project.
//! Projects are isolated at the artifact and execution layers: a client in one
//! project cannot read artifacts or executions from another project.

use axum::extract::FromRequestParts;
use axum::http::{request::Parts, StatusCode};

/// Header used by clients to identify the target project.
pub const PROJECT_HEADER: &str = "x-vee-project";

/// Project used when no header is supplied.
pub const DEFAULT_PROJECT: &str = "default";

/// Maximum length of a project identifier.
const MAX_PROJECT_ID_LEN: usize = 64;

/// Project context extracted from an incoming request.
#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub project_id: String,
}

impl ProjectContext {
    /// Create a context for the default project.
    pub fn default_project() -> Self {
        Self {
            project_id: DEFAULT_PROJECT.into(),
        }
    }
}

impl Default for ProjectContext {
    fn default() -> Self {
        Self::default_project()
    }
}

impl<S> FromRequestParts<S> for ProjectContext
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let raw = parts
            .headers
            .get(PROJECT_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(DEFAULT_PROJECT);

        match validate_project_id(raw) {
            Ok(id) => Ok(Self { project_id: id }),
            Err(_) => Err(StatusCode::BAD_REQUEST),
        }
    }
}

/// Validate and normalise a project identifier.
///
/// Rules:
/// - non-empty
/// - at most 64 characters
/// - only ASCII letters, digits, hyphens, underscores, and dots
fn validate_project_id(raw: &str) -> Result<String, String> {
    let id = raw.trim();
    if id.is_empty() {
        return Err("project id is empty".into());
    }
    if id.len() > MAX_PROJECT_ID_LEN {
        return Err(format!("project id exceeds {MAX_PROJECT_ID_LEN} characters"));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err("project id contains invalid characters".into());
    }
    Ok(id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_project_is_default() {
        assert_eq!(ProjectContext::default().project_id, "default");
    }

    #[test]
    fn validate_accepts_simple_names() {
        assert_eq!(validate_project_id("acme").unwrap(), "acme");
        assert_eq!(validate_project_id("acme-corp-01").unwrap(), "acme-corp-01");
        assert_eq!(validate_project_id("acme.corp_01").unwrap(), "acme.corp_01");
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(validate_project_id("").is_err());
        assert!(validate_project_id("   ").is_err());
    }

    #[test]
    fn validate_rejects_too_long() {
        let long = "a".repeat(MAX_PROJECT_ID_LEN + 1);
        assert!(validate_project_id(&long).is_err());
    }

    #[test]
    fn validate_rejects_special_characters() {
        assert!(validate_project_id("acme/corp").is_err());
        assert!(validate_project_id("acme corp").is_err());
        assert!(validate_project_id("acme@corp").is_err());
    }
}
