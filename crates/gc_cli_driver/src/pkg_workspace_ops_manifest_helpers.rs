use super::*;

pub(super) fn build_env_members_term(
    workspace_file: &Path,
    members: &[WorkspaceMember],
) -> Result<Term, String> {
    let workspace_dir = workspace_file.parent().unwrap_or_else(|| Path::new("."));
    let mut out = Vec::new();
    for member in members {
        let member_root = workspace_dir.join(&member.path);
        let pkg_file = member_root.join("package.toml");
        let (pkg_path, pkg_hash) = if pkg_file.is_file() {
            let bytes = std::fs::read(&pkg_file).map_err(|e| e.to_string())?;
            (
                Term::Str(pkg_file.display().to_string()),
                Term::Str(blake3::hash(&bytes).to_hex().to_string()),
            )
        } else {
            (Term::symbol(":none"), Term::symbol(":none"))
        };
        out.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":name")),
                    Term::Str(member.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(member.path.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":role")),
                    member
                        .role
                        .clone()
                        .map(Term::Str)
                        .unwrap_or_else(|| Term::symbol(":none")),
                ),
                (TermOrdKey(Term::symbol(":package-file")), pkg_path),
                (TermOrdKey(Term::symbol(":package-h")), pkg_hash),
            ]
            .into_iter()
            .collect(),
        ));
    }
    Ok(Term::Vector(out))
}

pub(super) fn build_env_deps_term(
    workspace_file: &Path,
    lock: &gc_pkg::GenesisLock,
) -> Result<Term, String> {
    let store_dir = super::workspace_store_dir(workspace_file);

    let mut reqs = Vec::new();
    for (name, req) in &lock.requirements {
        reqs.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":name")), Term::Str(name.clone())),
                (
                    TermOrdKey(Term::symbol(":selector")),
                    Term::Str(req.selector.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":update-policy")),
                    Term::Str(match req.update_policy {
                        UpdatePolicy::Manual => "manual".to_string(),
                        UpdatePolicy::Auto => "auto".to_string(),
                    }),
                ),
                (
                    TermOrdKey(Term::symbol(":strategy")),
                    Term::Str(req.strategy.as_str().to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":registry")),
                    req.registry
                        .clone()
                        .map(Term::Str)
                        .unwrap_or_else(|| Term::symbol(":none")),
                ),
                (
                    TermOrdKey(Term::symbol(":tag-policy")),
                    req.tag_policy
                        .clone()
                        .map(Term::Str)
                        .unwrap_or_else(|| Term::symbol(":none")),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let mut locked = Vec::new();
    for (name, entry) in &lock.locked {
        let snap_path = store_dir.join(&entry.snapshot);
        if !snap_path.is_file() {
            return Err(format!(
                "locked snapshot for dependency `{name}` is missing from local store: {}",
                snap_path.display()
            ));
        }
        if let Some(commit) = &entry.commit {
            let commit_path = store_dir.join(commit);
            if !commit_path.is_file() {
                return Err(format!(
                    "locked commit for dependency `{name}` is missing from local store: {}",
                    commit_path.display()
                ));
            }
        }

        locked.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":name")), Term::Str(name.clone())),
                (
                    TermOrdKey(Term::symbol(":commit")),
                    entry
                        .commit
                        .clone()
                        .map(Term::Str)
                        .unwrap_or_else(|| Term::symbol(":none")),
                ),
                (
                    TermOrdKey(Term::symbol(":snapshot")),
                    Term::Str(entry.snapshot.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":registry")),
                    entry
                        .registry
                        .clone()
                        .map(Term::Str)
                        .unwrap_or_else(|| Term::symbol(":none")),
                ),
                (
                    TermOrdKey(Term::symbol(":source-selector")),
                    Term::Str(entry.source_selector.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":resolved-ref")),
                    entry
                        .resolved_ref
                        .clone()
                        .map(Term::Str)
                        .unwrap_or_else(|| Term::symbol(":none")),
                ),
                (
                    TermOrdKey(Term::symbol(":exports-h")),
                    entry
                        .exports_hash
                        .clone()
                        .map(Term::Str)
                        .unwrap_or_else(|| Term::symbol(":none")),
                ),
                (
                    TermOrdKey(Term::symbol(":environment-fingerprint")),
                    entry
                        .environment_fingerprint
                        .clone()
                        .map(Term::Str)
                        .unwrap_or_else(|| Term::symbol(":none")),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    Ok(Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":store")),
                Term::Str(store_dir.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":requirements")),
                Term::Vector(reqs),
            ),
            (TermOrdKey(Term::symbol(":locked")), Term::Vector(locked)),
        ]
        .into_iter()
        .collect(),
    ))
}
