use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, Value};
use gc_pkg::{
    WorkspaceConfig, WorkspaceDefaults, WorkspaceMember, WorkspaceProfile, WorkspaceTask,
};

const AUTHORITY_BINDING: &str = "core/pkg::workspace-manifest-authority";
const REQUEST_KIND: &str = "genesis/pkg-workspace-manifest-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-workspace-manifest-authority-result-v0.1";
const SOURCE_LIMIT: usize = 16 * 1024 * 1024;
const COLLECTION_LIMIT: usize = 4096;
const NAME_LIMIT: usize = 1024;
const VALUE_LIMIT: usize = 16 * 1024 * 1024;

pub(super) struct AuthorizedWorkspace {
    pub(super) config: WorkspaceConfig,
    pub(super) default_runtime_backend: Option<String>,
    pub(super) selected_profile: Option<WorkspaceProfile>,
}

pub(super) fn load(
    cli: &crate::Cli,
    path: &Path,
    selected_profile: &str,
    require_profile: bool,
) -> Result<AuthorizedWorkspace, String> {
    bounded(selected_profile, NAME_LIMIT, "selected workspace profile")?;
    let bytes = read_bounded(path)?;
    let source = std::str::from_utf8(&bytes)
        .map_err(|_| "workspace manifest is not valid UTF-8".to_string())?;
    let document = toml::from_str::<toml::Value>(source)
        .map_err(|error| format!("workspace manifest is not valid TOML: {error}"))?;
    let source_hash = blake3::hash(&bytes).to_hex().to_string();
    let request = map([
        (":document", toml_to_term(document)),
        (":kind", Term::Str(REQUEST_KIND.to_string())),
        (":require-profile", Term::Bool(require_profile)),
        (":selected-profile", Term::Str(selected_profile.to_string())),
        (":source-h", Term::Str(source_hash.clone())),
        (":v", Term::Int(1.into())),
    ]);
    let request_hash = hex32(hash_term(&request));
    let mut context = crate::mk_ctx(cli);
    let prelude = crate::build_prelude(&mut context);
    let mut environment = prelude.env;
    crate::load_selfhost_toolchain(cli, &mut context, &mut environment)
        .map_err(|error| format!("load workspace-manifest authority: {error:?}"))?;
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
    decode(value, &request_hash, &source_hash, require_profile)
}

fn decode(
    value: Value,
    request_hash: &str,
    source_hash: &str,
    require_profile: bool,
) -> Result<AuthorizedWorkspace, String> {
    let Some(Term::Map(envelope)) = value.to_plain_term() else {
        return Err("workspace-manifest authority returned non-map".to_string());
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
        "workspace-manifest envelope",
    )?;
    require_string(&envelope, ":kind", RESULT_KIND)?;
    require_string(&envelope, ":request-h", request_hash)?;
    require_int(&envelope, ":v", 1)?;
    match field(&envelope, ":ok")? {
        Term::Bool(false) => {
            require_nil(&envelope, ":value")?;
            require_string(&envelope, ":code", "core/pkg/bad-workspace-manifest")?;
            return Err(format!(
                "core/pkg/bad-workspace-manifest: {}",
                required_string(&envelope, ":message")?
            ));
        }
        Term::Bool(true) => {
            require_nil(&envelope, ":code")?;
            require_nil(&envelope, ":message")?;
        }
        _ => return Err("workspace-manifest envelope :ok must be bool".to_string()),
    }
    let result = required_map(field(&envelope, ":value")?, "workspace-manifest result")?;
    require_exact_fields(
        result,
        &[
            ":defaults",
            ":members",
            ":profiles",
            ":selected-profile",
            ":source-h",
            ":tasks",
            ":version",
            ":workspace",
        ],
        "workspace-manifest result",
    )?;
    require_string(result, ":source-h", source_hash)?;
    require_int(result, ":version", 1)?;
    let workspace = bounded_string(result, ":workspace", VALUE_LIMIT)?;
    let members = decode_members(field(result, ":members")?)?;
    let defaults = decode_defaults(field(result, ":defaults")?)?;
    let profiles = decode_profiles(field(result, ":profiles")?)?;
    let tasks = decode_tasks(field(result, ":tasks")?)?;
    let selected_profile = decode_optional_profile(field(result, ":selected-profile")?)?;
    if require_profile && selected_profile.is_none() {
        return Err("workspace-manifest authority omitted required profile".to_string());
    }
    let default_runtime_backend = defaults.runtime_backend.clone();
    Ok(AuthorizedWorkspace {
        config: WorkspaceConfig {
            version: 1,
            workspace,
            members,
            defaults,
            profiles,
            tasks,
        },
        default_runtime_backend,
        selected_profile,
    })
}

fn decode_members(term: &Term) -> Result<Vec<WorkspaceMember>, String> {
    let values = bounded_vector(term, "workspace members")?;
    if values.is_empty() {
        return Err("workspace-manifest result members must be non-empty".to_string());
    }
    values
        .iter()
        .map(|value| {
            let fields = required_map(value, "workspace member")?;
            require_exact_fields(fields, &[":name", ":path", ":role"], "workspace member")?;
            Ok(WorkspaceMember {
                name: bounded_string(fields, ":name", NAME_LIMIT)?,
                path: bounded_string(fields, ":path", VALUE_LIMIT)?,
                role: optional_string(fields, ":role", VALUE_LIMIT)?,
            })
        })
        .collect()
}

fn decode_defaults(term: &Term) -> Result<WorkspaceDefaults, String> {
    let fields = required_map(term, "workspace defaults")?;
    require_exact_fields(
        fields,
        &[":policy", ":registry", ":runtime-backend", ":toolchain"],
        "workspace defaults",
    )?;
    Ok(WorkspaceDefaults {
        registry: optional_string(fields, ":registry", VALUE_LIMIT)?,
        policy: optional_string(fields, ":policy", VALUE_LIMIT)?,
        toolchain: optional_string(fields, ":toolchain", VALUE_LIMIT)?,
        runtime_backend: optional_string(fields, ":runtime-backend", 64)?,
    })
}

fn decode_profiles(term: &Term) -> Result<BTreeMap<String, WorkspaceProfile>, String> {
    let mut profiles = BTreeMap::new();
    for value in bounded_vector(term, "workspace profiles")? {
        let (name, profile) = decode_profile(value)?;
        if profiles.insert(name.clone(), profile).is_some() {
            return Err(format!(
                "workspace-manifest result duplicated profile `{name}`"
            ));
        }
    }
    Ok(profiles)
}

fn decode_optional_profile(term: &Term) -> Result<Option<WorkspaceProfile>, String> {
    if matches!(term, Term::Nil) {
        return Ok(None);
    }
    decode_profile(term).map(|(_, profile)| Some(profile))
}

fn decode_profile(term: &Term) -> Result<(String, WorkspaceProfile), String> {
    let fields = required_map(term, "workspace profile")?;
    require_exact_fields(
        fields,
        &[
            ":caps-policy",
            ":name",
            ":policy",
            ":registry",
            ":runtime-backend",
            ":toolchain",
        ],
        "workspace profile",
    )?;
    Ok((
        bounded_string(fields, ":name", NAME_LIMIT)?,
        WorkspaceProfile {
            caps_policy: optional_string(fields, ":caps-policy", VALUE_LIMIT)?,
            registry: optional_string(fields, ":registry", VALUE_LIMIT)?,
            policy: optional_string(fields, ":policy", VALUE_LIMIT)?,
            toolchain: optional_string(fields, ":toolchain", VALUE_LIMIT)?,
            runtime_backend: optional_string(fields, ":runtime-backend", 64)?,
        },
    ))
}

fn decode_tasks(term: &Term) -> Result<BTreeMap<String, WorkspaceTask>, String> {
    let mut tasks = BTreeMap::new();
    for value in bounded_vector(term, "workspace tasks")? {
        let fields = required_map(value, "workspace task")?;
        require_exact_fields(
            fields,
            &[":args", ":cmd", ":file", ":name", ":pkg"],
            "workspace task",
        )?;
        let name = bounded_string(fields, ":name", NAME_LIMIT)?;
        let arguments = bounded_vector(field(fields, ":args")?, "workspace task arguments")?
            .iter()
            .map(|value| match value {
                Term::Str(value) if value.len() <= VALUE_LIMIT => Ok(value.clone()),
                _ => Err("workspace task argument must be bounded string".to_string()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let task = WorkspaceTask {
            cmd: bounded_string(fields, ":cmd", VALUE_LIMIT)?,
            file: optional_string(fields, ":file", VALUE_LIMIT)?,
            pkg: optional_string(fields, ":pkg", VALUE_LIMIT)?,
            args: arguments,
        };
        if tasks.insert(name.clone(), task).is_some() {
            return Err(format!(
                "workspace-manifest result duplicated task `{name}`"
            ));
        }
    }
    Ok(tasks)
}

fn toml_to_term(value: toml::Value) -> Term {
    match value {
        toml::Value::String(value) => Term::Str(value),
        toml::Value::Integer(value) => Term::Int(value.into()),
        toml::Value::Boolean(value) => Term::Bool(value),
        toml::Value::Float(value) => map([(":toml-float", Term::Str(format!("{value:e}")))]),
        toml::Value::Datetime(value) => map([(":toml-datetime", Term::Str(value.to_string()))]),
        toml::Value::Array(values) => Term::Vector(values.into_iter().map(toml_to_term).collect()),
        toml::Value::Table(values) => Term::Map(
            values
                .into_iter()
                .map(|(key, value)| (TermOrdKey(Term::Str(key)), toml_to_term(value)))
                .collect(),
        ),
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("read workspace manifest `{}`: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| {
        format!(
            "stat opened workspace manifest `{}`: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "workspace manifest is not a regular file: {}",
            path.display()
        ));
    }
    let declared = metadata.len();
    if declared > SOURCE_LIMIT as u64 {
        return Err(format!(
            "workspace manifest exceeds {SOURCE_LIMIT}-byte transport limit"
        ));
    }
    let mut bytes = Vec::with_capacity(declared as usize);
    file.by_ref()
        .take(SOURCE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read workspace manifest `{}`: {error}", path.display()))?;
    if bytes.len() > SOURCE_LIMIT {
        return Err(format!(
            "workspace manifest exceeds {SOURCE_LIMIT}-byte transport limit"
        ));
    }
    Ok(bytes)
}

fn bounded_vector<'a>(term: &'a Term, label: &str) -> Result<&'a [Term], String> {
    match term {
        Term::Vector(values) if values.len() <= COLLECTION_LIMIT => Ok(values),
        Term::Vector(_) => Err(format!(
            "{label} exceeds {COLLECTION_LIMIT}-entry result limit"
        )),
        _ => Err(format!("{label} must be vector")),
    }
}

fn required_map<'a>(term: &'a Term, label: &str) -> Result<&'a BTreeMap<TermOrdKey, Term>, String> {
    match term {
        Term::Map(fields) => Ok(fields),
        _ => Err(format!("{label} must be map")),
    }
}

fn bounded_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    limit: usize,
) -> Result<String, String> {
    let value = required_string(fields, name)?;
    bounded(value, limit, name)?;
    Ok(value.to_string())
}

fn optional_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    limit: usize,
) -> Result<Option<String>, String> {
    match field(fields, name)? {
        Term::Nil => Ok(None),
        Term::Str(value) => {
            bounded(value, limit, name)?;
            Ok(Some(value.clone()))
        }
        _ => Err(format!(
            "workspace-manifest result {name} must be string or nil"
        )),
    }
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
        .ok_or_else(|| format!("workspace-manifest result missing {name}"))
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, String> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(format!("workspace-manifest result {name} must be string")),
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
        Err(format!("workspace-manifest result {name} mismatch"))
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Int(value) if value == &expected.into() => Ok(()),
        _ => Err(format!("workspace-manifest result {name} mismatch")),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), String> {
    if matches!(field(fields, name)?, Term::Nil) {
        Ok(())
    } else {
        Err(format!("workspace-manifest result {name} must be nil"))
    }
}

fn bounded(value: &str, limit: usize, label: &str) -> Result<(), String> {
    if value.len() <= limit {
        Ok(())
    } else {
        Err(format!("{label} exceeds {limit}-byte transport limit"))
    }
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
