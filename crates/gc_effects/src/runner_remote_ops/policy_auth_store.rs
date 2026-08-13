use crate::policy::{
    AuthorizedSecretSource, AuthorizedStoreCredentialError, AuthorizedStoreCredentials, CapsPolicy,
};

use super::{read_pem_path, resolve_auth_token, resolve_basic_password};

fn credential_error(error: AuthorizedStoreCredentialError, field_prefix: &str) -> String {
    match error {
        AuthorizedStoreCredentialError::AuthTokenInvalidType => {
            format!("{field_prefix}auth_token must be a string")
        }
        AuthorizedStoreCredentialError::AuthTokenEnvInvalidType => {
            format!("{field_prefix}auth_token_env must be a string")
        }
        AuthorizedStoreCredentialError::BasicUsernameInvalidType => {
            format!("{field_prefix}basic_username must be a string")
        }
        AuthorizedStoreCredentialError::BasicPasswordInvalidType => {
            format!("{field_prefix}basic_password must be a string")
        }
        AuthorizedStoreCredentialError::BasicPasswordEnvInvalidType => {
            format!("{field_prefix}basic_password_env must be a string")
        }
        AuthorizedStoreCredentialError::MtlsCaPemInvalidType => {
            format!("{field_prefix}mtls_ca_pem must be a string")
        }
        AuthorizedStoreCredentialError::MtlsIdentityPemInvalidType => {
            format!("{field_prefix}mtls_identity_pem must be a string")
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

pub(super) fn registry_auth_from_authority(
    authorized: &AuthorizedStoreCredentials,
    field_prefix: &str,
) -> Result<gc_registry::RegistryAuth, String> {
    let (bearer, basic_username, basic_password, mtls_ca_pem, mtls_identity_pem) =
        match authorized {
            AuthorizedStoreCredentials::Invalid(error) => {
                return Err(credential_error(*error, field_prefix));
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

pub(in crate::runner) fn store_registry_auth(
    policy: &CapsPolicy,
) -> Result<gc_registry::RegistryAuth, String> {
    let authorized = policy
        .authorized_store_credentials()
        .ok_or_else(|| "global store credential policy authority is missing".to_string())?;
    registry_auth_from_authority(authorized, "store.")
}
