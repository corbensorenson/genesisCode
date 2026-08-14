use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, Value};
use gc_pkg::{GenesisLock, WorkspaceConfig};

#[cfg(any(test, feature = "parity-harness"))]
#[allow(dead_code)] // Retained only as an explicit compatibility oracle.
#[path = "pkg_scaffold/parity.rs"]
mod parity;

use crate::pkg_caps_templates::{
    CAPS_CI_DEFAULT, CAPS_DEV_DEFAULT, CAPS_RELEASE_DEFAULT, render_backend_caps_policy,
};
use crate::pkg_workspace_ops::LocalPkgResult;

const AUTHORITY_BINDING: &str = "core/pkg::scaffold-authority";
const REQUEST_KIND: &str = "genesis/pkg-scaffold-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-scaffold-authority-result-v0.1";
const FILE_PATHS: [&str; 10] = [
    "genesis.workspace.toml",
    "genesis.lock",
    "package.toml",
    "src/main.gc",
    "deploy/presets.toml",
    "caps.toml",
    "caps.ci.toml",
    "caps.release.toml",
    "caps.backend.toml",
    "README.gcpm.md",
];

#[derive(Debug)]
struct AuthorizedScaffold {
    files: Vec<(PathBuf, String)>,
    report: Term,
}

pub(crate) struct PkgScaffoldArgs<'a> {
    pub(crate) archetype: &'a str,
    pub(crate) name: &'a str,
    pub(crate) root: &'a Path,
    pub(crate) force: bool,
    pub(crate) runtime_backend: Option<&'a str>,
    pub(crate) policy: &'a str,
    pub(crate) registry_default: Option<&'a str>,
}

pub(crate) fn handle_scaffold(
    cli: &crate::Cli,
    args: PkgScaffoldArgs<'_>,
) -> Result<LocalPkgResult, String> {
    let static_files = vec![
        ("caps.toml", CAPS_DEV_DEFAULT.to_string()),
        ("caps.ci.toml", CAPS_CI_DEFAULT.to_string()),
        ("caps.release.toml", CAPS_RELEASE_DEFAULT.to_string()),
        ("caps.backend.toml", render_backend_caps_policy(None, None)),
    ];
    let request = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":archetype")),
                Term::Str(args.archetype.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(REQUEST_KIND.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str(args.name.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":policy")),
                Term::Str(args.policy.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":registry-default")),
                args.registry_default
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":root")),
                Term::Str(args.root.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-backend")),
                args.runtime_backend
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":static-files")),
                Term::Vector(
                    static_files
                        .iter()
                        .map(|(path, body)| {
                            Term::Map(
                                [
                                    (TermOrdKey(Term::symbol(":body")), Term::Str(body.clone())),
                                    (
                                        TermOrdKey(Term::symbol(":path")),
                                        Term::Str((*path).to_string()),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            )
                        })
                        .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    );
    let request_hash = hex32(hash_term(&request));
    let mut context = crate::mk_ctx(cli);
    let prelude = crate::build_prelude(&mut context);
    let mut environment = prelude.env;
    crate::load_selfhost_toolchain(cli, &mut context, &mut environment)
        .map_err(|error| format!("load scaffold authority: {error:?}"))?;
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
    let authorized = decode_authorized_scaffold(
        value,
        &request_hash,
        args.root,
        args.archetype,
        args.policy,
        args.registry_default,
        &static_files,
    )?;

    preflight_scaffold(args.root, &authorized.files, args.force)?;
    for (relative, body) in &authorized.files {
        write_scaffold_file(&args.root.join(relative), body.as_bytes(), true)?;
    }

    Ok(LocalPkgResult {
        kind: "genesis/pkg-scaffold-v0.1",
        log_op: "pkg-scaffold",
        program_hash: hash_term(&authorized.report),
        value: authorized.report,
    })
}

fn decode_authorized_scaffold(
    value: Value,
    request_hash: &str,
    root: &Path,
    requested_archetype: &str,
    requested_policy: &str,
    requested_registry: Option<&str>,
    static_files: &[(&str, String)],
) -> Result<AuthorizedScaffold, String> {
    let Some(Term::Map(envelope)) = value.to_plain_term() else {
        return Err(format!(
            "scaffold authority returned non-map: {}",
            value.debug_repr()
        ));
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
        "scaffold envelope",
    )?;
    require_string(&envelope, ":kind", RESULT_KIND)?;
    require_int(&envelope, ":v", 1)?;
    require_string(&envelope, ":request-h", request_hash)?;
    match field(&envelope, ":ok")? {
        Term::Bool(false) => {
            require_nil(&envelope, ":value")?;
            let code = required_string(&envelope, ":code")?;
            if code != "core/pkg/bad-scaffold" {
                return Err("scaffold rejection code is outside closed inventory".to_string());
            }
            return Err(format!(
                "{code}: {}",
                required_string(&envelope, ":message")?
            ));
        }
        Term::Bool(true) => {
            require_nil(&envelope, ":code")?;
            require_nil(&envelope, ":message")?;
        }
        _ => return Err("scaffold envelope :ok must be bool".to_string()),
    }
    let Term::Map(result) = field(&envelope, ":value")? else {
        return Err("scaffold result :value must be map".to_string());
    };
    require_exact_fields(result, &[":files", ":report"], "scaffold result")?;
    let Term::Vector(file_terms) = field(result, ":files")? else {
        return Err("scaffold result :files must be vector".to_string());
    };
    if file_terms.len() != FILE_PATHS.len() {
        return Err("scaffold result file count mismatch".to_string());
    }
    let mut files = Vec::with_capacity(file_terms.len());
    let mut records = Vec::with_capacity(file_terms.len());
    for (index, term) in file_terms.iter().enumerate() {
        let Term::Map(file) = term else {
            return Err("scaffold file entry must be map".to_string());
        };
        require_exact_fields(file, &[":body", ":h", ":path"], "scaffold file")?;
        let path = required_string(file, ":path")?;
        if path != FILE_PATHS[index] {
            return Err(format!("scaffold file order/path mismatch at {index}"));
        }
        let body = required_string(file, ":body")?.to_string();
        let declared_hash = required_string(file, ":h")?;
        let actual_hash = blake3::hash(body.as_bytes()).to_hex().to_string();
        if declared_hash != actual_hash {
            return Err(format!("scaffold file hash contradicts body for {path}"));
        }
        if (5..=8).contains(&index) {
            let (expected_path, expected_body) = &static_files[index - 5];
            if path != *expected_path || &body != expected_body {
                return Err(format!(
                    "scaffold static file contradicts observation for {path}"
                ));
            }
        }
        records.push(format!("{path}:{declared_hash}"));
        files.push((PathBuf::from(path), body));
    }
    records.sort();
    let scaffold_hash = blake3::hash(records.join("\n").as_bytes())
        .to_hex()
        .to_string();

    let report = field(result, ":report")?.clone();
    let Term::Map(report_fields) = &report else {
        return Err("scaffold report must be map".to_string());
    };
    require_exact_fields(
        report_fields,
        &[
            ":archetype",
            ":files",
            ":files-written",
            ":ok",
            ":package",
            ":root",
            ":runtime-backend-profile",
            ":scaffold-h",
            ":workspace",
        ],
        "scaffold report",
    )?;
    require_int(report_fields, ":files-written", FILE_PATHS.len() as i64)?;
    if field(report_fields, ":ok")? != &Term::Bool(true) {
        return Err("scaffold report :ok must be true".to_string());
    }
    require_string(report_fields, ":root", &root.display().to_string())?;
    require_string(report_fields, ":scaffold-h", &scaffold_hash)?;
    let Term::Vector(report_paths) = field(report_fields, ":files")? else {
        return Err("scaffold report :files must be vector".to_string());
    };
    let expected_paths = FILE_PATHS
        .iter()
        .map(|path| Term::Str((*path).to_string()))
        .collect::<Vec<_>>();
    if report_paths != &expected_paths {
        return Err("scaffold report file inventory contradicts plan".to_string());
    }
    require_string(report_fields, ":archetype", requested_archetype)?;

    let workspace = WorkspaceConfig::from_toml_str(Path::new(FILE_PATHS[0]), &files[0].1)
        .map_err(|error| format!("scaffold workspace document is invalid: {error}"))?;
    require_string(report_fields, ":workspace", &workspace.workspace)?;
    if workspace.members.len() != 1 || workspace.members[0].path != "." {
        return Err("scaffold workspace member inventory is not closed".to_string());
    }
    require_string(report_fields, ":package", &workspace.members[0].name)?;
    if workspace.defaults.policy.as_deref() != Some(requested_policy)
        || workspace.defaults.registry.as_deref() != requested_registry
    {
        return Err("scaffold workspace defaults contradict request".to_string());
    }
    let backend = workspace
        .defaults
        .runtime_backend
        .as_deref()
        .ok_or_else(|| "scaffold workspace omits runtime backend".to_string())?;
    require_string(report_fields, ":runtime-backend-profile", backend)?;

    let lock = GenesisLock::from_toml_str(Path::new(FILE_PATHS[1]), &files[1].1)
        .map_err(|error| format!("scaffold lock document is invalid: {error}"))?;
    if lock.workspace != workspace.workspace || lock.policy != requested_policy {
        return Err("scaffold lock contradicts workspace or request".to_string());
    }
    match requested_registry {
        Some(registry) if lock.registries.get("default").map(String::as_str) == Some(registry) => {}
        None if !lock.registries.contains_key("default") => {}
        _ => return Err("scaffold lock registry contradicts request".to_string()),
    }
    Ok(AuthorizedScaffold { files, report })
}

fn preflight_scaffold(root: &Path, files: &[(PathBuf, String)], force: bool) -> Result<(), String> {
    preflight_directory_chain(root)?;
    for (relative, _) in files {
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(format!(
                "scaffold authority returned unsafe relative path: {}",
                relative.display()
            ));
        }
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            preflight_directory_chain(parent)?;
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "refusing scaffold destination symlink: {}",
                    path.display()
                ));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(format!(
                    "scaffold destination is not a regular file: {}",
                    path.display()
                ));
            }
            Ok(_) if !force => {
                return Err(format!(
                    "refusing to overwrite existing path without --force: {}",
                    path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "inspect scaffold destination {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn preflight_directory_chain(path: &Path) -> Result<(), String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("resolve scaffold working directory: {error}"))?
            .join(path)
    };
    let mut prefix = PathBuf::new();
    let mut missing = false;
    for component in absolute.components() {
        prefix.push(component.as_os_str());
        if missing {
            continue;
        }
        match std::fs::symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "refusing scaffold directory symlink: {}",
                    prefix.display()
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!(
                    "scaffold directory component is not a directory: {}",
                    prefix.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing = true,
            Err(error) => {
                return Err(format!(
                    "inspect scaffold directory {}: {error}",
                    prefix.display()
                ));
            }
        }
    }
    Ok(())
}

fn require_exact_fields(
    fields: &BTreeMap<TermOrdKey, Term>,
    names: &[&str],
    label: &str,
) -> Result<(), String> {
    let expected = names
        .iter()
        .map(|name| TermOrdKey(Term::symbol(*name)))
        .collect::<std::collections::BTreeSet<_>>();
    if fields
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        == expected
    {
        Ok(())
    } else {
        Err(format!("{label} field set mismatch"))
    }
}

fn field<'a>(fields: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> Result<&'a Term, String> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| format!("scaffold result missing {name}"))
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, String> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(format!("scaffold result {name} must be string")),
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
        Err(format!("scaffold result {name} mismatch"))
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Int(value) if value.to_string() == expected.to_string() => Ok(()),
        _ => Err(format!("scaffold result {name} mismatch")),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), String> {
    match field(fields, name)? {
        Term::Nil => Ok(()),
        _ => Err(format!("scaffold result {name} must be nil")),
    }
}

fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_scaffold_file(path: &Path, bytes: &[u8], force: bool) -> Result<(), String> {
    if path.is_file() && !force {
        return Err(format!(
            "refusing to overwrite existing file without --force: {}",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "failed to create scaffold directory {}: {e}",
                parent.display()
            )
        })?;
    }
    atomic_write_text(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))
}

pub(crate) fn atomic_write_text(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut sequence = 0u64;
    let tmp = loop {
        let candidate = parent.join(format!(
            ".{}.tmp-{}-{sequence}",
            path.file_name().and_then(|s| s.to_str()).unwrap_or("write"),
            crate::platform_process_id()
        ));
        sequence = sequence.saturating_add(1);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                use std::io::Write as _;
                if let Err(error) = file.write_all(bytes) {
                    drop(file);
                    let _ = std::fs::remove_file(&candidate);
                    return Err(error);
                }
                break candidate;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}
