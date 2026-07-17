use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // crates/gc_cli -> repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".genesis"
            | ".quarto"
            | "target"
            | "node_modules"
            | ".tmp"
            | ".cargo-install-target"
            | "_site"
            | "vendor"
    )
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else {
                continue;
            };
            if ft.is_dir() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str())
                    && is_ignored_dir(name)
                {
                    continue;
                }
                stack.push(p);
            } else if ft.is_file() {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn read_utf8(p: &Path) -> Option<String> {
    // Skip very large files deterministically (we only care about source/control files).
    let md = fs::metadata(p).ok()?;
    if md.len() > 2_000_000 {
        return None;
    }
    let bytes = fs::read(p).ok()?;
    std::str::from_utf8(&bytes).ok().map(|s| s.to_string())
}

#[test]
fn bootstrap_archive_is_not_referenced_by_active_code() {
    let root = repo_root();
    let files = walk_files(&root);

    let this_file = PathBuf::from(file!())
        .canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().to_string());

    let archive_checker_path =
        ["scripts/check_", "old", "_", "bootstrap", "_retirement.sh"].join("");
    let allow_paths: BTreeSet<String> = [
        "docs/spec/BOOTSTRAP_OLD.md".to_string(),
        "docs/spec/WASI.md".to_string(),
        "ROADMAP.md".to_string(),
        "docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json".to_string(),
        "docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json".to_string(),
        "genesis.gates.json".to_string(),
        "policies/check_update_boundary_v0.1.json".to_string(),
        "policies/gates_v0.1.json".to_string(),
        "scripts/check_bootstrap_retirement_gate.sh".to_string(),
        "scripts/render_bootstrap_retirement_gate_report.sh".to_string(),
        archive_checker_path,
        "upgrade_plan.md".to_string(),
    ]
    .into_iter()
    .map(|p| root.join(p).to_string_lossy().to_string())
    .collect();

    // Avoid embedding the exact substrings in this test file so the scan doesn't self-trigger.
    let needles: Vec<String> = vec![
        format!("bootstrap{}old", "_"),
        format!("old{}bootstrap", "_"),
    ];

    let mut offenders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in files {
        let path_s = p.to_string_lossy().to_string();
        if this_file.as_ref().is_some_and(|tf| tf == &path_s) {
            continue;
        }
        let retained_campaign_evidence = p
            .strip_prefix(&root)
            .is_ok_and(|relative| relative.starts_with("benchmarks/genesisbench/v0.1/campaigns"));
        if retained_campaign_evidence {
            // Transcripts preserve untrusted model output and historical repository listings.
            continue;
        }
        let Some(src) = read_utf8(&p) else {
            continue;
        };
        for needle in &needles {
            if !src.contains(needle) {
                continue;
            }
            if allow_paths.contains(&path_s) {
                continue;
            }
            offenders
                .entry(needle.clone())
                .or_default()
                .push(path_s.clone());
        }
    }

    if offenders.is_empty() {
        return;
    }
    let mut msg = String::new();
    msg.push_str("bootstrap archive references must remain archived-only.\n");
    for (needle, mut paths) in offenders {
        paths.sort();
        paths.dedup();
        msg.push_str(&format!("\nneedle: `{needle}`\n"));
        for p in paths.into_iter().take(50) {
            msg.push_str(&format!("  - {p}\n"));
        }
    }
    panic!("{msg}");
}
