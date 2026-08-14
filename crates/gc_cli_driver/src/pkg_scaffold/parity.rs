use super::*;
use gc_pkg::{
    GenesisLock, RUNTIME_BACKEND_BACKEND, RUNTIME_BACKEND_GFX, RUNTIME_BACKEND_GPU,
    RUNTIME_BACKEND_HEADLESS, WorkspaceConfig, WorkspaceMember, WorkspaceProfile, WorkspaceTask,
    normalize_runtime_backend_profile,
};

#[cfg(any(test, feature = "parity-harness"))]
pub(super) fn handle_scaffold_parity(args: PkgScaffoldArgs<'_>) -> Result<LocalPkgResult, String> {
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
        r#"schema = 1
name = "{package_name}"
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

#[cfg(test)]
mod tests {
    use super::{
        Archetype, PkgScaffoldArgs, handle_scaffold_parity, normalize_identifier,
        resolve_runtime_backend,
    };

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

    #[test]
    fn retained_oracle_sample_has_stable_file_identities() {
        let temp = std::env::temp_dir().join(format!(
            "genesis-scaffold-oracle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = temp.join("out");
        let result = handle_scaffold_parity(PkgScaffoldArgs {
            archetype: "web",
            name: "My Demo_App",
            root: &root,
            force: false,
            runtime_backend: None,
            policy: "policy:default-v0.1",
            registry_default: Some("gen://registry"),
        })
        .unwrap();
        let expected = [
            (
                "genesis.workspace.toml",
                "7f8ecca894cc2c84f1e3cb4b33dda1db3050318cbe38c6ba441c657199a7a78e",
            ),
            (
                "genesis.lock",
                "601eaaddaf704a3467054df3ee8add4209efbe4007323b407f7801d1bc6909b0",
            ),
            (
                "package.toml",
                "ec78936f91b4a245f825fa35c14454f030b1c623299594c1d7687a0d509d542c",
            ),
            (
                "src/main.gc",
                "a66e59ce024719c6462f3edd24baa68ab95198b16cf6e5efe3675b64a0883d61",
            ),
            (
                "deploy/presets.toml",
                "3200130d34e9e0d5ef86ffe11691d2419d5a88a3ca5fce94136171cecd8026db",
            ),
            (
                "caps.toml",
                "c59cc9fc2d22e351df9f1ca0993f5287747dd04424dd2ab29dc9c40b5feeaebe",
            ),
            (
                "caps.ci.toml",
                "263a3a57675d9f02d3b7f3e63e567556e26e9a8826c9c2f18a3de2608707bc1f",
            ),
            (
                "caps.release.toml",
                "facc334d775e73441b8861a5119af13be4b4f53fa548e039d99de28a7a78a388",
            ),
            (
                "caps.backend.toml",
                "df2d9cd700e2e45ee809db493119db52882baf94af8fc1b6f343d3de97d491c7",
            ),
            (
                "README.gcpm.md",
                "7570b6991c55accc88d837cbf079887b688ba1eabc2b6a36f82ba372f4e94927",
            ),
        ];
        for (path, expected_hash) in expected {
            let body = std::fs::read(root.join(path)).unwrap();
            assert_eq!(
                blake3::hash(&body).to_hex().as_str(),
                expected_hash,
                "{path}"
            );
        }
        let report = match result.value {
            gc_coreform::Term::Map(fields) => fields,
            _ => panic!("oracle report must be map"),
        };
        assert_eq!(
            report
                .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
                    ":scaffold-h",
                )))
                .and_then(|value| match value {
                    gc_coreform::Term::Str(value) => Some(value.as_str()),
                    _ => None,
                }),
            Some("aaf0e92bbba88301783207edfe8d637cf3bc9429e8f3b6ff3042b6507c36ca1f")
        );
        std::fs::remove_dir_all(temp).unwrap();
    }
}
