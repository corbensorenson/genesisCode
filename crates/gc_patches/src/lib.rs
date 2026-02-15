use std::collections::BTreeMap;
use std::path::Path;

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, parse_module, parse_term, print_module, print_term,
};
use gc_kernel::{MemLimits, StepLimit};
use gc_obligations::{
    EvidenceStore, ObligationError, PackageTestResult, pack, test_package_with_step_limit,
};
use gc_pkg::PackageManifest;
use num_traits::ToPrimitive;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("patch parse error: {0}")]
    Parse(String),

    #[error("patch validation error: {0}")]
    Validate(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("obligations error: {0}")]
    Obligations(#[from] ObligationError),
}

#[derive(Debug, Clone)]
pub struct PatchApplyResult {
    pub ok: bool,
    pub patch_artifact: String,
    pub report_artifact: String,
    pub acceptance_artifact: Option<String>,
    pub package_artifact: Option<String>,
}

#[derive(Debug, Clone)]
struct Patch {
    version: u64,
    intent: String,
    provenance: Term,
    ops: Vec<PatchOp>,
}

#[derive(Debug, Clone)]
enum PatchOp {
    ReplaceNode {
        module_path: String,
        path: Vec<PathStep>,
        new_term: Term,
    },
    AddModule {
        module_path: String,
        content: ModuleContent,
    },
    RemoveModule {
        module_path: String,
    },
    UpdateManifest {
        set: Option<Term>,
        obligations_add: Vec<String>,
        obligations_remove: Vec<String>,
        tests_add: Vec<String>,
        tests_remove: Vec<String>,
        caps_policy: Option<String>,
    },
}

#[derive(Debug, Clone)]
enum ModuleContent {
    Source(String),
    Forms(Vec<Term>),
}

#[derive(Debug, Clone)]
enum PathStep {
    Form(usize),
    PairCar,
    PairCdr,
    Vec(usize),
    Map(Term),
}

pub fn apply_patch(
    patch_path: &Path,
    pkg_toml: &Path,
    caps_override: Option<&Path>,
) -> Result<PatchApplyResult, PatchError> {
    apply_patch_with_step_limit(
        patch_path,
        pkg_toml,
        caps_override,
        StepLimit::Default,
        MemLimits::default(),
    )
}

pub fn apply_patch_with_step_limit(
    patch_path: &Path,
    pkg_toml: &Path,
    caps_override: Option<&Path>,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<PatchApplyResult, PatchError> {
    let patch_src = std::fs::read_to_string(patch_path)?;
    let patch_term = parse_term(&patch_src).map_err(|e| PatchError::Parse(e.to_string()))?;
    let patch = Patch::from_term(&patch_term)?;
    if patch.version != 1 {
        return Err(PatchError::Validate(format!(
            "unsupported patch :version {}",
            patch.version
        )));
    }

    let (_manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| PatchError::Validate(format!("{e}")))?;
    let store = EvidenceStore::open(&pkg_dir)?;

    // Store the patch artifact itself (as canonical CoreForm bytes).
    let patch_artifact = store.put_term(&patch_term)?;

    // Apply ops.
    for op in &patch.ops {
        apply_one_op(&pkg_dir, pkg_toml, op)?;
    }

    // Re-pack to compute updated package artifact record and module hashes.
    let package_artifact = Some(pack(pkg_toml)?);

    // Re-run obligations using updated manifest.
    let acceptance = Some(test_package_with_step_limit(
        pkg_toml,
        caps_override,
        step_limit,
        mem_limits,
    )?);

    let ok = acceptance.as_ref().is_some_and(|r| r.ok);

    let report = report_term(&patch, ok, &package_artifact, acceptance.as_ref());
    let report_artifact = store.put_term(&report)?;

    Ok(PatchApplyResult {
        ok,
        patch_artifact,
        report_artifact,
        acceptance_artifact: acceptance.as_ref().map(|r| r.acceptance_artifact.clone()),
        package_artifact,
    })
}

/// Validate a patch artifact term without performing any I/O.
pub fn validate_patch_term(t: &Term) -> Result<(), PatchError> {
    let _ = Patch::from_term(t)?;
    Ok(())
}

fn report_term(
    patch: &Patch,
    ok: bool,
    package_artifact: &Option<String>,
    acceptance: Option<&PackageTestResult>,
) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("genesis/patch-apply-v0.2".to_string()),
    );
    m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
    m.insert(
        TermOrdKey(Term::symbol(":intent")),
        Term::Str(patch.intent.clone()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":provenance")),
        patch.provenance.clone(),
    );
    m.insert(
        TermOrdKey(Term::symbol(":ops-count")),
        Term::Int((patch.ops.len() as i64).into()),
    );
    if let Some(p) = package_artifact {
        m.insert(
            TermOrdKey(Term::symbol(":package-artifact")),
            Term::Str(p.clone()),
        );
    }
    if let Some(a) = acceptance {
        m.insert(
            TermOrdKey(Term::symbol(":acceptance-artifact")),
            Term::Str(a.acceptance_artifact.clone()),
        );
    }
    Term::Map(m)
}

fn apply_one_op(pkg_dir: &Path, pkg_toml: &Path, op: &PatchOp) -> Result<(), PatchError> {
    match op {
        PatchOp::ReplaceNode {
            module_path,
            path,
            new_term,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_module(&src).map_err(|e| PatchError::Parse(e.to_string()))?;
            let mut forms =
                canonicalize_module(forms).map_err(|e| PatchError::Validate(e.to_string()))?;

            apply_replace(&mut forms, path, new_term.clone())?;
            let forms =
                canonicalize_module(forms).map_err(|e| PatchError::Validate(e.to_string()))?;
            let out = print_module(&forms);
            std::fs::write(&abs, out)?;
            Ok(())
        }
        PatchOp::AddModule {
            module_path,
            content,
        } => {
            let abs = pkg_dir.join(module_path);
            if abs.exists() {
                return Err(PatchError::Validate(format!(
                    "add-module target already exists: {}",
                    abs.display()
                )));
            }
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let forms = match content {
                ModuleContent::Source(s) => {
                    let forms = parse_module(s).map_err(|e| PatchError::Parse(e.to_string()))?;
                    canonicalize_module(forms).map_err(|e| PatchError::Validate(e.to_string()))?
                }
                ModuleContent::Forms(fs) => canonicalize_module(fs.clone())
                    .map_err(|e| PatchError::Validate(e.to_string()))?,
            };
            let out = print_module(&forms);
            std::fs::write(&abs, out)?;

            // Update manifest modules list by appending; pack will pin hashes.
            let mut s = std::fs::read_to_string(pkg_toml)?;
            let mut v: toml::Value =
                toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
            let mods = v
                .get_mut("modules")
                .and_then(|x| x.as_array_mut())
                .ok_or_else(|| {
                    PatchError::Validate("manifest missing modules array".to_string())
                })?;
            mods.push(toml::Value::Table(
                [
                    ("path".to_string(), toml::Value::String(module_path.clone())),
                    ("hash".to_string(), toml::Value::String("".to_string())),
                ]
                .into_iter()
                .collect(),
            ));
            s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
            std::fs::write(pkg_toml, s)?;
            Ok(())
        }
        PatchOp::RemoveModule { module_path } => {
            let abs = pkg_dir.join(module_path);
            if abs.exists() {
                std::fs::remove_file(&abs)?;
            }
            // Remove from manifest modules array.
            let mut s = std::fs::read_to_string(pkg_toml)?;
            let mut v: toml::Value =
                toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
            let mods = v
                .get_mut("modules")
                .and_then(|x| x.as_array_mut())
                .ok_or_else(|| {
                    PatchError::Validate("manifest missing modules array".to_string())
                })?;
            mods.retain(|m| m.get("path").and_then(|p| p.as_str()) != Some(module_path.as_str()));
            s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
            std::fs::write(pkg_toml, s)?;
            Ok(())
        }
        PatchOp::UpdateManifest {
            set,
            obligations_add,
            obligations_remove,
            tests_add,
            tests_remove,
            caps_policy,
        } => {
            let mut s = std::fs::read_to_string(pkg_toml)?;
            let mut v: toml::Value =
                toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
            if let Some(set) = set {
                apply_manifest_set(&mut v, set)?;
            }
            if !obligations_add.is_empty() || !obligations_remove.is_empty() {
                patch_string_vec_field(&mut v, "obligations", obligations_add, obligations_remove)?;
            }
            if !tests_add.is_empty() || !tests_remove.is_empty() {
                patch_string_vec_field(&mut v, "tests", tests_add, tests_remove)?;
            }
            if let Some(p) = caps_policy {
                v.as_table_mut()
                    .ok_or_else(|| PatchError::Validate("manifest must be a table".to_string()))?
                    .insert("caps_policy".to_string(), toml::Value::String(p.clone()));
            }
            s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
            std::fs::write(pkg_toml, s)?;
            Ok(())
        }
    }
}

fn patch_string_vec_field(
    v: &mut toml::Value,
    field: &str,
    add: &[String],
    remove: &[String],
) -> Result<(), PatchError> {
    let tbl = v
        .as_table_mut()
        .ok_or_else(|| PatchError::Validate("manifest must be a table".to_string()))?;
    let arr = tbl
        .entry(field.to_string())
        .or_insert_with(|| toml::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PatchError::Validate(format!("manifest field {field} must be array")))?;
    let mut set: std::collections::BTreeSet<String> = arr
        .iter()
        .filter_map(|x| x.as_str().map(|s| s.to_string()))
        .collect();
    for r in remove {
        set.remove(r);
    }
    for a in add {
        set.insert(a.clone());
    }
    *arr = set.into_iter().map(toml::Value::String).collect();
    Ok(())
}

fn apply_manifest_set(v: &mut toml::Value, set: &Term) -> Result<(), PatchError> {
    let Term::Map(m) = set else {
        return Err(PatchError::Validate(":set must be a map".to_string()));
    };
    let tbl = v
        .as_table_mut()
        .ok_or_else(|| PatchError::Validate("manifest must be a table".to_string()))?;
    for (k, vv) in m {
        let Term::Symbol(key) = &k.0 else {
            return Err(PatchError::Validate(
                "manifest :set keys must be symbols".to_string(),
            ));
        };
        // Strip leading ':' for convenience.
        let key = key.strip_prefix(':').unwrap_or(key.as_str()).to_string();
        tbl.insert(key, coreform_to_toml(vv)?);
    }
    Ok(())
}

fn coreform_to_toml(t: &Term) -> Result<toml::Value, PatchError> {
    use base64::Engine as _;
    match t {
        Term::Nil => Ok(toml::Value::String("nil".to_string())),
        Term::Bool(b) => Ok(toml::Value::Boolean(*b)),
        Term::Int(i) => {
            Ok(toml::Value::Integer(i.to_i64().ok_or_else(|| {
                PatchError::Validate("int out of range".to_string())
            })?))
        }
        Term::Str(s) => Ok(toml::Value::String(s.clone())),
        Term::Bytes(b) => Ok(toml::Value::String(
            base64::engine::general_purpose::STANDARD.encode(b),
        )),
        Term::Symbol(s) => Ok(toml::Value::String(s.clone())),
        Term::Vector(xs) => Ok(toml::Value::Array(
            xs.iter().map(coreform_to_toml).collect::<Result<_, _>>()?,
        )),
        Term::Map(m) => Ok(toml::Value::Table(
            m.iter()
                .map(|(k, v)| {
                    let kk = match &k.0 {
                        Term::Symbol(s) => s.clone(),
                        Term::Str(s) => s.clone(),
                        other => print_term(other),
                    };
                    Ok((kk, coreform_to_toml(v)?))
                })
                .collect::<Result<_, PatchError>>()?,
        )),
        Term::Pair(_, _) => Err(PatchError::Validate(
            "cannot convert list to TOML in :set".to_string(),
        )),
    }
}

fn apply_replace(forms: &mut [Term], path: &[PathStep], new_term: Term) -> Result<(), PatchError> {
    if path.is_empty() {
        return Err(PatchError::Validate("empty path".to_string()));
    }
    let mut cur: ReplaceTarget = ReplaceTarget::Module(forms);
    for (i, step) in path.iter().enumerate() {
        let is_last = i + 1 == path.len();
        cur = cur.step(step, is_last, new_term.clone())?;
    }
    Ok(())
}

enum ReplaceTarget<'a> {
    Module(&'a mut [Term]),
    Term(&'a mut Term),
}

impl<'a> ReplaceTarget<'a> {
    fn step(
        self,
        s: &PathStep,
        is_last: bool,
        new_term: Term,
    ) -> Result<ReplaceTarget<'a>, PatchError> {
        match self {
            ReplaceTarget::Module(forms) => match s {
                PathStep::Form(idx) => {
                    let t = forms.get_mut(*idx).ok_or_else(|| {
                        PatchError::Validate(format!("form index out of range: {idx}"))
                    })?;
                    if is_last {
                        *t = new_term;
                        Ok(ReplaceTarget::Term(t))
                    } else {
                        Ok(ReplaceTarget::Term(t))
                    }
                }
                _ => Err(PatchError::Validate(
                    "path must start with [:form i]".to_string(),
                )),
            },
            ReplaceTarget::Term(t) => {
                if is_last {
                    // Replace at this node with new_term; but only allowed if step is identity.
                }
                match s {
                    PathStep::PairCar => {
                        let Term::Pair(a, _) = t else {
                            return Err(PatchError::Validate("expected pair".to_string()));
                        };
                        if is_last {
                            **a = new_term;
                        }
                        Ok(ReplaceTarget::Term(a))
                    }
                    PathStep::PairCdr => {
                        let Term::Pair(_, d) = t else {
                            return Err(PatchError::Validate("expected pair".to_string()));
                        };
                        if is_last {
                            **d = new_term;
                        }
                        Ok(ReplaceTarget::Term(d))
                    }
                    PathStep::Vec(idx) => {
                        let Term::Vector(xs) = t else {
                            return Err(PatchError::Validate("expected vector".to_string()));
                        };
                        let elt = xs.get_mut(*idx).ok_or_else(|| {
                            PatchError::Validate(format!("vector index out of range: {idx}"))
                        })?;
                        if is_last {
                            *elt = new_term;
                        }
                        Ok(ReplaceTarget::Term(elt))
                    }
                    PathStep::Map(key) => {
                        let Term::Map(m) = t else {
                            return Err(PatchError::Validate("expected map".to_string()));
                        };
                        let elt = m.get_mut(&TermOrdKey(key.clone())).ok_or_else(|| {
                            PatchError::Validate(format!("missing map key {}", print_term(key)))
                        })?;
                        if is_last {
                            *elt = new_term;
                        }
                        Ok(ReplaceTarget::Term(elt))
                    }
                    PathStep::Form(_) => {
                        Err(PatchError::Validate("unexpected :form step".to_string()))
                    }
                }
            }
        }
    }
}

impl Patch {
    fn from_term(t: &Term) -> Result<Self, PatchError> {
        let Term::Map(m) = t else {
            return Err(PatchError::Validate("patch must be a map".to_string()));
        };
        let version = get_int(m, ":version")?.unwrap_or(1);
        let intent = get_str(m, ":intent")?.unwrap_or_else(|| "".to_string());
        let provenance = m
            .get(&TermOrdKey(Term::Symbol(":provenance".to_string())))
            .cloned()
            .unwrap_or(Term::Map(BTreeMap::new()));
        let ops_t = m
            .get(&TermOrdKey(Term::Symbol(":ops".to_string())))
            .ok_or_else(|| PatchError::Validate("missing :ops".to_string()))?;
        let Term::Vector(ops) = ops_t else {
            return Err(PatchError::Validate(":ops must be a vector".to_string()));
        };
        let mut parsed = Vec::new();
        for op in ops {
            parsed.push(parse_op(op)?);
        }
        Ok(Self {
            version,
            intent,
            provenance,
            ops: parsed,
        })
    }
}

fn parse_op(t: &Term) -> Result<PatchOp, PatchError> {
    let Term::Map(m) = t else {
        return Err(PatchError::Validate("op must be a map".to_string()));
    };
    let op = match m.get(&TermOrdKey(Term::Symbol(":op".to_string()))) {
        Some(Term::Symbol(s)) => s.as_str(),
        Some(x) => {
            return Err(PatchError::Validate(format!(
                ":op must be symbol, got {}",
                print_term(x)
            )));
        }
        None => return Err(PatchError::Validate("missing :op".to_string())),
    };
    match op {
        ":replace-node" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("replace-node missing :module-path".to_string())
            })?;
            let path_t = m
                .get(&TermOrdKey(Term::Symbol(":path".to_string())))
                .ok_or_else(|| PatchError::Validate("replace-node missing :path".to_string()))?;
            let path = parse_path(path_t)?;
            let new_term = m
                .get(&TermOrdKey(Term::Symbol(":new".to_string())))
                .ok_or_else(|| PatchError::Validate("replace-node missing :new".to_string()))?
                .clone();
            Ok(PatchOp::ReplaceNode {
                module_path,
                path,
                new_term,
            })
        }
        ":add-module" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("add-module missing :module-path".to_string())
            })?;
            let content_t = m
                .get(&TermOrdKey(Term::Symbol(":content".to_string())))
                .ok_or_else(|| PatchError::Validate("add-module missing :content".to_string()))?;
            let content = match content_t {
                Term::Str(s) => ModuleContent::Source(s.clone()),
                Term::Vector(xs) => ModuleContent::Forms(xs.clone()),
                _ => {
                    return Err(PatchError::Validate(
                        ":content must be string or vector".to_string(),
                    ));
                }
            };
            Ok(PatchOp::AddModule {
                module_path,
                content,
            })
        }
        ":remove-module" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("remove-module missing :module-path".to_string())
            })?;
            Ok(PatchOp::RemoveModule { module_path })
        }
        ":update-manifest" => {
            let set = m
                .get(&TermOrdKey(Term::Symbol(":set".to_string())))
                .cloned();
            let obligations_add = get_sym_vec(m, ":obligations-add")?;
            let obligations_remove = get_sym_vec(m, ":obligations-remove")?;
            let tests_add = get_sym_vec(m, ":tests-add")?;
            let tests_remove = get_sym_vec(m, ":tests-remove")?;
            let caps_policy = get_str(m, ":caps-policy")?;
            Ok(PatchOp::UpdateManifest {
                set,
                obligations_add,
                obligations_remove,
                tests_add,
                tests_remove,
                caps_policy,
            })
        }
        other => Err(PatchError::Validate(format!("unknown op {other}"))),
    }
}

fn parse_path(t: &Term) -> Result<Vec<PathStep>, PatchError> {
    let Term::Vector(steps) = t else {
        return Err(PatchError::Validate(":path must be a vector".to_string()));
    };
    let mut out = Vec::new();
    for s in steps {
        let Term::Vector(items) = s else {
            return Err(PatchError::Validate(
                "path step must be a vector".to_string(),
            ));
        };
        if items.is_empty() {
            return Err(PatchError::Validate("empty path step".to_string()));
        }
        let tag = match &items[0] {
            Term::Symbol(x) => x.as_str(),
            other => {
                return Err(PatchError::Validate(format!(
                    "bad path tag {}",
                    print_term(other)
                )));
            }
        };
        match tag {
            ":form" => {
                if items.len() != 2 {
                    return Err(PatchError::Validate(":form expects 1 arg".to_string()));
                }
                let idx = term_to_usize(&items[1])?;
                out.push(PathStep::Form(idx));
            }
            ":pair-car" => out.push(PathStep::PairCar),
            ":pair-cdr" => out.push(PathStep::PairCdr),
            ":vec" => {
                if items.len() != 2 {
                    return Err(PatchError::Validate(":vec expects 1 arg".to_string()));
                }
                out.push(PathStep::Vec(term_to_usize(&items[1])?));
            }
            ":map" => {
                if items.len() != 2 {
                    return Err(PatchError::Validate(":map expects 1 arg".to_string()));
                }
                out.push(PathStep::Map(items[1].clone()));
            }
            other => return Err(PatchError::Validate(format!("unknown path step {other}"))),
        }
    }
    Ok(out)
}

fn term_to_usize(t: &Term) -> Result<usize, PatchError> {
    match t {
        Term::Int(i) => i
            .to_usize()
            .ok_or_else(|| PatchError::Validate("index out of range".to_string())),
        _ => Err(PatchError::Validate("index must be int".to_string())),
    }
}

fn get_int(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Option<u64>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(None),
        Some(Term::Int(i)) => {
            Ok(Some(i.to_u64().ok_or_else(|| {
                PatchError::Validate(format!("{k} out of range"))
            })?))
        }
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be int, got {}",
            print_term(x)
        ))),
    }
}

fn get_str(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Option<String>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be string, got {}",
            print_term(x)
        ))),
    }
}

fn get_sym_vec(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Vec<String>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(Vec::new()),
        Some(Term::Vector(xs)) => Ok(xs
            .iter()
            .filter_map(|t| match t {
                Term::Symbol(s) => Some(s.clone()),
                _ => None,
            })
            .collect()),
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be vector, got {}",
            print_term(x)
        ))),
    }
}
