use crate::policy::{
    AuthorizedSecretSource, AuthorizedStoreCredentialError, AuthorizedStoreCredentials, CapsPolicy,
};

use super::{read_pem_path, resolve_auth_token, resolve_basic_password};

fn store_credential_error(error: AuthorizedStoreCredentialError) -> String {
    match error {
        AuthorizedStoreCredentialError::AuthTokenInvalidType => {
            "store.auth_token must be a string".to_string()
        }
        AuthorizedStoreCredentialError::AuthTokenEnvInvalidType => {
            "store.auth_token_env must be a string".to_string()
        }
        AuthorizedStoreCredentialError::BasicUsernameInvalidType => {
            "store.basic_username must be a string".to_string()
        }
        AuthorizedStoreCredentialError::BasicPasswordInvalidType => {
            "store.basic_password must be a string".to_string()
        }
        AuthorizedStoreCredentialError::BasicPasswordEnvInvalidType => {
            "store.basic_password_env must be a string".to_string()
        }
        AuthorizedStoreCredentialError::MtlsCaPemInvalidType => {
            "store.mtls_ca_pem must be a string".to_string()
        }
        AuthorizedStoreCredentialError::MtlsIdentityPemInvalidType => {
            "store.mtls_identity_pem must be a string".to_string()
        }
        AuthorizedStoreCredentialError::AuthTokenSourceConflict => {
            "auth_token and auth_token_env are mutually exclusive".to_string()
        }
        AuthorizedStoreCredentialError::BasicPasswordSourceConflict => {
            "basic_password and basic_password_env are mutually exclusive".to_string()
        }
        AuthorizedStoreCredentialError::BearerBasicConflict => {
            "auth_token/auth_token_env and basic_username are mutually exclusive".to_string()
        }
        AuthorizedStoreCredentialError::PasswordWithoutUsername => {
            "basic_password/basic_password_env requires basic_username".to_string()
        }
    }
}

fn resolve_authorized_bearer(source: &AuthorizedSecretSource) -> Result<Option<String>, String> {
    match source {
        AuthorizedSecretSource::Absent => resolve_auth_token(None, None),
        AuthorizedSecretSource::Inline(secret) => resolve_auth_token(Some(secret), None),
        AuthorizedSecretSource::Environment(name) => resolve_auth_token(None, Some(name)),
        AuthorizedSecretSource::ImplicitEmpty => {
            Err("bearer token cannot use an implicit empty credential".to_string())
        }
    }
}

fn resolve_authorized_password(source: &AuthorizedSecretSource) -> Result<Option<String>, String> {
    match source {
        AuthorizedSecretSource::Absent => resolve_basic_password(None, None),
        AuthorizedSecretSource::Inline(secret) => resolve_basic_password(Some(secret), None),
        AuthorizedSecretSource::Environment(name) => resolve_basic_password(None, Some(name)),
        AuthorizedSecretSource::ImplicitEmpty => Ok(Some(String::new())),
    }
}

pub(in crate::runner) fn store_registry_auth(
    policy: &CapsPolicy,
) -> Result<gc_registry::RegistryAuth, String> {
    let authorized = policy
        .authorized_store_credentials()
        .ok_or_else(|| "global store credential policy authority is missing".to_string())?;
    let (bearer, basic_username, basic_password, mtls_ca_pem, mtls_identity_pem) =
        match authorized {
            AuthorizedStoreCredentials::Invalid(error) => {
                return Err(store_credential_error(*error));
            }
            AuthorizedStoreCredentials::Valid {
                bearer,
                basic_username,
                basic_password,
                mtls_ca_pem,
                mtls_identity_pem,
            } => (
                bearer,
                basic_username,
                basic_password,
                mtls_ca_pem,
                mtls_identity_pem,
            ),
        };
    let bearer_token = resolve_authorized_bearer(bearer)?;
    let basic_password = resolve_authorized_password(basic_password)?;
    let mtls_ca_pem = match mtls_ca_pem.as_deref() {
        Some(path) => Some(read_pem_path(path)?),
        None => None,
    };
    let mtls_identity_pem = match mtls_identity_pem.as_deref() {
        Some(path) => Some(read_pem_path(path)?),
        None => None,
    };
    Ok(gc_registry::RegistryAuth {
        bearer_token,
        basic_username: basic_username.clone(),
        basic_password,
        mtls_ca_pem,
        mtls_identity_pem,
    })
}
