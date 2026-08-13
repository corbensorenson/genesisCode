use std::path::PathBuf;

use super::{AuthorizedOptionalBool, AuthorizedOptionalString, AuthorizedStringList};

#[derive(Debug, Clone)]
pub struct StorePolicy {
    /// Content-addressed store directory used by `core/store::*` capabilities.
    pub dir: Option<PathBuf>,

    /// Optional remote registry base used as a read-through source for `core/store::{has,get}`.
    ///
    /// This is secure-by-default: if `remote` is set, the runner still requires `remote_allow`
    /// to be non-empty and to allow the normalized base URL prefix.
    pub remote: Option<String>,

    /// Allowlist of remote base URL prefixes permitted for `store.remote`.
    pub remote_allow: Vec<String>,

    /// If true, `http://` remotes are permitted (default false).
    pub allow_http: bool,

    /// Optional cumulative per-run byte budget for content-addressed store writes.
    pub max_run_bytes: Option<usize>,

    /// Optional bearer token presented to remote registries.
    pub auth_token: Option<String>,

    /// Optional env var name containing bearer token for remote registries.
    pub auth_token_env: Option<String>,

    /// Optional username for HTTP basic auth against remote registries.
    pub basic_username: Option<String>,

    /// Optional inline password for HTTP basic auth.
    pub basic_password: Option<String>,

    /// Optional env var name containing HTTP basic auth password.
    pub basic_password_env: Option<String>,

    /// Optional PEM path for additional trusted CA roots used by remote TLS.
    pub mtls_ca_pem: Option<PathBuf>,

    /// Optional PEM path for client identity used by mTLS.
    pub mtls_identity_pem: Option<PathBuf>,

    /// Closed GenesisCode decision for global store remote selection and admission.
    pub(crate) authorized_remote: Option<AuthorizedStoreRemotePolicy>,

    /// Closed GenesisCode decision for global store credentials and TLS material.
    pub(crate) authorized_credentials: Option<AuthorizedStoreCredentials>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthorizedStoreRemotePolicy {
    pub remote: AuthorizedOptionalString,
    pub remote_allow: AuthorizedStringList,
    pub allow_http: AuthorizedOptionalBool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuthorizedSecretSource {
    Absent,
    Inline(String),
    Environment(String),
    ImplicitEmpty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthorizedStoreCredentialError {
    AuthTokenInvalidType,
    AuthTokenEnvInvalidType,
    BasicUsernameInvalidType,
    BasicPasswordInvalidType,
    BasicPasswordEnvInvalidType,
    MtlsCaPemInvalidType,
    MtlsIdentityPemInvalidType,
    AuthTokenSourceConflict,
    BasicPasswordSourceConflict,
    BearerBasicConflict,
    PasswordWithoutUsername,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuthorizedStoreCredentials {
    Invalid(AuthorizedStoreCredentialError),
    Valid {
        bearer: AuthorizedSecretSource,
        basic_username: Option<String>,
        basic_password: AuthorizedSecretSource,
        mtls_ca_pem: Option<PathBuf>,
        mtls_identity_pem: Option<PathBuf>,
    },
}
