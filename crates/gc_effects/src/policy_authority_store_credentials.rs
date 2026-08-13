use super::*;
use crate::policy::{
    AuthorizedSecretSource, AuthorizedStoreCredentialError, AuthorizedStoreCredentials, StorePolicy,
};

#[derive(Debug)]
struct RawCredentials {
    auth_token: Option<String>,
    auth_token_env: Option<String>,
    basic_username: Option<String>,
    basic_password: Option<String>,
    basic_password_env: Option<String>,
    mtls_ca_pem: Option<PathBuf>,
    mtls_identity_pem: Option<PathBuf>,
}

impl RawCredentials {
    fn from_table(table: Option<&toml::value::Table>) -> Self {
        let string = |key| {
            table
                .and_then(|table| table.get(key))
                .and_then(toml::Value::as_str)
                .map(str::to_string)
        };
        Self {
            auth_token: string("auth_token"),
            auth_token_env: string("auth_token_env"),
            basic_username: string("basic_username"),
            basic_password: string("basic_password"),
            basic_password_env: string("basic_password_env"),
            mtls_ca_pem: string("mtls_ca_pem").map(PathBuf::from),
            mtls_identity_pem: string("mtls_identity_pem").map(PathBuf::from),
        }
    }

    fn from_store(store: &StorePolicy) -> Self {
        Self {
            auth_token: store.auth_token.clone(),
            auth_token_env: store.auth_token_env.clone(),
            basic_username: store.basic_username.clone(),
            basic_password: store.basic_password.clone(),
            basic_password_env: store.basic_password_env.clone(),
            mtls_ca_pem: store.mtls_ca_pem.clone(),
            mtls_identity_pem: store.mtls_identity_pem.clone(),
        }
    }
}

const DETAIL_KEYS: [&str; 7] = [
    ":bearer-env",
    ":bearer-source",
    ":basic-password-env",
    ":basic-password-source",
    ":basic-username",
    ":mtls-ca-pem",
    ":mtls-identity-pem",
];

pub(super) fn operation_applies(op: &str) -> bool {
    matches!(
        op,
        "core/sync::pull" | "core/sync::push" | "core/pkg-low::publish"
    )
}

fn secret_input(value: Option<&toml::Value>) -> Term {
    match value {
        None => Term::Nil,
        Some(value) if value.is_str() => Term::symbol(":present"),
        Some(_) => Term::symbol(":invalid-type"),
    }
}

pub(in crate::policy) fn input(table: Option<&toml::value::Table>) -> Term {
    let get = |key| table.and_then(|table| table.get(key));
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":auth-token")),
                secret_input(get("auth_token")),
            ),
            (
                TermOrdKey(Term::symbol(":auth-token-env")),
                network::optional_string_input(get("auth_token_env")),
            ),
            (
                TermOrdKey(Term::symbol(":basic-password")),
                secret_input(get("basic_password")),
            ),
            (
                TermOrdKey(Term::symbol(":basic-password-env")),
                network::optional_string_input(get("basic_password_env")),
            ),
            (
                TermOrdKey(Term::symbol(":basic-username")),
                network::optional_string_input(get("basic_username")),
            ),
            (
                TermOrdKey(Term::symbol(":mtls-ca-pem")),
                network::optional_string_input(get("mtls_ca_pem")),
            ),
            (
                TermOrdKey(Term::symbol(":mtls-identity-pem")),
                network::optional_string_input(get("mtls_identity_pem")),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn invalid_string(table: Option<&toml::value::Table>, key: &str) -> bool {
    table
        .and_then(|table| table.get(key))
        .is_some_and(|value| !value.is_str())
}

fn present(table: Option<&toml::value::Table>, key: &str) -> bool {
    table.and_then(|table| table.get(key)).is_some()
}

pub(super) fn legacy(
    table: Option<&toml::value::Table>,
    raw: &StorePolicy,
) -> AuthorizedStoreCredentials {
    legacy_raw(table, &RawCredentials::from_store(raw))
}

pub(in crate::policy) fn legacy_operation(
    table: Option<&toml::value::Table>,
) -> AuthorizedStoreCredentials {
    legacy_raw(table, &RawCredentials::from_table(table))
}

fn legacy_raw(
    table: Option<&toml::value::Table>,
    raw: &RawCredentials,
) -> AuthorizedStoreCredentials {
    let malformed = [
        (
            "auth_token",
            AuthorizedStoreCredentialError::AuthTokenInvalidType,
        ),
        (
            "auth_token_env",
            AuthorizedStoreCredentialError::AuthTokenEnvInvalidType,
        ),
        (
            "basic_username",
            AuthorizedStoreCredentialError::BasicUsernameInvalidType,
        ),
        (
            "basic_password",
            AuthorizedStoreCredentialError::BasicPasswordInvalidType,
        ),
        (
            "basic_password_env",
            AuthorizedStoreCredentialError::BasicPasswordEnvInvalidType,
        ),
        (
            "mtls_ca_pem",
            AuthorizedStoreCredentialError::MtlsCaPemInvalidType,
        ),
        (
            "mtls_identity_pem",
            AuthorizedStoreCredentialError::MtlsIdentityPemInvalidType,
        ),
    ];
    for (key, error) in malformed {
        if invalid_string(table, key) {
            return AuthorizedStoreCredentials::Invalid(error);
        }
    }

    let auth_inline = present(table, "auth_token");
    let auth_env = present(table, "auth_token_env");
    let password_inline = present(table, "basic_password");
    let password_env = present(table, "basic_password_env");
    let username = present(table, "basic_username");
    if auth_inline && auth_env {
        return AuthorizedStoreCredentials::Invalid(
            AuthorizedStoreCredentialError::AuthTokenSourceConflict,
        );
    }
    if password_inline && password_env {
        return AuthorizedStoreCredentials::Invalid(
            AuthorizedStoreCredentialError::BasicPasswordSourceConflict,
        );
    }
    if (auth_inline || auth_env) && username {
        return AuthorizedStoreCredentials::Invalid(
            AuthorizedStoreCredentialError::BearerBasicConflict,
        );
    }
    if !username && (password_inline || password_env) {
        return AuthorizedStoreCredentials::Invalid(
            AuthorizedStoreCredentialError::PasswordWithoutUsername,
        );
    }

    let bearer = if auth_inline {
        AuthorizedSecretSource::Inline(raw.auth_token.clone().unwrap_or_default())
    } else if auth_env {
        AuthorizedSecretSource::Environment(raw.auth_token_env.clone().unwrap_or_default())
    } else {
        AuthorizedSecretSource::Absent
    };
    let basic_password = if username {
        if password_inline {
            AuthorizedSecretSource::Inline(raw.basic_password.clone().unwrap_or_default())
        } else if password_env {
            AuthorizedSecretSource::Environment(raw.basic_password_env.clone().unwrap_or_default())
        } else {
            AuthorizedSecretSource::ImplicitEmpty
        }
    } else {
        AuthorizedSecretSource::Absent
    };
    AuthorizedStoreCredentials::Valid {
        bearer,
        basic_username: raw.basic_username.clone(),
        basic_password,
        mtls_ca_pem: raw.mtls_ca_pem.clone(),
        mtls_identity_pem: raw.mtls_identity_pem.clone(),
    }
}

#[cfg(test)]
fn status_symbol(error: AuthorizedStoreCredentialError) -> &'static str {
    match error {
        AuthorizedStoreCredentialError::AuthTokenInvalidType => ":auth-token-invalid-type",
        AuthorizedStoreCredentialError::AuthTokenEnvInvalidType => ":auth-token-env-invalid-type",
        AuthorizedStoreCredentialError::BasicUsernameInvalidType => ":basic-username-invalid-type",
        AuthorizedStoreCredentialError::BasicPasswordInvalidType => ":basic-password-invalid-type",
        AuthorizedStoreCredentialError::BasicPasswordEnvInvalidType => {
            ":basic-password-env-invalid-type"
        }
        AuthorizedStoreCredentialError::MtlsCaPemInvalidType => ":mtls-ca-pem-invalid-type",
        AuthorizedStoreCredentialError::MtlsIdentityPemInvalidType => {
            ":mtls-identity-pem-invalid-type"
        }
        AuthorizedStoreCredentialError::AuthTokenSourceConflict => ":auth-token-source-conflict",
        AuthorizedStoreCredentialError::BasicPasswordSourceConflict => {
            ":basic-password-source-conflict"
        }
        AuthorizedStoreCredentialError::BearerBasicConflict => ":bearer-basic-conflict",
        AuthorizedStoreCredentialError::PasswordWithoutUsername => ":password-without-username",
    }
}

fn status_error(status: &str) -> Option<AuthorizedStoreCredentialError> {
    Some(match status {
        ":auth-token-invalid-type" => AuthorizedStoreCredentialError::AuthTokenInvalidType,
        ":auth-token-env-invalid-type" => AuthorizedStoreCredentialError::AuthTokenEnvInvalidType,
        ":basic-username-invalid-type" => AuthorizedStoreCredentialError::BasicUsernameInvalidType,
        ":basic-password-invalid-type" => AuthorizedStoreCredentialError::BasicPasswordInvalidType,
        ":basic-password-env-invalid-type" => {
            AuthorizedStoreCredentialError::BasicPasswordEnvInvalidType
        }
        ":mtls-ca-pem-invalid-type" => AuthorizedStoreCredentialError::MtlsCaPemInvalidType,
        ":mtls-identity-pem-invalid-type" => {
            AuthorizedStoreCredentialError::MtlsIdentityPemInvalidType
        }
        ":auth-token-source-conflict" => AuthorizedStoreCredentialError::AuthTokenSourceConflict,
        ":basic-password-source-conflict" => {
            AuthorizedStoreCredentialError::BasicPasswordSourceConflict
        }
        ":bearer-basic-conflict" => AuthorizedStoreCredentialError::BearerBasicConflict,
        ":password-without-username" => AuthorizedStoreCredentialError::PasswordWithoutUsername,
        _ => return None,
    })
}

#[cfg(test)]
fn path_term(path: Option<&Path>) -> Result<Term, EffectsError> {
    match path {
        None => Ok(Term::Nil),
        Some(path) => path
            .to_str()
            .map(|path| Term::Str(path.to_string()))
            .ok_or_else(|| authority_error("store credential path must be valid UTF-8")),
    }
}

#[cfg(test)]
pub(in crate::policy) fn term(policy: &AuthorizedStoreCredentials) -> Result<Term, EffectsError> {
    let (status, bearer_source, bearer_env, basic_username, password_source, password_env, ca, id) =
        match policy {
            AuthorizedStoreCredentials::Invalid(error) => (
                status_symbol(*error),
                Term::Nil,
                Term::Nil,
                Term::Nil,
                Term::Nil,
                Term::Nil,
                Term::Nil,
                Term::Nil,
            ),
            AuthorizedStoreCredentials::Valid {
                bearer,
                basic_username,
                basic_password,
                mtls_ca_pem,
                mtls_identity_pem,
            } => {
                let (bearer_source, bearer_env) = match bearer {
                    AuthorizedSecretSource::Absent => (Term::symbol(":none"), Term::Nil),
                    AuthorizedSecretSource::Inline(_) => (Term::symbol(":inline"), Term::Nil),
                    AuthorizedSecretSource::Environment(name) => {
                        (Term::symbol(":environment"), Term::Str(name.clone()))
                    }
                    AuthorizedSecretSource::ImplicitEmpty => {
                        return Err(authority_error(
                            "bearer credential cannot use implicit-empty source",
                        ));
                    }
                };
                let (password_source, password_env) = match basic_password {
                    AuthorizedSecretSource::Absent => (Term::symbol(":none"), Term::Nil),
                    AuthorizedSecretSource::Inline(_) => (Term::symbol(":inline"), Term::Nil),
                    AuthorizedSecretSource::Environment(name) => {
                        (Term::symbol(":environment"), Term::Str(name.clone()))
                    }
                    AuthorizedSecretSource::ImplicitEmpty => {
                        (Term::symbol(":implicit-empty"), Term::Nil)
                    }
                };
                (
                    ":valid",
                    bearer_source,
                    bearer_env,
                    basic_username
                        .as_ref()
                        .map(|value| Term::Str(value.clone()))
                        .unwrap_or(Term::Nil),
                    password_source,
                    password_env,
                    path_term(mtls_ca_pem.as_deref())?,
                    path_term(mtls_identity_pem.as_deref())?,
                )
            }
        };
    Ok(Term::Map(
        [
            (TermOrdKey(Term::symbol(":bearer-env")), bearer_env),
            (TermOrdKey(Term::symbol(":bearer-source")), bearer_source),
            (
                TermOrdKey(Term::symbol(":basic-password-env")),
                password_env,
            ),
            (
                TermOrdKey(Term::symbol(":basic-password-source")),
                password_source,
            ),
            (TermOrdKey(Term::symbol(":basic-username")), basic_username),
            (TermOrdKey(Term::symbol(":mtls-ca-pem")), ca),
            (TermOrdKey(Term::symbol(":mtls-identity-pem")), id),
            (TermOrdKey(Term::symbol(":status")), Term::symbol(status)),
        ]
        .into_iter()
        .collect(),
    ))
}

fn field<'a>(map: &'a BTreeMap<TermOrdKey, Term>, key: &str) -> Result<&'a Term, EffectsError> {
    map.get(&TermOrdKey(Term::symbol(key))).ok_or_else(|| {
        authority_error(format!(
            "resource result :store :credential-policy is missing {key}"
        ))
    })
}

fn optional_string(term: &Term, field_name: &str) -> Result<Option<String>, EffectsError> {
    match term {
        Term::Nil => Ok(None),
        Term::Str(value) => Ok(Some(value.clone())),
        _ => Err(authority_error(format!(
            "resource result :store :credential-policy {field_name} must be nil or a string"
        ))),
    }
}

pub(in crate::policy) fn decode(
    term: &Term,
    raw: &StorePolicy,
) -> Result<AuthorizedStoreCredentials, EffectsError> {
    decode_raw(
        term,
        &RawCredentials::from_store(raw),
        "resource result :store",
    )
}

pub(in crate::policy) fn decode_operation(
    term: &Term,
    table: Option<&toml::value::Table>,
) -> Result<AuthorizedStoreCredentials, EffectsError> {
    decode_raw(term, &RawCredentials::from_table(table), "operation result")
}

fn decode_raw(
    term: &Term,
    raw: &RawCredentials,
    scope: &str,
) -> Result<AuthorizedStoreCredentials, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error(format!(
            "{scope} :credential-policy must be a data map"
        )));
    };
    let expected: BTreeSet<_> = DETAIL_KEYS
        .into_iter()
        .chain([":status"])
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(format!(
            "{scope} :credential-policy field set mismatch"
        )));
    }
    let status = match field(map, ":status")? {
        Term::Symbol(status) => status.as_str(),
        _ => {
            return Err(authority_error(
                "resource result :store :credential-policy status must be a symbol",
            ));
        }
    };
    if let Some(error) = status_error(status) {
        if DETAIL_KEYS
            .iter()
            .any(|key| field(map, key).is_ok_and(|value| value != &Term::Nil))
        {
            return Err(authority_error(
                "invalid store credential decision must carry nil detail fields",
            ));
        }
        return Ok(AuthorizedStoreCredentials::Invalid(error));
    }
    if status != ":valid" {
        return Err(authority_error(
            "resource result :store :credential-policy has unknown status",
        ));
    }

    let bearer_env = optional_string(field(map, ":bearer-env")?, ":bearer-env")?;
    let bearer = match field(map, ":bearer-source")? {
        Term::Symbol(source) if source == ":none" && bearer_env.is_none() => {
            AuthorizedSecretSource::Absent
        }
        Term::Symbol(source) if source == ":inline" && bearer_env.is_none() => {
            AuthorizedSecretSource::Inline(raw.auth_token.clone().ok_or_else(|| {
                authority_error("inline bearer decision has no retained inline token")
            })?)
        }
        Term::Symbol(source) if source == ":environment" => {
            let name = bearer_env.ok_or_else(|| {
                authority_error("environment bearer decision is missing its environment name")
            })?;
            if raw.auth_token_env.as_deref() != Some(name.as_str()) {
                return Err(authority_error(
                    "environment bearer decision substituted its environment name",
                ));
            }
            AuthorizedSecretSource::Environment(name)
        }
        _ => {
            return Err(authority_error(
                "resource result bearer source contradicts bearer environment",
            ));
        }
    };

    let basic_username = optional_string(field(map, ":basic-username")?, ":basic-username")?;
    if basic_username.as_deref() != raw.basic_username.as_deref() {
        return Err(authority_error(
            "store credential decision substituted the basic username",
        ));
    }
    let password_env = optional_string(field(map, ":basic-password-env")?, ":basic-password-env")?;
    let basic_password = match field(map, ":basic-password-source")? {
        Term::Symbol(source)
            if source == ":none" && password_env.is_none() && basic_username.is_none() =>
        {
            AuthorizedSecretSource::Absent
        }
        Term::Symbol(source)
            if source == ":inline" && password_env.is_none() && basic_username.is_some() =>
        {
            AuthorizedSecretSource::Inline(raw.basic_password.clone().ok_or_else(|| {
                authority_error("inline basic password decision has no retained password")
            })?)
        }
        Term::Symbol(source) if source == ":environment" && basic_username.is_some() => {
            let name = password_env.ok_or_else(|| {
                authority_error(
                    "environment basic password decision is missing its environment name",
                )
            })?;
            if raw.basic_password_env.as_deref() != Some(name.as_str()) {
                return Err(authority_error(
                    "environment basic password decision substituted its environment name",
                ));
            }
            AuthorizedSecretSource::Environment(name)
        }
        Term::Symbol(source)
            if source == ":implicit-empty"
                && password_env.is_none()
                && basic_username.is_some() =>
        {
            AuthorizedSecretSource::ImplicitEmpty
        }
        _ => {
            return Err(authority_error(
                "resource result basic password source contradicts its fields",
            ));
        }
    };

    let ca = optional_string(field(map, ":mtls-ca-pem")?, ":mtls-ca-pem")?;
    let id = optional_string(field(map, ":mtls-identity-pem")?, ":mtls-identity-pem")?;
    if ca.as_deref() != raw.mtls_ca_pem.as_deref().and_then(Path::to_str)
        || id.as_deref() != raw.mtls_identity_pem.as_deref().and_then(Path::to_str)
    {
        return Err(authority_error(
            "store credential decision substituted an mTLS path",
        ));
    }
    Ok(AuthorizedStoreCredentials::Valid {
        bearer,
        basic_username,
        basic_password,
        mtls_ca_pem: ca.map(PathBuf::from),
        mtls_identity_pem: id.map(PathBuf::from),
    })
}
