use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtHeader {
    pub alg: String,
    pub typ: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CapabilityClaims {
    /// Execution ID (JWT `sub`).
    pub sub: String,
    /// Capability name.
    pub cap: String,
    /// Granted flag.
    pub grt: bool,
    /// Granting authority.
    pub aud: String,
    /// Optional reason.
    pub rsn: Option<String>,
    /// Unique token id.
    pub jti: String,
    /// Issued at (unix timestamp).
    pub iat: i64,
    /// Expiration (unix timestamp).
    pub exp: i64,
}
