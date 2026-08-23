use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey};
use gc_pkg::{GenesisLock, UpdatePolicy, WorkspaceConfig};

use super::*;

const BODY_LIMIT: usize = 16 * 1024 * 1024;

pub(crate) fn handle_env(
    cli: &crate::Cli,
    profile: &str,
    runtime_backend_override: Option<&str>,
    lock: &Path,
    workspace_file: &Path,
    out_dir: &Path,
) -> Result<LocalPkgResult, String> {
    let env_workspace = super::pkg_workspace_env_select::load_workspace(workspace_file, profile)?;
    let workspace = env_workspace.config;
    let lock_model = GenesisLock::load(lock).map_err(|error| error.to_string())?;
    let selected_profile = workspace
        .profiles
        .get(profile)
        .ok_or_else(|| format!("workspace profile `{profile}` not found"))?;
    let workspace_bytes = workspace.to_toml_canonical().into_bytes();
    let lock_bytes = lock_model.to_toml_canonical().into_bytes();
    bounded_bytes(&workspace_bytes, "canonical workspace")?;
    bounded_bytes(&lock_bytes, "canonical lock")?;

    let workspace_root = workspace_file.parent().unwrap_or_else(|| Path::new("."));
    let wasi_root = workspace_root
        .join(".genesis")
        .join("runtime")
        .join("wasi-http-bridge");
    let path_separator = std::path::MAIN_SEPARATOR.to_string();
    let plan_request = map([
        (
            ":active",
            Term::Str(crate::active_runtime_backend_profile().to_string()),
        ),
        (
            ":default-backend",
            optional(env_workspace.default_runtime_backend.as_deref()),
        ),
        (
            ":defaults",
            map([
                (":policy", optional(workspace.defaults.policy.as_deref())),
                (
                    ":registry",
                    optional(workspace.defaults.registry.as_deref()),
                ),
                (
                    ":toolchain",
                    optional(workspace.defaults.toolchain.as_deref()),
                ),
            ]),
        ),
        (
            ":generated-by",
            Term::Str(format!("genesis {}", env!("CARGO_PKG_VERSION"))),
        ),
        (
            ":kind",
            Term::Str(super::pkg_workspace_env_authority::PLAN_KIND.to_string()),
        ),
        (":lock", lock_observation(workspace_file, &lock_model)),
        (":lock-bytes", Term::Bytes(lock_bytes.into())),
        (":members", members_observation(workspace_file, &workspace)?),
        (
            ":out-root-prefix",
            Term::Str(path_prefix(out_dir, &path_separator)),
        ),
        (":override", optional(runtime_backend_override)),
        (":path-separator", Term::Str(path_separator)),
        (
            ":paths",
            map([
                (":lock-file", path_term(lock)),
                (
                    ":store-root",
                    path_term(&super::workspace_store_dir(workspace_file)),
                ),
                (":wasi-http-dir", path_term(&wasi_root.join("http"))),
                (":wasi-https-dir", path_term(&wasi_root.join("https"))),
                (":wasi-root", path_term(&wasi_root)),
                (
                    ":wasi-runtime-file",
                    path_term(&wasi_root.join("runtime.gc")),
                ),
                (":workspace-file", path_term(workspace_file)),
            ]),
        ),
        (":profile", Term::Str(profile.to_string())),
        (
            ":profile-backend",
            optional(env_workspace.profile_runtime_backend.as_deref()),
        ),
        (
            ":profile-values",
            map([
                (
                    ":caps-policy",
                    optional(selected_profile.caps_policy.as_deref()),
                ),
                (":policy", optional(selected_profile.policy.as_deref())),
                (":registry", optional(selected_profile.registry.as_deref())),
                (
                    ":toolchain",
                    optional(selected_profile.toolchain.as_deref()),
                ),
            ]),
        ),
        (":v", Term::Int(1.into())),
        (":workspace", Term::Str(workspace.workspace.clone())),
        (":workspace-bytes", Term::Bytes(workspace_bytes.into())),
    ]);

    let mut backend_plan = None;
    let authorized =
        super::pkg_workspace_env_authority::authorize(cli, plan_request, out_dir, |plan| {
            let caps_path = resolve_workspace_path(workspace_file, &plan.caps_policy_raw);
            let caps_bytes = read_required_file(&caps_path, "caps policy")?;
            let toolchain = plan
                .effective_toolchain
                .as_deref()
                .map(|raw| {
                    let path = resolve_workspace_path(workspace_file, raw);
                    let bytes = read_required_file(&path, "toolchain")?;
                    Ok::<Term, String>(file_observation(&path, bytes))
                })
                .transpose()?;
            let remote_root = match plan.effective_registry.as_deref() {
                Some(remote) if remote.starts_with("http://") || remote.starts_with("https://") => {
                    Some(
                        gc_registry::wasi_http_bridge_resolve_remote_root(&wasi_root, remote)
                            .map_err(|error| {
                                format!(
                                    "resolve wasi http bridge root for registry `{remote}`: {error}"
                                )
                            })?,
                    )
                }
                _ => None,
            };
            let backend = if plan.backend_required {
                let planned =
                    super::pkg_workspace_ops_backend::plan_backend_env_bundle(workspace_file)?;
                let observation = map([
                    (":bridge-cmd", path_term(&planned.bridge_cmd)),
                    (":bridge-ready", Term::Bool(true)),
                    (":bridge-sha256", Term::Str(planned.bridge_sha256.clone())),
                    (
                        ":effective-caps-bytes",
                        Term::Bytes(planned.effective_caps_body.clone().into()),
                    ),
                    (
                        ":effective-caps-h",
                        Term::Str(planned.effective_caps_hash.clone()),
                    ),
                ]);
                backend_plan = Some(planned);
                observation
            } else {
                Term::Nil
            };
            Ok(map([
                (":backend", backend),
                (":caps-policy", file_observation(&caps_path, caps_bytes)),
                (":toolchain", toolchain.unwrap_or(Term::Nil)),
                (
                    ":wasi",
                    map([
                        (":remote-root", optional_path(remote_root.as_deref())),
                        (":root", path_term(&wasi_root)),
                    ]),
                ),
            ]))
        })?;

    super::pkg_workspace_env_materialize::commit(&authorized, backend_plan.as_ref())?;
    let value = authorized.public;
    Ok(LocalPkgResult {
        kind: "genesis/pkg-env-v0.1",
        log_op: "pkg-env",
        program_hash: hash_term(&value),
        value,
    })
}

fn members_observation(workspace_file: &Path, workspace: &WorkspaceConfig) -> Result<Term, String> {
    let workspace_root = workspace_file.parent().unwrap_or_else(|| Path::new("."));
    let mut members = Vec::with_capacity(workspace.members.len());
    for member in &workspace.members {
        let package_file = workspace_root.join(&member.path).join("package.toml");
        let (package_path, package_hash) = match std::fs::read(&package_file) {
            Ok(bytes) => {
                bounded_bytes(&bytes, "member package manifest")?;
                (
                    Term::Str(package_file.display().to_string()),
                    Term::Str(blake3_hex(&bytes)),
                )
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Term::Nil, Term::Nil),
            Err(error) => {
                return Err(format!(
                    "read member package manifest `{}`: {error}",
                    package_file.display()
                ));
            }
        };
        members.push(map([
            (":name", Term::Str(member.name.clone())),
            (":package-file", package_path),
            (":package-h", package_hash),
            (":path", Term::Str(member.path.clone())),
            (":role", optional(member.role.as_deref())),
        ]));
    }
    Ok(Term::Vector(members))
}

fn lock_observation(workspace_file: &Path, lock: &GenesisLock) -> Term {
    let requirements = lock
        .requirements
        .iter()
        .map(|(name, requirement)| {
            map([
                (":name", Term::Str(name.clone())),
                (":registry", optional(requirement.registry.as_deref())),
                (":selector", Term::Str(requirement.selector.clone())),
                (
                    ":strategy",
                    Term::Str(requirement.strategy.as_str().to_string()),
                ),
                (":tag-policy", optional(requirement.tag_policy.as_deref())),
                (
                    ":update-policy",
                    Term::Str(
                        match requirement.update_policy {
                            UpdatePolicy::Manual => "manual",
                            UpdatePolicy::Auto => "auto",
                        }
                        .to_string(),
                    ),
                ),
            ])
        })
        .collect();
    let store = super::workspace_store_dir(workspace_file);
    let locked = lock
        .locked
        .iter()
        .map(|(name, entry)| {
            let snapshot_path = store.join(&entry.snapshot);
            let (commit_path, commit_present) = entry
                .commit
                .as_deref()
                .map(|commit| {
                    let path = store.join(commit);
                    let present = path.is_file();
                    (Term::Str(path.display().to_string()), present)
                })
                .unwrap_or((Term::Nil, true));
            map([
                (":commit", optional(entry.commit.as_deref())),
                (":commit-path", commit_path),
                (":commit-present", Term::Bool(commit_present)),
                (
                    ":environment-fingerprint",
                    optional(entry.environment_fingerprint.as_deref()),
                ),
                (":exports-h", optional(entry.exports_hash.as_deref())),
                (":name", Term::Str(name.clone())),
                (":registry", optional(entry.registry.as_deref())),
                (":resolved-ref", optional(entry.resolved_ref.as_deref())),
                (":snapshot", Term::Str(entry.snapshot.clone())),
                (":snapshot-path", path_term(&snapshot_path)),
                (":snapshot-present", Term::Bool(snapshot_path.is_file())),
                (":source-selector", Term::Str(entry.source_selector.clone())),
            ])
        })
        .collect();
    map([
        (":locked", Term::Vector(locked)),
        (":requirements", Term::Vector(requirements)),
    ])
}

fn file_observation(path: &Path, bytes: Vec<u8>) -> Term {
    map([
        (":bytes", Term::Bytes(bytes.clone().into())),
        (":h", Term::Str(blake3_hex(&bytes))),
        (":path", path_term(path)),
    ])
}

fn read_required_file(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("{label} file not found `{}`: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "{label} must be a regular non-symlink file: {}",
            path.display()
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("read {label} `{}`: {error}", path.display()))?;
    bounded_bytes(&bytes, label)?;
    Ok(bytes)
}

fn resolve_workspace_path(workspace_file: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        workspace_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

fn path_prefix(path: &Path, separator: &str) -> String {
    let mut value = path.display().to_string();
    if !value.ends_with(separator) {
        value.push_str(separator);
    }
    value
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn optional(value: Option<&str>) -> Term {
    super::pkg_workspace_env_authority::optional(value)
}

fn optional_path(value: Option<&Path>) -> Term {
    value.map(path_term).unwrap_or(Term::Nil)
}

fn path_term(path: &Path) -> Term {
    Term::Str(path.display().to_string())
}

fn bounded_bytes(bytes: &[u8], label: &str) -> Result<(), String> {
    if bytes.len() <= BODY_LIMIT {
        Ok(())
    } else {
        Err(format!(
            "{label} exceeds {BODY_LIMIT}-byte workspace environment limit"
        ))
    }
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

pub(super) fn write_if_same_or_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if path.is_file() {
        let old = std::fs::read(path)?;
        if old == bytes {
            return Ok(());
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "refusing to overwrite immutable env artifact at {}",
                path.display()
            ),
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::pkg_scaffold::atomic_write_text(path, bytes)
}
