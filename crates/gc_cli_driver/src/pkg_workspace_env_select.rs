use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, Value};
use gc_pkg::WorkspaceConfig;

const AUTHORITY_BINDING: &str = "core/pkg::workspace-env-select-authority";
const REQUEST_KIND: &str = "genesis/pkg-workspace-env-select-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-workspace-env-select-authority-result-v0.1";

pub(super) struct AuthorizedRuntimeBackend {
    pub(super) selected: String,
    pub(super) compatible: bool,
}

pub(super) struct EnvWorkspace {
    pub(super) config: WorkspaceConfig,
    pub(super) default_runtime_backend: Option<String>,
    pub(super) profile_runtime_backend: Option<String>,
}

pub(super) fn load_workspace(path: &Path, selected_profile: &str) -> Result<EnvWorkspace, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let mut document = source
        .parse::<toml::Table>()
        .map_err(|error| format!("workspace parse error: {}: {error}", path.display()))?;

    let default_runtime_backend = take_runtime_backend(
        document
            .get_mut("defaults")
            .and_then(toml::Value::as_table_mut),
        "defaults",
    )?;
    let mut profile_runtime_backends = BTreeMap::new();
    if let Some(profiles) = document
        .get_mut("profiles")
        .and_then(toml::Value::as_table_mut)
    {
        for (name, profile) in profiles {
            let runtime_backend =
                take_runtime_backend(profile.as_table_mut(), &format!("profile `{name}`"))?;
            profile_runtime_backends.insert(name.clone(), runtime_backend);
        }
    }

    // The shared parser still owns structural workspace admission. Backend values
    // are restored only after it has stopped acting as the selection oracle.
    let structural_source = toml::to_string(&document)
        .map_err(|error| format!("workspace observation serialization failed: {error}"))?;
    let mut config = WorkspaceConfig::from_toml_str(path, &structural_source)
        .map_err(|error| error.to_string())?;
    config.defaults.runtime_backend = default_runtime_backend.clone();
    for (name, runtime_backend) in &profile_runtime_backends {
        let Some(profile) = config.profiles.get_mut(name) else {
            return Err(format!(
                "workspace profile `{name}` disappeared during structural admission"
            ));
        };
        profile.runtime_backend.clone_from(runtime_backend);
    }

    Ok(EnvWorkspace {
        config,
        default_runtime_backend,
        profile_runtime_backend: profile_runtime_backends.remove(selected_profile).flatten(),
    })
}

fn take_runtime_backend(
    table: Option<&mut toml::Table>,
    location: &str,
) -> Result<Option<String>, String> {
    let Some(value) = table.and_then(|table| table.remove("runtime_backend")) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|value| Some(value.to_string()))
        .ok_or_else(|| format!("workspace {location} runtime_backend must be a string"))
}

pub(super) fn select_runtime_backend(
    cli: &crate::Cli,
    profile: &str,
    runtime_backend_override: Option<&str>,
    profile_runtime_backend: Option<&str>,
    default_runtime_backend: Option<&str>,
    active_runtime_backend: &str,
) -> Result<AuthorizedRuntimeBackend, String> {
    let request = map([
        (":active", Term::Str(active_runtime_backend.to_string())),
        (":default", optional_string(default_runtime_backend)),
        (":kind", Term::Str(REQUEST_KIND.to_string())),
        (":override", optional_string(runtime_backend_override)),
        (":profile", Term::Str(profile.to_string())),
        (":profile-backend", optional_string(profile_runtime_backend)),
        (":v", Term::Int(1.into())),
    ]);
    let request_hash = hex32(hash_term(&request));
    let mut context = crate::mk_ctx(cli);
    let prelude = crate::build_prelude(&mut context);
    let mut environment = prelude.env;
    crate::load_selfhost_toolchain(cli, &mut context, &mut environment)
        .map_err(|error| format!("load workspace-env-select authority: {error:?}"))?;
    let authority = environment
        .get(AUTHORITY_BINDING)
        .ok_or_else(|| format!("missing binding {AUTHORITY_BINDING}"))?;
    let value = authority
        .apply(&mut context, Value::data(request))
        .map_err(|error| format!("{AUTHORITY_BINDING} failed: {error}"))?;
    if let Some((code, message, _)) = crate::extract_protocol_error(&context, &value) {
        return Err(format!(
            "{AUTHORITY_BINDING} returned sealed error: {code}: {message}"
        ));
    }
    decode_authorized(value, &request_hash, active_runtime_backend)
}

fn decode_authorized(
    value: Value,
    request_hash: &str,
    active_runtime_backend: &str,
) -> Result<AuthorizedRuntimeBackend, String> {
    let Some(Term::Map(envelope)) = value.to_plain_term() else {
        return Err("workspace-env-select authority returned non-map".to_string());
    };
    require_exact_fields(
        &envelope,
        &[
            ":code",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
            ":value",
        ],
        "workspace-env-select envelope",
    )?;
    require_string(&envelope, ":kind", RESULT_KIND)?;
    require_string(&envelope, ":request-h", request_hash)?;
    require_int(&envelope, ":v", 1)?;
    match field(&envelope, ":ok")? {
        Term::Bool(false) => {
            require_nil(&envelope, ":value")?;
            require_string(&envelope, ":code", "core/pkg/bad-workspace-env-selection")?;
            return Err(format!(
                "core/pkg/bad-workspace-env-selection: {}",
                required_string(&envelope, ":message")?
            ));
        }
        Term::Bool(true) => {
            require_nil(&envelope, ":code")?;
            require_nil(&envelope, ":message")?;
        }
        _ => return Err("workspace-env-select envelope :ok must be bool".to_string()),
    }
    let Term::Map(result) = field(&envelope, ":value")? else {
        return Err("workspace-env-select result :value must be map".to_string());
    };
    require_exact_fields(
        result,
        &[":active", ":compatible", ":selected", ":source"],
        "workspace-env-select result",
    )?;
    require_string(result, ":active", active_runtime_backend)?;
    let selected = required_string(result, ":selected")?.to_string();
    if !matches!(selected.as_str(), "headless" | "gpu" | "gfx" | "backend") {
        return Err("workspace-env-select returned invalid selected backend".to_string());
    }
    let compatible = match field(result, ":compatible")? {
        Term::Bool(value) => *value,
        _ => return Err("workspace-env-select :compatible must be bool".to_string()),
    };
    match field(result, ":source")? {
        Term::Symbol(source)
            if matches!(
                source.as_str(),
                ":override" | ":profile" | ":default" | ":builtin"
            ) => {}
        _ => return Err("workspace-env-select :source is invalid".to_string()),
    }
    Ok(AuthorizedRuntimeBackend {
        selected,
        compatible,
    })
}

fn optional_string(value: Option<&str>) -> Term {
    value
        .map(|value| Term::Str(value.to_string()))
        .unwrap_or(Term::Nil)
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn require_exact_fields(
    fields: &BTreeMap<TermOrdKey, Term>,
    names: &[&str],
    label: &str,
) -> Result<(), String> {
    let expected = names
        .iter()
        .map(|name| TermOrdKey(Term::symbol(*name)))
        .collect::<BTreeSet<_>>();
    if fields.keys().cloned().collect::<BTreeSet<_>>() == expected {
        Ok(())
    } else {
        Err(format!("{label} field set mismatch"))
    }
}

fn field<'a>(fields: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> Result<&'a Term, String> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| format!("workspace-env-select result missing {name}"))
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, String> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(format!("workspace-env-select {name} must be string")),
    }
}

fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), String> {
    if required_string(fields, name)? == expected {
        Ok(())
    } else {
        Err(format!("workspace-env-select {name} contradicts request"))
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Int(value) if value == &expected.into() => Ok(()),
        _ => Err(format!("workspace-env-select {name} must be {expected}")),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), String> {
    if field(fields, name)? == &Term::Nil {
        Ok(())
    } else {
        Err(format!("workspace-env-select {name} must be nil"))
    }
}

fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
