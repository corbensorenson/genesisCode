use std::path::Path;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::Value;

use super::PkgCmd;

pub(crate) fn build_pkg_ai_report(
    cmd: &PkgCmd,
    value: &Value,
    caps: &Path,
) -> Option<serde_json::Value> {
    match cmd {
        PkgCmd::Add { spec, lock, .. } => Some(build_add_report(caps, spec, lock)),
        PkgCmd::Remove { name, lock } => Some(build_remove_report(caps, name, lock, value)),
        PkgCmd::Lock { lock, strict } => Some(build_lock_report(value, caps, lock, *strict)),
        PkgCmd::Update { lock, only } => Some(build_update_report(value, caps, lock, only)),
        PkgCmd::Run {
            task,
            workspace_file,
        } => Some(build_run_report(caps, task, workspace_file)),
        PkgCmd::Build {
            pkg,
            target,
            out_dir,
        } => Some(build_build_report(caps, pkg, target, out_dir, value)),
        PkgCmd::SelfOptimize { pkg, dry_run, .. } => {
            Some(build_self_optimize_report(value, caps, pkg, *dry_run))
        }
        PkgCmd::Install {
            lock,
            frozen,
            strict,
        } => Some(build_install_report(caps, lock, *frozen, *strict, value)),
        PkgCmd::Verify { lock, .. } => Some(build_verify_report(caps, lock, value)),
        PkgCmd::Doctor { lock, .. } => Some(build_doctor_ai_report(caps, lock)),
        PkgCmd::Env {
            profile,
            lock,
            workspace_file,
            out_dir,
            hydrate,
            ..
        } => Some(build_env_report(
            caps,
            profile,
            lock,
            workspace_file,
            out_dir,
            *hydrate,
            value,
        )),
        PkgCmd::Publish {
            remote,
            refname,
            policy,
            expected_old,
            depth,
            commit,
        } => Some(build_publish_report(
            value,
            PublishReport {
                caps,
                remote,
                refname,
                policy,
                expected_old: expected_old.as_deref(),
                depth: *depth,
                commit: commit.as_deref(),
            },
        )),
        PkgCmd::Bridge {
            ecosystem,
            name,
            version,
            source,
            source_hash,
            lock,
            dep_name,
            registry,
            ..
        } => Some(build_bridge_report(
            value,
            BridgeReport {
                caps,
                ecosystem,
                name,
                version,
                source,
                source_hash,
                lock: lock.as_deref(),
                dep_name: dep_name.as_deref(),
                registry: registry.as_deref(),
            },
        )),
        _ => None,
    }
}

fn build_add_report(caps: &Path, spec: &str, lock: &Path) -> serde_json::Value {
    serde_json::json!({
        "schema": "genesis/pkg-add-report-v0.1",
        "workflow": "add",
        "changed": true,
        "spec": spec,
        "lock": lock.display().to_string(),
        "why": "registered deterministic dependency requirement selector in lock requirements",
        "fix_options": [
            {
                "id": "lock-now",
                "command": format!("genesis gcpm --caps {} lock --lock {} --strict", caps.display(), lock.display()),
                "why": "resolve and pin the new requirement into deterministic commit/snapshot entries"
            },
            {
                "id": "doctor-lock",
                "command": format!("genesis gcpm --caps {} doctor --lock {}", caps.display(), lock.display()),
                "why": "surface deterministic remediation if requirement metadata is inconsistent"
            }
        ]
    })
}

fn build_remove_report(caps: &Path, name: &str, lock: &Path, value: &Value) -> serde_json::Value {
    let removed = map_get_bool(value, ":removed").unwrap_or(false);
    serde_json::json!({
        "schema": "genesis/pkg-remove-report-v0.1",
        "workflow": "remove",
        "changed": removed,
        "name": name,
        "lock": lock.display().to_string(),
        "why": if removed {
            "removed requirement and associated locked entry from deterministic lock state"
        } else {
            "dependency entry was not present in lock requirements"
        },
        "fix_options": [
            {
                "id": "verify-lock",
                "command": format!("genesis gcpm --caps {} verify --lock {}", caps.display(), lock.display()),
                "why": "confirm lock closure remains valid after removal"
            },
            {
                "id": "doctor-lock",
                "command": format!("genesis gcpm --caps {} doctor --lock {}", caps.display(), lock.display()),
                "why": "emit deterministic remediation if removal exposed stale references"
            }
        ]
    })
}

fn build_self_optimize_report(
    value: &Value,
    caps: &Path,
    pkg: &Path,
    dry_run: bool,
) -> serde_json::Value {
    let promoted = map_get_bool(value, ":promoted").unwrap_or(false);
    let promotable = map_get_bool(value, ":promotable").unwrap_or(false);
    let proposed_count = map_get_int(value, ":proposed-count").unwrap_or(0);
    let acceptance_artifact = map_get_str(value, ":acceptance-artifact");
    let translation_artifact = map_get_str(value, ":translation-artifact");
    let report_artifact = map_get_str(value, ":report-artifact");

    serde_json::json!({
        "schema": "genesis/pkg-self-optimize-report-v0.1",
        "workflow": "self-optimize",
        "pkg": pkg.display().to_string(),
        "dry_run": dry_run,
        "proposed_count": proposed_count,
        "promotable": promotable,
        "promoted": promoted,
        "acceptance_artifact": acceptance_artifact,
        "translation_artifact": translation_artifact,
        "report_artifact": report_artifact,
        "why": if promoted {
            "promoted optimized module rewrites after translation-validation and obligation success"
        } else if promotable {
            "candidate rewrites are promotable but were not applied (dry-run)"
        } else {
            "candidate rewrites failed obligation gate and were rolled back"
        },
        "fix_options": [
            {
                "id": "show-report",
                "command": report_artifact
                    .as_ref()
                    .map(|h| format!("genesis store --caps {} get {} --out self-optimize-report.gc", caps.display(), h))
                    .unwrap_or_else(|| "genesis gcpm self-optimize --pkg package.toml".to_string()),
                "why": "inspect deterministic self-optimization evidence/proof payload"
            },
            {
                "id": "rerun",
                "command": format!("genesis gcpm --caps {} self-optimize --pkg {}", caps.display(), pkg.display()),
                "why": "retry optimization promotion with current package state"
            }
        ]
    })
}

fn build_run_report(caps: &Path, task: &str, workspace_file: &Path) -> serde_json::Value {
    serde_json::json!({
        "schema": "genesis/pkg-run-report-v0.1",
        "workflow": "run",
        "changed": false,
        "task": task,
        "workspace_file": workspace_file.display().to_string(),
        "why": "executed deterministic workspace task contract",
        "fix_options": [
            {
                "id": "doctor-lock",
                "command": format!("genesis gcpm --caps {} doctor --lock genesis.lock", caps.display()),
                "why": "collect deterministic diagnostics when a task fails due to dependency/capability drift"
            },
            {
                "id": "verify-lock",
                "command": format!("genesis gcpm --caps {} verify --lock genesis.lock", caps.display()),
                "why": "verify that run-time task inputs remain lock-consistent"
            }
        ]
    })
}

fn build_build_report(
    caps: &Path,
    pkg: &Path,
    target: &str,
    out_dir: &Path,
    value: &Value,
) -> serde_json::Value {
    let bundle_hash = map_get_str(value, ":bundle-h");
    serde_json::json!({
        "schema": "genesis/pkg-build-report-v0.1",
        "workflow": "build",
        "changed": true,
        "pkg": pkg.display().to_string(),
        "target": target,
        "out_dir": out_dir.display().to_string(),
        "bundle_hash": bundle_hash,
        "why": "emitted deterministic target bundle + provenance metadata for deployment/runtime handoff",
        "fix_options": [
            {
                "id": "verify-lock",
                "command": format!("genesis gcpm --caps {} verify --lock genesis.lock", caps.display()),
                "why": "ensure build inputs remain pinned and reproducible"
            },
            {
                "id": "rebuild-target",
                "command": format!("genesis gcpm --caps {} build --pkg {} --target {} --out-dir {}", caps.display(), pkg.display(), target, out_dir.display()),
                "why": "re-run deterministic bundle production after addressing target/runtime mismatch"
            }
        ]
    })
}

fn build_lock_report(value: &Value, caps: &Path, lock: &Path, strict: bool) -> serde_json::Value {
    let lock_hash = map_get_str(value, ":lock-h");
    let locked_count = map_get_int(value, ":locked-count").unwrap_or(0);
    let rationale_count = map_get_int(value, ":rationale-count").unwrap_or(0);
    let rationale_artifact = map_get_str(value, ":rationale-artifact");
    let changed = locked_count > 0;

    serde_json::json!({
        "schema": "genesis/pkg-lock-report-v0.1",
        "workflow": "lock",
        "changed": changed,
        "lock": lock.display().to_string(),
        "lock_hash": lock_hash,
        "locked_count": locked_count,
        "rationale_count": rationale_count,
        "rationale_artifact": rationale_artifact,
        "strict": strict,
        "why": "resolved requirements into deterministic locked commit/snapshot entries",
        "fix_options": [
            {
                "id": "verify-lock",
                "command": format!("genesis gcpm --caps {} verify --lock {}", caps.display(), lock.display()),
                "why": "validate lock and artifact closure after resolution"
            },
            {
                "id": "doctor-lock",
                "command": format!("genesis gcpm --caps {} doctor --lock {}", caps.display(), lock.display()),
                "why": "produce machine-actionable diagnostics and remediation hints"
            }
        ]
    })
}

fn build_update_report(
    value: &Value,
    caps: &Path,
    lock: &Path,
    only: &[String],
) -> serde_json::Value {
    let lock_hash = map_get_str(value, ":lock-h");
    let updated_count = map_get_int(value, ":updated").unwrap_or(0);
    let selected_count = map_get_int(value, ":selected-count").unwrap_or(0);
    let rationale_count = map_get_int(value, ":rationale-count").unwrap_or(0);
    let rationale_artifact = map_get_str(value, ":rationale-artifact");
    let changed = updated_count > 0;

    serde_json::json!({
        "schema": "genesis/pkg-update-report-v0.1",
        "workflow": "update",
        "changed": changed,
        "lock": lock.display().to_string(),
        "lock_hash": lock_hash,
        "updated_count": updated_count,
        "selected_count": selected_count,
        "rationale_count": rationale_count,
        "rationale_artifact": rationale_artifact,
        "only": only,
        "why": if changed {
            "advanced tracked dependencies and rewrote lock deterministically"
        } else {
            "no tracked dependency required advancement"
        },
        "fix_options": [
            {
                "id": "verify-lock",
                "command": format!("genesis gcpm --caps {} verify --lock {}", caps.display(), lock.display()),
                "why": "validate post-update lock integrity"
            },
            {
                "id": "install-lock",
                "command": format!("genesis gcpm --caps {} install --lock {} --strict", caps.display(), lock.display()),
                "why": "materialize and strictly validate updated artifacts"
            }
        ]
    })
}

fn build_install_report(
    caps: &Path,
    lock: &Path,
    frozen: bool,
    strict: bool,
    value: &Value,
) -> serde_json::Value {
    let ok = map_get_bool(value, ":ok").unwrap_or(true);
    serde_json::json!({
        "schema": "genesis/pkg-install-report-v0.1",
        "workflow": "install",
        "changed": false,
        "lock": lock.display().to_string(),
        "frozen": frozen,
        "strict": strict,
        "ok": ok,
        "why": if ok {
            "verified and materialized deterministic lock closure into local store state"
        } else {
            "install verification failed for one or more locked artifacts"
        },
        "fix_options": [
            {
                "id": "doctor-lock",
                "command": format!("genesis gcpm --caps {} doctor --lock {}", caps.display(), lock.display()),
                "why": "emit deterministic remediation for missing or invalid lock artifacts"
            },
            {
                "id": "verify-lock",
                "command": format!("genesis gcpm --caps {} verify --lock {}", caps.display(), lock.display()),
                "why": "re-run strict lock integrity checks before retrying install"
            }
        ]
    })
}

fn build_verify_report(caps: &Path, lock: &Path, value: &Value) -> serde_json::Value {
    let ok = map_get_bool(value, ":ok").unwrap_or(true);
    serde_json::json!({
        "schema": "genesis/pkg-verify-report-v0.1",
        "workflow": "verify",
        "changed": false,
        "lock": lock.display().to_string(),
        "ok": ok,
        "why": if ok {
            "validated deterministic lock closure and artifact references"
        } else {
            "verification found deterministic lock/artifact integrity failures"
        },
        "fix_options": [
            {
                "id": "doctor-lock",
                "command": format!("genesis gcpm --caps {} doctor --lock {}", caps.display(), lock.display()),
                "why": "produce deterministic remediation plan for verification failures"
            },
            {
                "id": "install-strict",
                "command": format!("genesis gcpm --caps {} install --lock {} --strict", caps.display(), lock.display()),
                "why": "attempt strict materialization path to close missing artifact gaps"
            }
        ]
    })
}

fn build_doctor_ai_report(caps: &Path, lock: &Path) -> serde_json::Value {
    serde_json::json!({
        "schema": "genesis/pkg-doctor-ai-report-v0.1",
        "workflow": "doctor",
        "changed": false,
        "lock": lock.display().to_string(),
        "why": "generated deterministic diagnostics and machine-actionable remediation hints",
        "fix_options": [
            {
                "id": "verify-lock",
                "command": format!("genesis gcpm --caps {} verify --lock {}", caps.display(), lock.display()),
                "why": "confirm doctor-advised remediations repaired lock/artifact integrity"
            },
            {
                "id": "install-strict",
                "command": format!("genesis gcpm --caps {} install --lock {} --strict", caps.display(), lock.display()),
                "why": "materialize and validate deterministic closure after remediation"
            }
        ]
    })
}

fn build_env_report(
    caps: &Path,
    profile: &str,
    lock: &Path,
    workspace_file: &Path,
    out_dir: &Path,
    hydrate: bool,
    value: &Value,
) -> serde_json::Value {
    let env_root = map_get_str(value, ":env-root");
    serde_json::json!({
        "schema": "genesis/pkg-env-report-v0.1",
        "workflow": "env",
        "changed": true,
        "profile": profile,
        "lock": lock.display().to_string(),
        "workspace_file": workspace_file.display().to_string(),
        "out_dir": out_dir.display().to_string(),
        "hydrate": hydrate,
        "env_root": env_root,
        "why": "realized deterministic workspace environment profile for agent/runtime execution",
        "fix_options": [
            {
                "id": "verify-lock",
                "command": format!("genesis gcpm --caps {} verify --lock {}", caps.display(), lock.display()),
                "why": "confirm lock and artifact closure before re-materializing environment"
            },
            {
                "id": "rebuild-env",
                "command": format!("genesis gcpm --caps {} env --profile {} --lock {} --workspace-file {} --out-dir {}{}", caps.display(), profile, lock.display(), workspace_file.display(), out_dir.display(), if hydrate { " --hydrate" } else { "" }),
                "why": "re-run deterministic environment realization after remediation"
            }
        ]
    })
}

struct PublishReport<'a> {
    caps: &'a Path,
    remote: &'a str,
    refname: &'a str,
    policy: &'a str,
    expected_old: Option<&'a str>,
    depth: u64,
    commit: Option<&'a str>,
}

fn build_publish_report(value: &Value, report: PublishReport<'_>) -> serde_json::Value {
    let PublishReport {
        caps,
        remote,
        refname,
        policy,
        expected_old,
        depth,
        commit,
    } = report;
    let published_commit = map_get_str(value, ":commit");
    let changed = published_commit.is_some();

    serde_json::json!({
        "schema": "genesis/pkg-publish-report-v0.1",
        "workflow": "publish",
        "changed": changed,
        "remote": remote,
        "ref": refname,
        "policy": policy,
        "depth": depth,
        "requested_commit": commit,
        "published_commit": published_commit,
        "expected_old": expected_old,
        "why": "planned artifact closure upload and policy-gated remote ref advancement",
        "fix_options": [
            {
                "id": "verify-lock",
                "command": format!("genesis gcpm --caps {} verify --lock genesis.lock", caps.display()),
                "why": "ensure lock/evidence integrity before publish retries"
            },
            {
                "id": "doctor-lock",
                "command": format!("genesis gcpm --caps {} doctor --lock genesis.lock", caps.display()),
                "why": "surface deterministic remediation paths for publish blockers"
            }
        ]
    })
}

struct BridgeReport<'a> {
    caps: &'a Path,
    ecosystem: &'a str,
    name: &'a str,
    version: &'a str,
    source: &'a str,
    source_hash: &'a str,
    lock: Option<&'a Path>,
    dep_name: Option<&'a str>,
    registry: Option<&'a str>,
}

fn build_bridge_report(value: &Value, report: BridgeReport<'_>) -> serde_json::Value {
    let BridgeReport {
        caps,
        ecosystem,
        name,
        version,
        source,
        source_hash,
        lock,
        dep_name,
        registry,
    } = report;
    let commit = map_get_str(value, ":commit");
    let snapshot = map_get_str(value, ":snapshot");
    let provenance_root = map_get_str(value, ":provenance-root");
    let evidence = map_get_str(value, ":conversion-evidence");
    let attestation = map_get_str(value, ":attestation");
    serde_json::json!({
        "schema": "genesis/pkg-bridge-report-v0.1",
        "workflow": "bridge",
        "changed": commit.is_some(),
        "ecosystem": ecosystem,
        "name": name,
        "version": version,
        "source": source,
        "source_hash": source_hash,
        "commit": commit,
        "snapshot": snapshot,
        "provenance_root": provenance_root,
        "conversion_evidence": evidence,
        "attestation": attestation,
        "lock": lock.map(|p| p.display().to_string()),
        "dep_name": dep_name,
        "registry": registry,
        "why": "transformed external package coordinate into deterministic signed GenesisPkg commit/snapshot artifacts with replayable conversion evidence",
        "fix_options": [
            {
                "id": "verify-lock",
                "command": lock
                    .map(|p| format!("genesis gcpm --caps {} verify --lock {}", caps.display(), p.display()))
                    .unwrap_or_else(|| format!("genesis gcpm --caps {} verify --lock genesis.lock", caps.display())),
                "why": "verify pinned lock closure after bridge conversion"
            },
            {
                "id": "publish-bridge",
                "command": "genesis gcpm --caps <caps> publish --remote <remote> --ref refs/heads/main --policy <policy-h> --commit <bridge-commit-h>".to_string(),
                "why": "promote mirrored artifact lineage into registry refs after policy review"
            }
        ]
    })
}

fn map_get_str(value: &Value, key: &str) -> Option<String> {
    let term = value.to_term_for_log(None);
    let Term::Map(m) = term else {
        return None;
    };
    m.get(&TermOrdKey(Term::symbol(key))).and_then(|t| match t {
        Term::Str(s) => Some(s.clone()),
        _ => None,
    })
}

fn map_get_int(value: &Value, key: &str) -> Option<i64> {
    let term = value.to_term_for_log(None);
    let Term::Map(m) = term else {
        return None;
    };
    m.get(&TermOrdKey(Term::symbol(key))).and_then(|t| match t {
        Term::Int(i) => i.to_string().parse::<i64>().ok(),
        _ => None,
    })
}

fn map_get_bool(value: &Value, key: &str) -> Option<bool> {
    let term = value.to_term_for_log(None);
    let Term::Map(m) = term else {
        return None;
    };
    m.get(&TermOrdKey(Term::symbol(key))).and_then(|t| match t {
        Term::Bool(b) => Some(*b),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::build_pkg_ai_report;
    use crate::PkgCmd;
    use gc_coreform::Term;
    use gc_kernel::Value;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn publish_report_includes_remote_ref_and_schema() {
        let mut m = BTreeMap::new();
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":commit")),
            Term::Str("c".repeat(64)),
        );
        let value = Value::data(Term::Map(m));
        let cmd = PkgCmd::Publish {
            remote: "gen://registry".to_string(),
            refname: "refs/heads/main".to_string(),
            policy: "a".repeat(64),
            expected_old: Some("b".repeat(64)),
            depth: 1,
            commit: None,
        };
        let report = build_pkg_ai_report(&cmd, &value, &PathBuf::from("caps.toml")).unwrap();
        assert_eq!(
            report.get("schema").and_then(|v| v.as_str()),
            Some("genesis/pkg-publish-report-v0.1")
        );
        assert_eq!(
            report.get("remote").and_then(|v| v.as_str()),
            Some("gen://registry")
        );
        assert_eq!(
            report.get("ref").and_then(|v| v.as_str()),
            Some("refs/heads/main")
        );
        let expected_commit = "c".repeat(64);
        assert_eq!(
            report.get("published_commit").and_then(|v| v.as_str()),
            Some(expected_commit.as_str())
        );
    }

    #[test]
    fn update_report_carries_selection_and_rationale_metrics() {
        let mut m = BTreeMap::new();
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":lock-h")),
            Term::Str("d".repeat(64)),
        );
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":updated")),
            Term::Int(0.into()),
        );
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":selected-count")),
            Term::Int(0.into()),
        );
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":rationale-count")),
            Term::Int(1.into()),
        );
        m.insert(
            gc_coreform::TermOrdKey(Term::symbol(":rationale-artifact")),
            Term::Str("e".repeat(64)),
        );
        let value = Value::data(Term::Map(m));
        let cmd = PkgCmd::Update {
            lock: PathBuf::from("genesis.lock"),
            only: vec!["missing-dep".to_string()],
        };
        let report = build_pkg_ai_report(&cmd, &value, &PathBuf::from("caps.toml")).unwrap();
        assert_eq!(
            report.get("schema").and_then(|v| v.as_str()),
            Some("genesis/pkg-update-report-v0.1")
        );
        assert_eq!(
            report.get("selected_count").and_then(|v| v.as_i64()),
            Some(0)
        );
        assert_eq!(
            report.get("rationale_count").and_then(|v| v.as_i64()),
            Some(1)
        );
        assert_eq!(
            report.get("rationale_artifact").and_then(|v| v.as_str()),
            Some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")
        );
        assert_eq!(
            report.pointer("/only/0").and_then(|v| v.as_str()),
            Some("missing-dep")
        );
    }

    #[test]
    fn extended_workflow_reports_cover_operational_surface() {
        let caps = PathBuf::from("caps.toml");
        let value = Value::data(Term::Map(BTreeMap::new()));
        let cmds = vec![
            (
                PkgCmd::Add {
                    spec: "dep@refs/heads/main".to_string(),
                    lock: PathBuf::from("genesis.lock"),
                    update_policy: "manual".to_string(),
                    registry: None,
                    strategy: None,
                    tag_policy: None,
                },
                "genesis/pkg-add-report-v0.1",
            ),
            (
                PkgCmd::Remove {
                    name: "dep".to_string(),
                    lock: PathBuf::from("genesis.lock"),
                },
                "genesis/pkg-remove-report-v0.1",
            ),
            (
                PkgCmd::Run {
                    task: "check".to_string(),
                    workspace_file: PathBuf::from("genesis.workspace.toml"),
                },
                "genesis/pkg-run-report-v0.1",
            ),
            (
                PkgCmd::Build {
                    pkg: PathBuf::from("package.toml"),
                    target: "web".to_string(),
                    out_dir: PathBuf::from(".genesis/build"),
                },
                "genesis/pkg-build-report-v0.1",
            ),
            (
                PkgCmd::Install {
                    lock: PathBuf::from("genesis.lock"),
                    frozen: true,
                    strict: true,
                },
                "genesis/pkg-install-report-v0.1",
            ),
            (
                PkgCmd::Verify {
                    lock: PathBuf::from("genesis.lock"),
                    pkg: PathBuf::from("package.toml"),
                    strict_sound: false,
                },
                "genesis/pkg-verify-report-v0.1",
            ),
            (
                PkgCmd::Doctor {
                    lock: PathBuf::from("genesis.lock"),
                    pkg: PathBuf::from("package.toml"),
                    strict_sound: false,
                },
                "genesis/pkg-doctor-ai-report-v0.1",
            ),
            (
                PkgCmd::Env {
                    profile: "dev".to_string(),
                    runtime_backend: None,
                    lock: PathBuf::from("genesis.lock"),
                    workspace_file: PathBuf::from("genesis.workspace.toml"),
                    out_dir: PathBuf::from(".genesis/env"),
                    hydrate: false,
                },
                "genesis/pkg-env-report-v0.1",
            ),
            (
                PkgCmd::Bridge {
                    ecosystem: "crates".to_string(),
                    name: "serde".to_string(),
                    version: "1.0.0".to_string(),
                    source: "serde@1.0.0".to_string(),
                    source_hash: "a".repeat(64),
                    key_id: "mirror-key".to_string(),
                    public_key: "b".repeat(64),
                    lock: Some(PathBuf::from("genesis.lock")),
                    dep_name: Some("serde".to_string()),
                    registry: Some("default".to_string()),
                },
                "genesis/pkg-bridge-report-v0.1",
            ),
        ];

        for (cmd, expected_schema) in cmds {
            let report = build_pkg_ai_report(&cmd, &value, &caps).expect("expected report");
            assert_eq!(
                report.get("schema").and_then(|v| v.as_str()),
                Some(expected_schema)
            );
            let fix_options = report
                .get("fix_options")
                .and_then(|v| v.as_array())
                .expect("fix_options array");
            assert!(
                !fix_options.is_empty(),
                "expected deterministic fix_options for schema {expected_schema}"
            );
        }
    }
}
