use std::collections::BTreeMap;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use gc_effects::ArtifactStore;
use gc_vcs::validate_hex_hash;

pub(crate) struct QualificationLineageContext<'a> {
    pub commit: Option<&'a str>,
    pub snapshot: &'a str,
    pub policy: Option<&'a str>,
    pub profile: &'a str,
    pub store_dir: &'a Path,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedQualificationTest {
    pub id: String,
    pub artifact: String,
    pub manifest_hash: String,
    pub run_id: String,
    pub runner: String,
    pub snapshot: String,
    pub policy: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedQualificationTestSpec {
    id: String,
    manifest_hash: String,
}

const MANIFEST_KIND: &str = "genesis/qualification-test-run-manifest-v0.1";

pub(crate) fn resolve_qualification_tests(
    specs: &[String],
    ctx: QualificationLineageContext<'_>,
) -> Result<Vec<ResolvedQualificationTest>, String> {
    let parsed_specs = parse_qualification_test_specs(specs)?;
    let store = ArtifactStore::open(ctx.store_dir)
        .map_err(|e| format!("open artifact store {}: {e}", ctx.store_dir.display()))?;

    let mut out = Vec::with_capacity(parsed_specs.len());
    for spec in parsed_specs {
        out.push(resolve_single_qualification_test_spec(&store, &spec, &ctx)?);
    }
    out.sort_by(|a, b| {
        a.id.cmp(&b.id)
            .then_with(|| a.artifact.cmp(&b.artifact))
            .then_with(|| a.manifest_hash.cmp(&b.manifest_hash))
    });
    out.dedup_by(|a, b| {
        a.id == b.id && a.artifact == b.artifact && a.manifest_hash == b.manifest_hash
    });
    Ok(out)
}

fn parse_qualification_test_specs(
    specs: &[String],
) -> Result<Vec<ParsedQualificationTestSpec>, String> {
    if specs.is_empty() {
        return Err(
            "at least one --test-artifact id=<run-manifest-hex64> is required for tool qualification"
                .to_string(),
        );
    }
    let mut out = Vec::with_capacity(specs.len());
    for raw in specs {
        let (id_raw, manifest_hash_raw) = raw.split_once('=').ok_or_else(|| {
            format!("invalid --test-artifact `{raw}`; expected id=<run-manifest-hex64>")
        })?;
        let id = id_raw.trim();
        if id.is_empty() {
            return Err(format!("invalid --test-artifact `{raw}`: empty id"));
        }
        let manifest_hash = manifest_hash_raw.trim();
        validate_hex_hash(manifest_hash)
            .map_err(|e| format!("invalid --test-artifact `{raw}` hash: {e}"))?;
        out.push(ParsedQualificationTestSpec {
            id: id.to_string(),
            manifest_hash: manifest_hash.to_string(),
        });
    }
    out.sort_by(|a, b| {
        a.id.cmp(&b.id)
            .then_with(|| a.manifest_hash.cmp(&b.manifest_hash))
    });
    out.dedup_by(|a, b| a.id == b.id && a.manifest_hash == b.manifest_hash);
    Ok(out)
}

fn resolve_single_qualification_test_spec(
    store: &ArtifactStore,
    spec: &ParsedQualificationTestSpec,
    ctx: &QualificationLineageContext<'_>,
) -> Result<ResolvedQualificationTest, String> {
    let manifest_term = load_store_term(store, &spec.manifest_hash, "qualification run manifest")?;
    let manifest = as_map(&manifest_term, "qualification run manifest")?;

    let manifest_kind = required_symbol_or_string(manifest, ":kind", "qualification run manifest")?;
    let manifest_kind_norm = normalize_symbol_like(&manifest_kind);
    if manifest_kind != MANIFEST_KIND
        && manifest_kind_norm != ":qualification-test-run-manifest-v0.1"
    {
        return Err(format!(
            "qualification run manifest {} has unsupported :kind `{manifest_kind}`",
            spec.manifest_hash
        ));
    }

    let manifest_test_id = required_string(manifest, ":test-id", "qualification run manifest")?;
    if manifest_test_id != spec.id {
        return Err(format!(
            "qualification run manifest {} :test-id mismatch: expected `{}`, got `{}`",
            spec.manifest_hash, spec.id, manifest_test_id
        ));
    }
    let manifest_profile = required_string(manifest, ":profile", "qualification run manifest")?;
    if manifest_profile != ctx.profile {
        return Err(format!(
            "qualification run manifest {} profile mismatch: expected `{}`, got `{}`",
            spec.manifest_hash, ctx.profile, manifest_profile
        ));
    }
    let run_id = required_string(manifest, ":run-id", "qualification run manifest")?;
    let runner = required_string(manifest, ":runner", "qualification run manifest")?;

    let release = required_map(manifest, ":release", "qualification run manifest")?;
    let manifest_commit =
        required_string_or_nil(release, ":commit", "qualification run manifest :release")?;
    if let Some(commit) = &manifest_commit {
        validate_hex_hash(commit).map_err(|e| {
            format!(
                "qualification run manifest {} :release/:commit: {e}",
                spec.manifest_hash
            )
        })?;
    }
    if let Some(expected_commit) = ctx.commit {
        match manifest_commit.as_deref() {
            Some(actual) if actual == expected_commit => {}
            Some(actual) => {
                return Err(format!(
                    "qualification run manifest {} commit mismatch: expected `{expected_commit}`, got `{actual}`",
                    spec.manifest_hash
                ));
            }
            None => {
                return Err(format!(
                    "qualification run manifest {} missing :release/:commit while gcpm qualify provided --commit",
                    spec.manifest_hash
                ));
            }
        }
    }

    let snapshot = required_string(release, ":snapshot", "qualification run manifest :release")?;
    validate_hex_hash(&snapshot).map_err(|e| {
        format!(
            "qualification run manifest {} :release/:snapshot: {e}",
            spec.manifest_hash
        )
    })?;
    if snapshot != ctx.snapshot {
        return Err(format!(
            "qualification run manifest {} snapshot mismatch: expected `{}`, got `{}`",
            spec.manifest_hash, ctx.snapshot, snapshot
        ));
    }

    let policy = required_string_or_nil(release, ":policy", "qualification run manifest :release")?;
    if let Some(policy_hash) = &policy {
        validate_hex_hash(policy_hash).map_err(|e| {
            format!(
                "qualification run manifest {} :release/:policy: {e}",
                spec.manifest_hash
            )
        })?;
    }
    if let Some(expected_policy) = ctx.policy {
        match policy.as_deref() {
            Some(actual) if actual == expected_policy => {}
            Some(actual) => {
                return Err(format!(
                    "qualification run manifest {} policy mismatch: expected `{expected_policy}`, got `{actual}`",
                    spec.manifest_hash
                ));
            }
            None => {
                return Err(format!(
                    "qualification run manifest {} missing :release/:policy while gcpm qualify provided --policy",
                    spec.manifest_hash
                ));
            }
        }
    }

    let artifact = required_string(manifest, ":artifact", "qualification run manifest")?;
    validate_hex_hash(&artifact).map_err(|e| {
        format!(
            "qualification run manifest {} :artifact: {e}",
            spec.manifest_hash
        )
    })?;
    let test_artifact_term =
        load_store_term(store, &artifact, "qualification test artifact payload")?;
    let test_artifact_map = as_map(&test_artifact_term, "qualification test artifact payload")?;
    let artifact_kind = required_symbol_or_string(
        test_artifact_map,
        ":kind",
        "qualification test artifact payload",
    )?;
    if artifact_kind.trim().is_empty() {
        return Err(format!(
            "qualification test artifact payload {} has empty :kind",
            artifact
        ));
    }
    let artifact_ok = required_bool(
        test_artifact_map,
        ":ok",
        "qualification test artifact payload",
    )?;
    if !artifact_ok {
        return Err(format!(
            "qualification test artifact payload {} must declare :ok true",
            artifact
        ));
    }

    let result = normalize_symbol_like(&required_symbol_or_string(
        manifest,
        ":result",
        "qualification run manifest",
    )?);
    if result != ":pass" {
        return Err(format!(
            "qualification run manifest {} :result must be :pass, got `{}`",
            spec.manifest_hash, result
        ));
    }

    Ok(ResolvedQualificationTest {
        id: spec.id.clone(),
        artifact,
        manifest_hash: spec.manifest_hash.clone(),
        run_id,
        runner,
        snapshot,
        policy,
    })
}

fn load_store_term(store: &ArtifactStore, hex: &str, what: &str) -> Result<Term, String> {
    let bytes = store
        .get_bytes(hex)
        .map_err(|e| format!("{what} `{hex}` unavailable in local store: {e}"))?;
    let src = std::str::from_utf8(&bytes)
        .map_err(|e| format!("{what} `{hex}` is not utf8 CoreForm term bytes: {e}"))?;
    let term = parse_term(src).map_err(|e| format!("{what} `{hex}` parse failure: {e}"))?;
    let canonical_src = print_term(&term) + "\n";
    let canonical_hash = blake3::hash(canonical_src.as_bytes()).to_hex().to_string();
    if canonical_hash != hex {
        return Err(format!(
            "{what} `{hex}` must be canonical CoreForm bytes (computed canonical hash {canonical_hash})"
        ));
    }
    Ok(term)
}

fn as_map<'a>(term: &'a Term, what: &str) -> Result<&'a BTreeMap<TermOrdKey, Term>, String> {
    match term {
        Term::Map(m) => Ok(m),
        _ => Err(format!("{what} must be map")),
    }
}

fn required_map<'a>(
    map: &'a BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<&'a BTreeMap<TermOrdKey, Term>, String> {
    let key_t = TermOrdKey(Term::symbol(key));
    match map.get(&key_t) {
        Some(Term::Map(m)) => Ok(m),
        Some(_) => Err(format!("{what} {key} must be map")),
        None => Err(format!("{what} missing {key}")),
    }
}

fn required_string(
    map: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    let key_t = TermOrdKey(Term::symbol(key));
    match map.get(&key_t) {
        Some(Term::Str(s)) => {
            if s.trim().is_empty() {
                Err(format!("{what} {key} cannot be empty"))
            } else {
                Ok(s.clone())
            }
        }
        Some(_) => Err(format!("{what} {key} must be string")),
        None => Err(format!("{what} missing {key}")),
    }
}

fn required_symbol_or_string(
    map: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    let key_t = TermOrdKey(Term::symbol(key));
    match map.get(&key_t) {
        Some(Term::Str(s)) | Some(Term::Symbol(s)) => {
            if s.trim().is_empty() {
                Err(format!("{what} {key} cannot be empty"))
            } else {
                Ok(s.clone())
            }
        }
        Some(_) => Err(format!("{what} {key} must be symbol|string")),
        None => Err(format!("{what} missing {key}")),
    }
}

fn required_bool(map: &BTreeMap<TermOrdKey, Term>, key: &str, what: &str) -> Result<bool, String> {
    let key_t = TermOrdKey(Term::symbol(key));
    match map.get(&key_t) {
        Some(Term::Bool(v)) => Ok(*v),
        Some(_) => Err(format!("{what} {key} must be bool")),
        None => Err(format!("{what} missing {key}")),
    }
}

fn required_string_or_nil(
    map: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<Option<String>, String> {
    let key_t = TermOrdKey(Term::symbol(key));
    match map.get(&key_t) {
        Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => {
            if s.trim().is_empty() {
                Err(format!("{what} {key} cannot be empty"))
            } else {
                Ok(Some(s.clone()))
            }
        }
        Some(_) => Err(format!("{what} {key} must be string|nil")),
        None => Err(format!("{what} missing {key}")),
    }
}

fn normalize_symbol_like(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with(':') {
        trimmed.to_string()
    } else {
        format!(":{trimmed}")
    }
}
