use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, Value};

#[path = "pkg_workspace_env_authority/decode.rs"]
mod decode;
use decode::*;

const AUTHORITY_BINDING: &str = "core/pkg::workspace-env-authority";
pub(super) const PLAN_KIND: &str = "genesis/pkg-workspace-env-plan-authority-request-v0.1";
const FINALIZE_KIND: &str = "genesis/pkg-workspace-env-finalize-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-workspace-env-authority-result-v0.1";

pub(super) struct AuthorizedPlan {
    pub(super) term: Term,
    pub(super) hash: String,
    pub(super) active: String,
    pub(super) backend_required: bool,
    pub(super) caps_policy_raw: String,
    pub(super) effective_registry: Option<String>,
    pub(super) effective_toolchain: Option<String>,
    pub(super) profile: String,
    pub(super) selected: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FileScope {
    Environment,
    External,
}

pub(super) struct AuthorizedFile {
    pub(super) scope: FileScope,
    pub(super) path: PathBuf,
    pub(super) body: Vec<u8>,
}

pub(super) struct AuthorizedEnvironment {
    pub(super) env_root: PathBuf,
    pub(super) files: Vec<AuthorizedFile>,
    pub(super) mkdirs: Vec<PathBuf>,
    pub(super) public: Term,
}

pub(super) fn optional(value: Option<&str>) -> Term {
    decode::optional_term(value)
}

pub(super) fn authorize<F>(
    cli: &crate::Cli,
    plan_request: Term,
    out_dir: &Path,
    observe: F,
) -> Result<AuthorizedEnvironment, String>
where
    F: FnOnce(&AuthorizedPlan) -> Result<Term, String>,
{
    let plan_request_hash = hex32(hash_term(&plan_request));
    let mut context = crate::mk_ctx(cli);
    let prelude = crate::build_prelude(&mut context);
    let mut environment = prelude.env;
    crate::load_selfhost_toolchain(cli, &mut context, &mut environment)
        .map_err(|error| format!("load workspace-env authority: {error:?}"))?;
    let authority = environment
        .get(AUTHORITY_BINDING)
        .ok_or_else(|| format!("missing binding {AUTHORITY_BINDING}"))?;
    let plan_value = authority
        .clone()
        .apply(&mut context, Value::data(plan_request.clone()))
        .map_err(|error| format!("{AUTHORITY_BINDING} plan failed: {error}"))?;
    let plan_result = decode_envelope(&context, plan_value, &plan_request_hash, "plan")?;
    let plan = decode_plan(plan_result)?;
    let observations = observe(&plan)?;
    let finalize_request = map([
        (":kind", Term::Str(FINALIZE_KIND.to_string())),
        (":observations", observations.clone()),
        (":plan", plan.term.clone()),
        (":plan-h", Term::Str(plan.hash.clone())),
        (":plan-request", plan_request.clone()),
        (":v", Term::Int(1.into())),
    ]);
    let finalize_hash = hex32(hash_term(&finalize_request));
    let finalized = authority
        .apply(&mut context, Value::data(finalize_request))
        .map_err(|error| format!("{AUTHORITY_BINDING} finalize failed: {error}"))?;
    let result = decode_envelope(&context, finalized, &finalize_hash, "finalize")?;
    decode_environment(result, &plan, &plan_request, &observations, out_dir)
}

fn decode_envelope(
    context: &gc_kernel::EvalCtx,
    value: Value,
    request_hash: &str,
    phase: &str,
) -> Result<BTreeMap<TermOrdKey, Term>, String> {
    if let Some((code, message, _)) = crate::extract_protocol_error(context, &value) {
        return Err(format!(
            "{AUTHORITY_BINDING} returned sealed error: {code}: {message}"
        ));
    }
    let Some(Term::Map(envelope)) = value.to_plain_term() else {
        return Err(format!("workspace-env {phase} authority returned non-map"));
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
        "workspace-env envelope",
    )?;
    require_string(&envelope, ":kind", RESULT_KIND)?;
    require_string(&envelope, ":request-h", request_hash)?;
    require_int(&envelope, ":v", 1)?;
    match field(&envelope, ":ok")? {
        Term::Bool(false) => {
            require_nil(&envelope, ":value")?;
            let code = required_string(&envelope, ":code")?;
            if !matches!(
                code,
                "core/pkg/bad-workspace-env" | "core/pkg/bad-workspace-env-selection"
            ) {
                return Err("workspace-env authority returned invalid rejection code".to_string());
            }
            Err(format!(
                "{code}: {}",
                required_string(&envelope, ":message")?
            ))
        }
        Term::Bool(true) => {
            require_nil(&envelope, ":code")?;
            require_nil(&envelope, ":message")?;
            let Term::Map(result) = field(&envelope, ":value")? else {
                return Err(format!("workspace-env {phase} result must be map"));
            };
            Ok(result.clone())
        }
        _ => Err("workspace-env envelope :ok must be bool".to_string()),
    }
}

fn decode_plan(result: BTreeMap<TermOrdKey, Term>) -> Result<AuthorizedPlan, String> {
    require_exact_fields(&result, &[":plan", ":plan-h"], "workspace-env plan result")?;
    let Term::Map(plan_fields) = field(&result, ":plan")? else {
        return Err("workspace-env :plan must be map".to_string());
    };
    require_exact_fields(
        plan_fields,
        &[
            ":active",
            ":backend-required",
            ":caps-policy-raw",
            ":deps-body",
            ":deps-h",
            ":effective-policy",
            ":effective-registry",
            ":effective-toolchain",
            ":lock-h",
            ":members-body",
            ":members-h",
            ":profile",
            ":profile-policy",
            ":profile-registry",
            ":profile-toolchain",
            ":selected",
            ":source",
            ":workspace-h",
        ],
        "workspace-env plan",
    )?;
    let plan = field(&result, ":plan")?.clone();
    let actual_hash = hex32(hash_term(&plan));
    require_string(&result, ":plan-h", &actual_hash)?;
    let active = required_string(plan_fields, ":active")?.to_string();
    let selected = required_string(plan_fields, ":selected")?.to_string();
    for (label, value) in [
        (":active", active.as_str()),
        (":selected", selected.as_str()),
    ] {
        if !matches!(value, "headless" | "gpu" | "gfx" | "backend") {
            return Err(format!("workspace-env plan {label} is invalid"));
        }
    }
    let backend_required = required_bool(plan_fields, ":backend-required")?;
    let caps_policy_raw = required_string(plan_fields, ":caps-policy-raw")?.to_string();
    if caps_policy_raw.is_empty() {
        return Err("workspace-env plan caps policy is empty".to_string());
    }
    let effective_registry = optional_string(plan_fields, ":effective-registry")?;
    let effective_toolchain = optional_string(plan_fields, ":effective-toolchain")?;
    let profile = required_string(plan_fields, ":profile")?.to_string();
    if profile.is_empty() {
        return Err("workspace-env plan profile is empty".to_string());
    }
    for (body_key, hash_key) in [(":deps-body", ":deps-h"), (":members-body", ":members-h")] {
        let body = required_bytes(plan_fields, body_key)?;
        require_string(plan_fields, hash_key, &blake3_hex(body))?;
        let source = std::str::from_utf8(body)
            .map_err(|_| format!("workspace-env plan {body_key} must be UTF-8 CoreForm"))?;
        gc_coreform::parse_term(source)
            .map_err(|error| format!("workspace-env plan {body_key} is invalid: {error}"))?;
    }
    for key in [":lock-h", ":workspace-h"] {
        require_lower_hex64(required_string(plan_fields, key)?, key)?;
    }
    for key in [
        ":effective-policy",
        ":profile-policy",
        ":profile-registry",
        ":profile-toolchain",
    ] {
        let _ = optional_string(plan_fields, key)?;
    }
    match field(plan_fields, ":source")? {
        Term::Symbol(source)
            if matches!(
                source.as_str(),
                ":override" | ":profile" | ":default" | ":builtin"
            ) => {}
        _ => return Err("workspace-env plan source is invalid".to_string()),
    }
    Ok(AuthorizedPlan {
        term: plan,
        hash: actual_hash,
        active,
        backend_required,
        caps_policy_raw,
        effective_registry,
        effective_toolchain,
        profile,
        selected,
    })
}

fn decode_environment(
    result: BTreeMap<TermOrdKey, Term>,
    plan: &AuthorizedPlan,
    plan_request: &Term,
    observations: &Term,
    out_dir: &Path,
) -> Result<AuthorizedEnvironment, String> {
    require_exact_fields(
        &result,
        &[
            ":env-h",
            ":env-root",
            ":files",
            ":mkdirs",
            ":profile-h",
            ":public",
        ],
        "workspace-env final result",
    )?;
    let env_h = required_string(&result, ":env-h")?;
    require_lower_hex64(env_h, ":env-h")?;
    let env_root = out_dir.join(env_h);
    require_string(&result, ":env-root", &env_root.display().to_string())?;
    let profile_h = required_string(&result, ":profile-h")?;
    require_lower_hex64(profile_h, ":profile-h")?;
    let expected_runtime = nested_string(plan_request, ":paths", ":wasi-runtime-file")?;
    let Term::Vector(file_terms) = field(&result, ":files")? else {
        return Err("workspace-env :files must be vector".to_string());
    };
    let expected_names = expected_file_names(plan, observations)?;
    if file_terms.len() != expected_names.len() + 1 {
        return Err("workspace-env file inventory length mismatch".to_string());
    }
    let mut files = Vec::with_capacity(file_terms.len());
    let mut env_bodies = BTreeMap::<String, Vec<u8>>::new();
    for (index, file_term) in file_terms.iter().enumerate() {
        let Term::Map(file) = file_term else {
            return Err("workspace-env file entry must be map".to_string());
        };
        require_exact_fields(
            file,
            &[":body", ":h", ":path", ":scope"],
            "workspace-env file",
        )?;
        let body = required_bytes(file, ":body")?.to_vec();
        require_string(file, ":h", &blake3_hex(&body))?;
        let path = required_string(file, ":path")?;
        let scope = match field(file, ":scope")? {
            Term::Symbol(value) if value == ":env" => FileScope::Environment,
            Term::Symbol(value) if value == ":external" => FileScope::External,
            _ => return Err("workspace-env file scope is invalid".to_string()),
        };
        if index < expected_names.len() {
            if scope != FileScope::Environment || path != expected_names[index] {
                return Err("workspace-env environment file order or scope mismatch".to_string());
            }
            let relative = Path::new(path);
            if relative.components().count() != 1
                || !matches!(relative.components().next(), Some(Component::Normal(_)))
            {
                return Err("workspace-env returned unsafe environment-relative path".to_string());
            }
            env_bodies.insert(path.to_string(), body.clone());
            files.push(AuthorizedFile {
                scope,
                path: relative.to_path_buf(),
                body,
            });
        } else {
            if scope != FileScope::External || path != expected_runtime {
                return Err("workspace-env external file path mismatch".to_string());
            }
            files.push(AuthorizedFile {
                scope,
                path: PathBuf::from(path),
                body,
            });
        }
    }
    validate_authorized_bodies(
        &env_bodies,
        env_h,
        profile_h,
        plan,
        plan_request,
        observations,
    )?;
    let mkdirs = decode_mkdirs(field(&result, ":mkdirs")?, plan_request, observations)?;
    let public = field(&result, ":public")?.clone();
    validate_public(&public, plan, observations, &env_root, env_h, profile_h)?;
    Ok(AuthorizedEnvironment {
        env_root,
        files,
        mkdirs,
        public,
    })
}

fn expected_file_names(
    plan: &AuthorizedPlan,
    observations: &Term,
) -> Result<Vec<&'static str>, String> {
    let mut names = vec![
        "env.gcenv",
        "members.gc",
        "deps.gc",
        "workspace.toml",
        "genesis.lock",
        "caps-policy.toml",
    ];
    if plan.backend_required {
        names.push("caps-policy.backend.effective.toml");
    }
    let observations = as_map(observations, "workspace-env observations")?;
    if !matches!(field(observations, ":toolchain")?, Term::Nil) {
        names.push("toolchain.gc");
    }
    names.extend(["profile.gc", "provenance.gc", "wasi-http-bridge.gc"]);
    Ok(names)
}

fn validate_authorized_bodies(
    bodies: &BTreeMap<String, Vec<u8>>,
    env_h: &str,
    profile_h: &str,
    plan: &AuthorizedPlan,
    plan_request: &Term,
    observations: &Term,
) -> Result<(), String> {
    let env_body = body(bodies, "env.gcenv")?;
    if blake3_hex(env_body) != env_h {
        return Err("workspace-env env body identity mismatch".to_string());
    }
    if blake3_hex(body(bodies, "profile.gc")?) != profile_h {
        return Err("workspace-env profile body identity mismatch".to_string());
    }
    let plan_fields = as_map(&plan.term, "workspace-env plan")?;
    for (name, key) in [("members.gc", ":members-body"), ("deps.gc", ":deps-body")] {
        if body(bodies, name)? != required_bytes(plan_fields, key)? {
            return Err(format!("workspace-env {name} contradicts authorized plan"));
        }
    }
    let request = as_map(plan_request, "workspace-env plan request")?;
    for (name, key) in [
        ("workspace.toml", ":workspace-bytes"),
        ("genesis.lock", ":lock-bytes"),
    ] {
        if body(bodies, name)? != required_bytes(request, key)? {
            return Err(format!(
                "workspace-env {name} contradicts source observation"
            ));
        }
    }
    let observations = as_map(observations, "workspace-env observations")?;
    let caps = as_map(field(observations, ":caps-policy")?, "caps observation")?;
    if body(bodies, "caps-policy.toml")? != required_bytes(caps, ":bytes")? {
        return Err("workspace-env caps policy body mismatch".to_string());
    }
    if let Some(toolchain) = bodies.get("toolchain.gc") {
        let observed = as_map(field(observations, ":toolchain")?, "toolchain observation")?;
        if toolchain.as_slice() != required_bytes(observed, ":bytes")? {
            return Err("workspace-env toolchain body mismatch".to_string());
        }
    }
    if let Some(effective) = bodies.get("caps-policy.backend.effective.toml") {
        let backend = as_map(field(observations, ":backend")?, "backend observation")?;
        if effective.as_slice() != required_bytes(backend, ":effective-caps-bytes")? {
            return Err("workspace-env effective caps body mismatch".to_string());
        }
    }
    for name in [
        "env.gcenv",
        "profile.gc",
        "provenance.gc",
        "wasi-http-bridge.gc",
    ] {
        let source = std::str::from_utf8(body(bodies, name)?)
            .map_err(|_| format!("workspace-env {name} must be UTF-8 CoreForm"))?;
        gc_coreform::parse_term(source)
            .map_err(|error| format!("workspace-env {name} is invalid CoreForm: {error}"))?;
    }
    Ok(())
}

fn decode_mkdirs(
    value: &Term,
    plan_request: &Term,
    observations: &Term,
) -> Result<Vec<PathBuf>, String> {
    let Term::Vector(values) = value else {
        return Err("workspace-env :mkdirs must be vector".to_string());
    };
    let mut expected = vec![
        nested_string(plan_request, ":paths", ":wasi-http-dir")?.to_string(),
        nested_string(plan_request, ":paths", ":wasi-https-dir")?.to_string(),
    ];
    let wasi = nested_map(observations, ":wasi")?;
    if let Some(remote_root) = optional_string(wasi, ":remote-root")? {
        expected.push(remote_root);
    }
    let actual = values
        .iter()
        .map(|value| match value {
            Term::Str(path) => Ok(path.clone()),
            _ => Err("workspace-env mkdir path must be string".to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if actual != expected {
        return Err("workspace-env mkdir inventory mismatch".to_string());
    }
    Ok(actual.into_iter().map(PathBuf::from).collect())
}

fn validate_public(
    public: &Term,
    plan: &AuthorizedPlan,
    observations: &Term,
    env_root: &Path,
    env_h: &str,
    profile_h: &str,
) -> Result<(), String> {
    let fields = as_map(public, "workspace-env public result")?;
    require_exact_fields(
        fields,
        &[
            ":active-runtime-backend-profile",
            ":backend-bridge-cmd",
            ":backend-bridge-ready",
            ":backend-bridge-sha256",
            ":caps-policy",
            ":caps-policy-effective",
            ":caps-policy-effective-h",
            ":caps-policy-h",
            ":env-h",
            ":env-root",
            ":ok",
            ":profile",
            ":profile-h",
            ":runtime-backend-compatible",
            ":runtime-backend-profile",
            ":wasi-http-bridge-remote",
            ":wasi-http-bridge-remote-root",
            ":wasi-http-bridge-root",
        ],
        "workspace-env public result",
    )?;
    require_bool(fields, ":ok", true)?;
    require_bool(fields, ":runtime-backend-compatible", true)?;
    require_string(fields, ":active-runtime-backend-profile", &plan.active)?;
    require_string(fields, ":runtime-backend-profile", &plan.selected)?;
    require_string(fields, ":profile", &plan.profile)?;
    require_string(fields, ":env-h", env_h)?;
    require_string(fields, ":profile-h", profile_h)?;
    require_string(fields, ":env-root", &env_root.display().to_string())?;
    let observations = as_map(observations, "workspace-env observations")?;
    let caps = as_map(field(observations, ":caps-policy")?, "caps observation")?;
    require_string(fields, ":caps-policy", required_string(caps, ":path")?)?;
    require_string(fields, ":caps-policy-h", required_string(caps, ":h")?)?;
    let wasi = as_map(field(observations, ":wasi")?, "wasi observation")?;
    require_string(
        fields,
        ":wasi-http-bridge-root",
        required_string(wasi, ":root")?,
    )?;
    require_optional_string(
        fields,
        ":wasi-http-bridge-remote",
        plan.effective_registry.as_deref(),
    )?;
    require_optional_string(
        fields,
        ":wasi-http-bridge-remote-root",
        optional_string(wasi, ":remote-root")?.as_deref(),
    )?;
    if plan.backend_required {
        let backend = as_map(field(observations, ":backend")?, "backend observation")?;
        require_bool(fields, ":backend-bridge-ready", true)?;
        require_string(
            fields,
            ":caps-policy-effective",
            &env_root
                .join("caps-policy.backend.effective.toml")
                .display()
                .to_string(),
        )?;
        require_string(
            fields,
            ":backend-bridge-cmd",
            required_string(backend, ":bridge-cmd")?,
        )?;
        require_string(
            fields,
            ":backend-bridge-sha256",
            &format!("sha256:{}", required_string(backend, ":bridge-sha256")?),
        )?;
        require_string(
            fields,
            ":caps-policy-effective-h",
            required_string(backend, ":effective-caps-h")?,
        )?;
    } else {
        for key in [
            ":backend-bridge-cmd",
            ":backend-bridge-sha256",
            ":caps-policy-effective",
            ":caps-policy-effective-h",
        ] {
            require_symbol(fields, key, ":none")?;
        }
        require_bool(fields, ":backend-bridge-ready", false)?;
    }
    Ok(())
}
