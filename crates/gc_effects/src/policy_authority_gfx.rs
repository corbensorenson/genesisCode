use super::*;
use crate::policy::AuthorizedGfxProfile;

const PROFILE: &str = "first_party_profile";
const PROFILE_ALIAS: &str = "gfx_first_party_profile";
const RUNTIME_PROFILE: &str = "runtime_profile";
const RUNTIME_PROFILE_ALIAS: &str = "host_runtime_profile";

#[cfg(target_os = "wasi")]
pub(crate) fn production_default() -> AuthorizedGfxProfile {
    AuthorizedGfxProfile::Browser
}

#[cfg(all(not(target_os = "wasi"), feature = "gfx-desktop-backend"))]
pub(crate) fn production_default() -> AuthorizedGfxProfile {
    AuthorizedGfxProfile::Desktop
}

#[cfg(all(not(target_os = "wasi"), not(feature = "gfx-desktop-backend")))]
pub(crate) fn production_default() -> AuthorizedGfxProfile {
    AuthorizedGfxProfile::Interactive
}

fn profile_symbol(profile: AuthorizedGfxProfile) -> &'static str {
    match profile {
        AuthorizedGfxProfile::Headless => ":headless",
        AuthorizedGfxProfile::Interactive => ":interactive",
        AuthorizedGfxProfile::Desktop => ":desktop",
        AuthorizedGfxProfile::Browser => ":browser",
    }
}

pub(super) fn input(table: Option<&toml::value::Table>) -> Term {
    let get = |key| table.and_then(|table| table.get(key));
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":production-default")),
                Term::symbol(profile_symbol(production_default())),
            ),
            (
                TermOrdKey(Term::symbol(":profile")),
                network::optional_string_input(get(PROFILE)),
            ),
            (
                TermOrdKey(Term::symbol(":profile-alias")),
                network::optional_string_input(get(PROFILE_ALIAS)),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-profile")),
                network::optional_string_input(get(RUNTIME_PROFILE)),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-profile-alias")),
                network::optional_string_input(get(RUNTIME_PROFILE_ALIAS)),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn selected<'a>(
    table: Option<&'a BTreeMap<String, toml::Value>>,
    primary: &str,
    alias: &str,
) -> Option<&'a toml::Value> {
    table.and_then(|table| table.get(primary).or_else(|| table.get(alias)))
}

fn is_production(value: Option<&toml::Value>) -> bool {
    value.and_then(toml::Value::as_str).is_some_and(|raw| {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "production" | "prod" | "release"
        )
    })
}

pub(crate) fn legacy(policy: Option<&OpPolicy>) -> AuthorizedGfxProfile {
    let table = policy.map(|policy| &policy.extra);
    let profile = selected(table, PROFILE, PROFILE_ALIAS).and_then(toml::Value::as_str);
    match profile.map(|raw| raw.trim().to_ascii_lowercase()) {
        Some(value) if value == "interactive" => AuthorizedGfxProfile::Interactive,
        Some(value) if value == "desktop" => AuthorizedGfxProfile::Desktop,
        Some(value) if value == "browser" => AuthorizedGfxProfile::Browser,
        Some(value) if value == "headless" => AuthorizedGfxProfile::Headless,
        Some(value) if value == "production" || value == "prod" => production_default(),
        Some(_) => AuthorizedGfxProfile::Headless,
        None if is_production(selected(table, RUNTIME_PROFILE, RUNTIME_PROFILE_ALIAS)) => {
            production_default()
        }
        None => AuthorizedGfxProfile::Headless,
    }
}

pub(crate) fn decode(term: &Term, allowed: bool) -> Result<AuthorizedGfxProfile, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(AuthorizedGfxProfile::Headless)
        } else {
            Err(authority_error("denied result :gfx-policy must be nil"))
        };
    }
    match term {
        Term::Symbol(value) if value == ":headless" => Ok(AuthorizedGfxProfile::Headless),
        Term::Symbol(value) if value == ":interactive" => Ok(AuthorizedGfxProfile::Interactive),
        Term::Symbol(value) if value == ":desktop" => Ok(AuthorizedGfxProfile::Desktop),
        Term::Symbol(value) if value == ":browser" => Ok(AuthorizedGfxProfile::Browser),
        _ => Err(authority_error(
            "admitted result :gfx-policy must be a supported symbol",
        )),
    }
}
