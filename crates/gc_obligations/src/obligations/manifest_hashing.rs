fn pin_manifest_hashes(
    pkg_toml: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    deps: &[(String, String, String)],
) -> Result<(), ObligationError> {
    let mut doc: toml::Value = toml::from_str(&std::fs::read_to_string(pkg_toml)?)
        .map_err(|e| ObligationError::Manifest(format!("{}: {e}", pkg_toml.display())))?;
    let tbl = doc
        .as_table_mut()
        .ok_or_else(|| ObligationError::Manifest("package.toml must be a table".to_string()))?;

    // modules = [{ path, hash }, ...]
    if let Some(arr) = tbl.get_mut("modules").and_then(|v| v.as_array_mut()) {
        for (i, m) in modules.iter().enumerate() {
            if let Some(entry) = arr.get_mut(i).and_then(|v| v.as_table_mut()) {
                entry.insert("hash".to_string(), toml::Value::String(hex32(m.hash)));
            }
        }
    }

    // dependencies = [{ name, path, hash }, ...]
    if let Some(arr) = tbl.get_mut("dependencies").and_then(|v| v.as_array_mut()) {
        for dep in deps {
            let (name, _path, hash_hex) = dep;
            for item in arr.iter_mut() {
                let Some(t) = item.as_table_mut() else {
                    continue;
                };
                if t.get("name").and_then(|v| v.as_str()) == Some(name.as_str()) {
                    t.insert("hash".to_string(), toml::Value::String(hash_hex.clone()));
                }
            }
        }
    }

    let out = toml::to_string_pretty(&doc)
        .map_err(|e| ObligationError::Manifest(format!("cannot write manifest: {e}")))?;
    std::fs::write(pkg_toml, out)?;
    let _ = manifest;
    Ok(())
}

fn load_modules(
    pkg_dir: &Path,
    entries: &[ModuleEntry],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<Vec<LoadedModule>, ObligationError> {
    enforce_frontend_allowed(frontend, "module loading")?;
    let mut out = Vec::new();
    if frontend_is_rust(frontend) {
        for e in entries {
            let abs = pkg_dir.join(&e.path);
            let src = std::fs::read_to_string(&abs)?;
            let forms =
                parse_module(&src).map_err(|pe| ObligationError::Module(format!("{pe}")))?;
            let forms =
                canonicalize_module(forms).map_err(|e| ObligationError::Module(e.to_string()))?;
            let meta = extract_meta_static(&forms);
            let h = hash_module(&forms);
            out.push(LoadedModule {
                entry: e.clone(),
                abs_path: abs,
                forms,
                meta,
                hash: h,
            });
        }
    } else {
        let CoreformFrontend::Selfhost(cfg) = frontend else {
            return Err(ObligationError::Module(
                "invalid frontend dispatch in module loading".to_string(),
            ));
        };
        // Toolchain bootstrap is trusted and therefore uncharged.
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(limits.mem_limits);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            cfg.bootstrap_mode,
            cfg.artifact.as_deref(),
        )
        .map_err(|e| ObligationError::Module(format!("selfhost/init: {e}")))?;

        // Apply user/configured limits to parse+canonicalize work.
        ctx.steps = 0;
        ctx.step_limit = limits.step_limit.resolve();
        for e in entries {
            let abs = pkg_dir.join(&e.path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = selfhost_parse_canonicalize_module(&mut ctx, &env, &src)?;
            let meta = selfhost_extract_module_meta(&mut ctx, &env, &forms)?;
            let h = selfhost_hash_module_forms(&mut ctx, &env, &forms)?;
            out.push(LoadedModule {
                entry: e.clone(),
                abs_path: abs,
                forms,
                meta,
                hash: h,
            });
        }
    }
    Ok(out)
}

fn pack_dep_hashes(
    pkg_dir: &Path,
    deps: &[DepEntry],
    frontend: &CoreformFrontend,
) -> Result<Vec<(String, String, String)>, ObligationError> {
    let mut out = Vec::new();
    for d in deps {
        let dep_path = pkg_dir.join(&d.path);
        let dep_pkg = if dep_path.is_dir() {
            dep_path.join("package.toml")
        } else {
            dep_path
        };
        let hex = pack_with_frontend(&dep_pkg, frontend.clone())?;
        out.push((d.name.clone(), d.path.clone(), hex));
    }
    Ok(out)
}

fn check_dep_hashes(
    pkg_dir: &Path,
    deps: &[DepEntry],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<(), ObligationError> {
    let mut visited = std::collections::BTreeSet::new();
    for d in deps {
        let want = d.hash.as_deref().unwrap_or("");
        if want.is_empty() {
            return Err(ObligationError::Manifest(format!(
                "dependency {} is missing pinned hash; run `genesis pack` and commit the updated package.toml",
                d.name
            )));
        }
        let dep_path = pkg_dir.join(&d.path);
        let dep_pkg = if dep_path.is_dir() {
            dep_path.join("package.toml")
        } else {
            dep_path
        };
        let got = compute_package_artifact_hash(&dep_pkg, true, &mut visited, frontend, limits)?;
        if got != want {
            return Err(ObligationError::Manifest(format!(
                "dependency hash mismatch for {}: manifest has {}, computed {}",
                d.name, want, got
            )));
        }
    }
    Ok(())
}

fn compute_package_artifact_hash(
    pkg_toml: &Path,
    require_pinned: bool,
    visited: &mut std::collections::BTreeSet<PathBuf>,
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<String, ObligationError> {
    let canon = std::fs::canonicalize(pkg_toml)?;
    if !visited.insert(canon.clone()) {
        return Err(ObligationError::Manifest(format!(
            "dependency cycle detected at {}",
            canon.display()
        )));
    }

    let (manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| ObligationError::Manifest(e.to_string()))?;
    let modules = load_modules(&pkg_dir, &manifest.modules, frontend, limits)?;
    if require_pinned {
        for m in &modules {
            let want = m.entry.hash.as_deref().unwrap_or("");
            if want.is_empty() {
                return Err(ObligationError::Manifest(format!(
                    "{}: module {} missing pinned hash",
                    pkg_toml.display(),
                    m.entry.path
                )));
            }
            let got = hex32(m.hash);
            if want != got {
                return Err(ObligationError::Manifest(format!(
                    "{}: module hash mismatch for {}: manifest has {}, computed {}",
                    pkg_toml.display(),
                    m.entry.path,
                    want,
                    got
                )));
            }
        }
    }

    let mut deps_out = Vec::new();
    for d in &manifest.dependencies {
        let dep_path = pkg_dir.join(&d.path);
        let dep_pkg = if dep_path.is_dir() {
            dep_path.join("package.toml")
        } else {
            dep_path
        };
        let dep_hash =
            compute_package_artifact_hash(&dep_pkg, require_pinned, visited, frontend, limits)?;
        if require_pinned {
            let want = d.hash.as_deref().unwrap_or("");
            if want.is_empty() || want != dep_hash {
                return Err(ObligationError::Manifest(format!(
                    "{}: dependency hash mismatch for {}",
                    pkg_toml.display(),
                    d.name
                )));
            }
        }
        deps_out.push((d.name.clone(), d.path.clone(), dep_hash));
    }

    let record = package_record_term(&manifest, &modules, &deps_out);
    let bytes = gc_coreform::print_term(&record).into_bytes();
    let hex = blake3::hash(&bytes).to_hex().to_string();
    visited.remove(&canon);
    Ok(hex)
}

fn package_record_term(
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    deps: &[(String, String, String)],
) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("genesis/package-v0.2".to_string()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":name")),
        Term::Str(manifest.name.clone()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":version")),
        Term::Str(manifest.version.clone()),
    );

    let mods: Vec<Term> = modules
        .iter()
        .map(|x| {
            let mut mm = BTreeMap::new();
            mm.insert(
                TermOrdKey(Term::symbol(":path")),
                Term::Str(x.entry.path.clone()),
            );
            mm.insert(
                TermOrdKey(Term::symbol(":hash")),
                Term::Bytes(x.hash.to_vec().into()),
            );
            Term::Map(mm)
        })
        .collect();
    m.insert(TermOrdKey(Term::symbol(":modules")), Term::Vector(mods));

    let deps_t: Vec<Term> = deps
        .iter()
        .map(|(name, path, hash_hex)| {
            let mut dm = BTreeMap::new();
            dm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
            dm.insert(TermOrdKey(Term::symbol(":path")), Term::Str(path.clone()));
            dm.insert(
                TermOrdKey(Term::symbol(":hash")),
                Term::Str(hash_hex.clone()),
            );
            Term::Map(dm)
        })
        .collect();
    m.insert(
        TermOrdKey(Term::symbol(":dependencies")),
        Term::Vector(deps_t),
    );

    m.insert(
        TermOrdKey(Term::symbol(":obligations")),
        Term::Vector(
            manifest
                .obligations
                .iter()
                .cloned()
                .map(Term::Symbol)
                .collect(),
        ),
    );
    m.insert(
        TermOrdKey(Term::symbol(":tests")),
        Term::Vector(manifest.tests.iter().cloned().map(Term::Symbol).collect()),
    );

    Term::Map(m)
}

fn acceptance_term(manifest: &PackageManifest, ok: bool, obs: &[ObligationResult]) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("genesis/acceptance-v0.2".to_string()),
    );
    m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
    m.insert(
        TermOrdKey(Term::symbol(":package")),
        Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":name")),
                    Term::Str(manifest.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":version")),
                    Term::Str(manifest.version.clone()),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    );
    let entries: Vec<Term> = obs
        .iter()
        .map(|r| {
            let mut rm = BTreeMap::new();
            rm.insert(
                TermOrdKey(Term::symbol(":name")),
                Term::Symbol(r.name.clone()),
            );
            rm.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(r.ok));
            if let Some(a) = &r.artifact {
                rm.insert(TermOrdKey(Term::symbol(":artifact")), Term::Str(a.clone()));
            }
            if !r.errors.is_empty() {
                rm.insert(
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(r.errors.iter().cloned().map(Term::Str).collect()),
                );
            }
            Term::Map(rm)
        })
        .collect();
    m.insert(
        TermOrdKey(Term::symbol(":obligations")),
        Term::Vector(entries),
    );
    Term::Map(m)
}

