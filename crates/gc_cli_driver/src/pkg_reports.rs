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
        PkgCmd::Lock { lock, strict } => Some(build_lock_report(value, caps, lock, *strict)),
        PkgCmd::Update { lock } => Some(build_update_report(value, caps, lock)),
        PkgCmd::Publish {
            remote,
            refname,
            policy,
            expected_old,
            depth,
            commit,
        } => Some(build_publish_report(
            value,
            caps,
            remote,
            refname,
            policy,
            expected_old.as_deref(),
            *depth,
            commit.as_deref(),
        )),
        _ => None,
    }
}

fn build_lock_report(value: &Value, caps: &Path, lock: &Path, strict: bool) -> serde_json::Value {
    let lock_hash = map_get_str(value, ":lock-h");
    let locked_count = map_get_int(value, ":locked-count").unwrap_or(0);
    let changed = locked_count > 0;

    serde_json::json!({
        "schema": "genesis/pkg-lock-report-v0.1",
        "workflow": "lock",
        "changed": changed,
        "lock": lock.display().to_string(),
        "lock_hash": lock_hash,
        "locked_count": locked_count,
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

fn build_update_report(value: &Value, caps: &Path, lock: &Path) -> serde_json::Value {
    let lock_hash = map_get_str(value, ":lock-h");
    let updated_count = map_get_int(value, ":updated").unwrap_or(0);
    let changed = updated_count > 0;

    serde_json::json!({
        "schema": "genesis/pkg-update-report-v0.1",
        "workflow": "update",
        "changed": changed,
        "lock": lock.display().to_string(),
        "lock_hash": lock_hash,
        "updated_count": updated_count,
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

#[allow(clippy::too_many_arguments)]
fn build_publish_report(
    value: &Value,
    caps: &Path,
    remote: &str,
    refname: &str,
    policy: &str,
    expected_old: Option<&str>,
    depth: u64,
    commit: Option<&str>,
) -> serde_json::Value {
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
        let value = Value::Data(Term::Map(m));
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
}
