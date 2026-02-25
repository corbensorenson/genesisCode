use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_pkg::{
    GenesisLock, RUNTIME_BACKEND_BACKEND, RUNTIME_BACKEND_GFX, RUNTIME_BACKEND_GPU,
    RUNTIME_BACKEND_HEADLESS, WorkspaceConfig, WorkspaceMember, WorkspaceProfile, WorkspaceTask,
    normalize_runtime_backend_profile,
};

use crate::pkg_caps_templates::{
    CAPS_CI_DEFAULT, CAPS_DEV_DEFAULT, CAPS_RELEASE_DEFAULT, render_backend_caps_policy,
};
use crate::pkg_workspace_ops::LocalPkgResult;

pub(crate) struct PkgScaffoldArgs<'a> {
    pub(crate) archetype: &'a str,
    pub(crate) name: &'a str,
    pub(crate) root: &'a Path,
    pub(crate) force: bool,
    pub(crate) runtime_backend: Option<&'a str>,
    pub(crate) policy: &'a str,
    pub(crate) registry_default: Option<&'a str>,
}

pub(crate) fn handle_scaffold(args: PkgScaffoldArgs<'_>) -> Result<LocalPkgResult, String> {
    let archetype = Archetype::parse(args.archetype)?;
    let workspace_name = normalize_identifier(args.name);
    if workspace_name.is_empty() {
        return Err("scaffold name must contain alphanumeric characters".to_string());
    }
    let module_suffix = workspace_name.replace('-', "_");
    let module_ns = format!("pkg/{module_suffix}");
    let package_name = format!("{workspace_name}-{}", archetype.id());
    let runtime_backend = resolve_runtime_backend(archetype, args.runtime_backend)?;

    let mut ws = WorkspaceConfig::empty(workspace_name.clone());
    ws.members = vec![WorkspaceMember {
        name: package_name.clone(),
        path: ".".to_string(),
        role: Some("app".to_string()),
    }];
    ws.defaults.policy = Some(args.policy.to_string());
    ws.defaults.runtime_backend = Some(runtime_backend.clone());
    ws.defaults.registry = args.registry_default.map(|s| s.to_string());
    ws.profiles = build_workspace_profiles(
        args.policy,
        args.registry_default,
        runtime_backend.as_str(),
        archetype,
    );
    ws.tasks = build_workspace_tasks(archetype);

    let mut lock = GenesisLock::empty(workspace_name.clone());
    lock.policy = args.policy.to_string();
    if let Some(registry_default) = args.registry_default {
        lock.registries
            .insert("default".to_string(), registry_default.to_string());
    }

    let ws_body = ws.to_toml_canonical();
    let lock_body = lock.to_toml_canonical();
    let package_body = render_package_toml(&package_name);
    let module_body = render_module_template(&module_ns, archetype);
    let deploy_body = render_deploy_preset(archetype, runtime_backend.as_str());
    let readme_body = render_readme(
        &workspace_name,
        &package_name,
        archetype,
        runtime_backend.as_str(),
    );
    let backend_caps_body = render_backend_caps_policy(None, None);

    let files: Vec<(PathBuf, String)> = vec![
        (PathBuf::from("genesis.workspace.toml"), ws_body),
        (PathBuf::from("genesis.lock"), lock_body),
        (PathBuf::from("package.toml"), package_body),
        (PathBuf::from("src/main.gc"), module_body),
        (PathBuf::from("deploy/presets.toml"), deploy_body),
        (PathBuf::from("caps.toml"), CAPS_DEV_DEFAULT.to_string()),
        (PathBuf::from("caps.ci.toml"), CAPS_CI_DEFAULT.to_string()),
        (
            PathBuf::from("caps.release.toml"),
            CAPS_RELEASE_DEFAULT.to_string(),
        ),
        (PathBuf::from("caps.backend.toml"), backend_caps_body),
        (PathBuf::from("README.gcpm.md"), readme_body),
    ];

    for (rel, body) in &files {
        let path = args.root.join(rel);
        write_scaffold_file(&path, body.as_bytes(), args.force)?;
    }

    let mut file_hash_records = Vec::with_capacity(files.len());
    let mut rel_paths = Vec::with_capacity(files.len());
    for (rel, body) in &files {
        let rel_s = rel.display().to_string();
        let file_h = blake3::hash(body.as_bytes()).to_hex().to_string();
        file_hash_records.push(format!("{rel_s}:{file_h}"));
        rel_paths.push(Term::Str(rel_s));
    }
    file_hash_records.sort();
    let scaffold_h = blake3::hash(file_hash_records.join("\n").as_bytes())
        .to_hex()
        .to_string();

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(workspace_name),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(package_name),
            ),
            (
                TermOrdKey(Term::symbol(":archetype")),
                Term::Str(archetype.id().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-backend-profile")),
                Term::Str(runtime_backend),
            ),
            (
                TermOrdKey(Term::symbol(":root")),
                Term::Str(args.root.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":files-written")),
                Term::Int((rel_paths.len() as i64).into()),
            ),
            (TermOrdKey(Term::symbol(":files")), Term::Vector(rel_paths)),
            (
                TermOrdKey(Term::symbol(":scaffold-h")),
                Term::Str(scaffold_h),
            ),
        ]
        .into_iter()
        .collect(),
    );

    Ok(LocalPkgResult {
        kind: "genesis/pkg-scaffold-v0.1",
        log_op: "pkg-scaffold",
        program_hash: hash_term(&value),
        value,
    })
}

#[derive(Clone, Copy)]
enum Archetype {
    Web,
    Service,
    Desktop,
    Mobile,
    XrGame,
    DataAi,
}

impl Archetype {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "web" => Ok(Self::Web),
            "service" => Ok(Self::Service),
            "desktop" => Ok(Self::Desktop),
            "mobile" => Ok(Self::Mobile),
            "xr-game" => Ok(Self::XrGame),
            "data-ai" => Ok(Self::DataAi),
            _ => Err(
                "unknown archetype; expected one of: web|service|desktop|mobile|xr-game|data-ai"
                    .to_string(),
            ),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Service => "service",
            Self::Desktop => "desktop",
            Self::Mobile => "mobile",
            Self::XrGame => "xr-game",
            Self::DataAi => "data-ai",
        }
    }

    fn default_runtime_backend(self) -> &'static str {
        match self {
            Self::Web | Self::Desktop | Self::XrGame => RUNTIME_BACKEND_GFX,
            Self::Service => RUNTIME_BACKEND_BACKEND,
            Self::Mobile | Self::DataAi => RUNTIME_BACKEND_GPU,
        }
    }

    fn primary_build_target(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Service => "service-runtime",
            Self::Desktop => "desktop",
            Self::Mobile => "ios",
            Self::XrGame => "web",
            Self::DataAi => "service-runtime",
        }
    }
}

fn resolve_runtime_backend(
    archetype: Archetype,
    runtime_backend_override: Option<&str>,
) -> Result<String, String> {
    let chosen = runtime_backend_override.unwrap_or(archetype.default_runtime_backend());
    normalize_runtime_backend_profile(chosen).ok_or_else(|| {
        format!("invalid runtime backend `{chosen}`; expected one of headless|gpu|gfx|backend")
    })
}

fn build_workspace_profiles(
    policy: &str,
    registry_default: Option<&str>,
    runtime_backend: &str,
    archetype: Archetype,
) -> BTreeMap<String, WorkspaceProfile> {
    let registry = registry_default.map(|s| s.to_string());
    let mut profiles = BTreeMap::new();
    profiles.insert(
        "dev".to_string(),
        WorkspaceProfile {
            caps_policy: Some("caps.toml".to_string()),
            registry: registry.clone(),
            policy: Some(policy.to_string()),
            toolchain: None,
            runtime_backend: Some(runtime_backend.to_string()),
        },
    );
    profiles.insert(
        "backend".to_string(),
        WorkspaceProfile {
            caps_policy: Some("caps.backend.toml".to_string()),
            registry: registry_default.map(|s| s.to_string()),
            policy: Some(policy.to_string()),
            toolchain: None,
            runtime_backend: Some(RUNTIME_BACKEND_BACKEND.to_string()),
        },
    );
    profiles.insert(
        "ci".to_string(),
        WorkspaceProfile {
            caps_policy: Some("caps.ci.toml".to_string()),
            registry: registry.clone(),
            policy: Some(policy.to_string()),
            toolchain: None,
            runtime_backend: Some(RUNTIME_BACKEND_HEADLESS.to_string()),
        },
    );
    profiles.insert(
        "release".to_string(),
        WorkspaceProfile {
            caps_policy: Some("caps.release.toml".to_string()),
            registry,
            policy: Some(policy.to_string()),
            toolchain: None,
            runtime_backend: Some(match archetype {
                Archetype::Service => RUNTIME_BACKEND_BACKEND.to_string(),
                Archetype::DataAi => RUNTIME_BACKEND_GPU.to_string(),
                _ => runtime_backend.to_string(),
            }),
        },
    );
    profiles
}

fn build_workspace_tasks(archetype: Archetype) -> BTreeMap<String, WorkspaceTask> {
    let mut tasks = BTreeMap::new();
    tasks.insert(
        "test".to_string(),
        WorkspaceTask {
            cmd: "test".to_string(),
            file: None,
            pkg: Some("package.toml".to_string()),
            args: vec![],
        },
    );
    tasks.insert(
        "pack".to_string(),
        WorkspaceTask {
            cmd: "pack".to_string(),
            file: None,
            pkg: Some("package.toml".to_string()),
            args: vec![],
        },
    );
    tasks.insert(
        "typecheck".to_string(),
        WorkspaceTask {
            cmd: "typecheck".to_string(),
            file: None,
            pkg: Some("package.toml".to_string()),
            args: vec![],
        },
    );
    tasks.insert(
        "run".to_string(),
        WorkspaceTask {
            cmd: "run".to_string(),
            file: Some("src/main.gc".to_string()),
            pkg: None,
            args: vec!["--caps".to_string(), "caps.toml".to_string()],
        },
    );
    tasks.insert(
        "optimize".to_string(),
        WorkspaceTask {
            cmd: "optimize".to_string(),
            file: Some("src/main.gc".to_string()),
            pkg: None,
            args: vec!["--stage1-gate".to_string()],
        },
    );
    tasks.insert(
        "build-primary".to_string(),
        WorkspaceTask {
            cmd: "build".to_string(),
            file: None,
            pkg: Some("package.toml".to_string()),
            args: vec![
                "--target".to_string(),
                archetype.primary_build_target().to_string(),
            ],
        },
    );
    tasks
}

fn render_package_toml(package_name: &str) -> String {
    format!(
        r#"name = "{package_name}"
version = "0.1.0"
obligations = []
dependencies = []
tests = []
property_tests = []
caps_policy = "caps.toml"

[[modules]]
path = "src/main.gc"
"#
    )
}

fn render_module_template(module_ns: &str, archetype: Archetype) -> String {
    format!(
        r#"(def ::meta
  (quote
    {{
      :caps []
      :exports [{module_ns}::main]
      :types {{{module_ns}::main ?}}}}))

(def {module_ns}::main
  (fn (_)
    {{
      :archetype :{}
      :status "scaffold-ok"}}))

{module_ns}::main
"#,
        archetype.id()
    )
}

fn render_deploy_preset(archetype: Archetype, runtime_backend: &str) -> String {
    let mut out = format!(
        r#"schema = "genesis/gcpm-scaffold-deploy-presets-v0.1"
archetype = "{}"
runtime_backend = "{}"
primary_target = "{}"
"#,
        archetype.id(),
        runtime_backend,
        archetype.primary_build_target(),
    );
    if matches!(archetype, Archetype::Mobile) {
        out.push_str("secondary_targets = [\"android\"]\n");
    } else {
        out.push_str("secondary_targets = []\n");
    }
    out
}

fn render_readme(
    workspace_name: &str,
    package_name: &str,
    archetype: Archetype,
    runtime_backend: &str,
) -> String {
    format!(
        r#"# {workspace_name}

Deterministic `gcpm scaffold` workspace for archetype `{}`.

## Quick Start

1. `genesis gcpm --caps caps.toml test --pkg package.toml`
2. `genesis gcpm --caps caps.toml run run`
3. `genesis gcpm --caps caps.toml env --profile dev`
4. `genesis gcpm --caps caps.toml build --pkg package.toml --target {}`

## Scaffold Contract

- package: `{package_name}`
- runtime backend profile: `{runtime_backend}`
- deploy presets: `deploy/presets.toml`
"#,
        archetype.id(),
        archetype.primary_build_target(),
    )
}

fn normalize_identifier(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.trim().chars() {
        let normalized = match ch {
            'a'..='z' | '0'..='9' => Some(ch),
            'A'..='Z' => Some(ch.to_ascii_lowercase()),
            '_' | '-' | ' ' => Some('-'),
            _ => None,
        };
        if let Some(c) = normalized {
            if c == '-' {
                if out.is_empty() || prev_dash {
                    continue;
                }
                prev_dash = true;
                out.push(c);
            } else {
                prev_dash = false;
                out.push(c);
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
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

fn atomic_write_text(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let pid = std::process::id();
    let tmp = parent.join(format!(
        ".{}.tmp-{pid}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("write")
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::{Archetype, normalize_identifier, resolve_runtime_backend};

    #[test]
    fn normalize_identifier_compacts_and_lowers() {
        assert_eq!(normalize_identifier("  My Demo_App  "), "my-demo-app");
        assert_eq!(normalize_identifier("!!!"), "");
    }

    #[test]
    fn resolve_runtime_backend_validates_aliases() {
        assert_eq!(
            resolve_runtime_backend(Archetype::Web, Some("profile-gfx")).unwrap(),
            "gfx".to_string()
        );
        assert!(resolve_runtime_backend(Archetype::Web, Some("weird")).is_err());
    }
}
