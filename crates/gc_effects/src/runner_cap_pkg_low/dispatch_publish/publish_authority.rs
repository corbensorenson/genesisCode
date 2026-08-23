use super::*;
use crate::pkg_lock_read_authority::{
    PkgPublishDecision, PkgPublishInspection, PkgPublishObject, PkgPublishPreparation,
};

const MAX_PUBLISH_OBJECT_BYTES: usize = 4 * 1024 * 1024;
const MAX_PUBLISH_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_PUBLISH_OBJECTS: usize = 4096;

#[expect(
    clippy::too_many_arguments,
    reason = "the publish boundary keeps authority, transport, policy, and sealing explicit"
)]
pub(super) fn handle_publish(
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    mut refs_authority: Option<&mut RefsAuthority>,
    authority: Option<&mut PkgLockReadAuthority>,
    budget: &mut ArtifactBudgetState,
    bridge_runtime: &mut HostBridgeRuntime,
    error_tok: SealId,
    op: &str,
) -> Result<Value, EffectsError> {
    macro_rules! payload_try {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(message) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        message,
                        Some(op),
                    ));
                }
            }
        };
    }

    let remote = payload_try!(payload_pkg_publish_remote(payload));
    let refname = payload_try!(payload_pkg_publish_ref(payload));
    let policy_hash = payload_try!(payload_pkg_publish_policy(payload));
    let expected_old = payload_try!(payload_pkg_publish_expected_old(payload));
    let depth = payload_try!(payload_pkg_publish_depth(payload));
    let commit_override = payload_try!(payload_pkg_publish_commit(payload));

    // Authority availability is established before local ref lookup or artifact reads.
    let authority = authority.ok_or_else(|| {
        EffectsError::Log(
            "core/pkg-low::publish requires the artifact-loaded GenesisCode publish authority"
                .to_string(),
        )
    })?;
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/pkg-low::publish".to_string())
    })?;
    let refs = refs.ok_or_else(|| {
        EffectsError::Log("missing refs db for core/pkg-low::publish".to_string())
    })?;

    let commit_hash = match commit_override {
        Some(hash) => hash,
        None => match RefsAuthority::consumer_get(refs_authority.as_deref_mut(), refs, &refname) {
            Ok(Some(hash)) => hash,
            Ok(None) => {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/ref-not-found",
                    format!("local ref is unset: {refname}"),
                    Some(op),
                ));
            }
            Err(error) => {
                return Ok(mk_error(
                    error_tok,
                    "core/refs/io-error",
                    error.to_string(),
                    Some(op),
                ));
            }
        },
    };
    for (name, hash) in [("commit", &commit_hash), ("policy", &policy_hash)] {
        if !lowercase_hash(hash) {
            return Ok(mk_error(
                error_tok,
                "core/pkg/bad-payload",
                format!("{name} must be lowercase 64-hex"),
                Some(op),
            ));
        }
    }

    let mut observed_bytes = 0usize;
    let policy_object = match load_object(store, &policy_hash, &mut observed_bytes) {
        Ok(object) => object,
        Err(error) => return Ok(error.seal(error_tok, op)),
    };
    let commit_object = match load_object(store, &commit_hash, &mut observed_bytes) {
        Ok(object) => object,
        Err(error) => return Ok(error.seal(error_tok, op)),
    };
    let facts = publish_facts(
        &remote,
        &refname,
        &policy_object,
        &commit_object,
        expected_old.as_deref(),
        depth,
    );

    let (attestation_hashes, evidence_hashes, inspect_hash) =
        match authority.inspect_publish(&facts)? {
            PkgPublishInspection::Accept {
                attestation_hashes,
                evidence_hashes,
                inspect_hash,
            } => (attestation_hashes, evidence_hashes, inspect_hash),
            PkgPublishInspection::Error { code, message } => {
                return Ok(mk_error(error_tok, &code, message, Some(op)));
            }
        };
    let requested_count = attestation_hashes
        .len()
        .saturating_add(evidence_hashes.len());
    if requested_count > MAX_PUBLISH_OBJECTS {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "publish requested objects",
            requested_count,
            MAX_PUBLISH_OBJECTS,
        ));
    }
    let attestations = match load_objects(store, &attestation_hashes, &mut observed_bytes) {
        Ok(objects) => objects,
        Err(error) => return Ok(error.seal(error_tok, op)),
    };
    let evidence = match load_objects(store, &evidence_hashes, &mut observed_bytes) {
        Ok(objects) => objects,
        Err(error) => return Ok(error.seal(error_tok, op)),
    };

    let (crypto_facts, prepare_hash) =
        match authority.prepare_publish(&facts, &inspect_hash, &evidence, &attestations)? {
            PkgPublishPreparation::Accept {
                crypto_facts,
                prepare_hash,
            } => (crypto_facts, prepare_hash),
            PkgPublishPreparation::Error { code, message } => {
                return Ok(mk_error(error_tok, &code, message, Some(op)));
            }
        };
    let decision = authority.finalize_publish(
        &facts,
        &inspect_hash,
        &prepare_hash,
        &evidence,
        &attestations,
        crypto_facts,
    )?;
    let (commit, provenance, published_ref, sync) = match decision {
        PkgPublishDecision::Accept {
            commit,
            provenance,
            refname,
            sync,
        } => (commit, provenance, refname, sync),
        PkgPublishDecision::Error { code, message } => {
            return Ok(mk_error(error_tok, &code, message, Some(op)));
        }
    };

    let sync_pol = pol.or_else(|| policy.op_policy("core/sync::push"));
    let sync_out = call_capability_with_runtime(
        "core/sync::push",
        &sync,
        sync_pol,
        policy,
        Some(store),
        Some(refs),
        refs_authority.as_deref_mut(),
        None,
        None,
        None,
        None,
        None,
        budget,
        None,
        bridge_runtime,
        error_tok,
    )?;
    append_authority_result(sync_out, commit, published_ref, provenance)
}

fn publish_facts(
    remote: &str,
    refname: &str,
    policy: &PkgPublishObject,
    commit: &PkgPublishObject,
    expected_old: Option<&str>,
    depth: u64,
) -> Term {
    term_map([
        (":commit", commit.term.clone()),
        (":commit-h", Term::Str(commit.hash.clone())),
        (":depth", Term::Int(depth.into())),
        (
            ":expected-old",
            expected_old
                .map(|value| Term::Str(value.to_string()))
                .unwrap_or(Term::Nil),
        ),
        (":policy", policy.term.clone()),
        (":policy-h", Term::Str(policy.hash.clone())),
        (":ref", Term::Str(refname.to_string())),
        (":remote", Term::Str(remote.to_string())),
    ])
}

fn load_objects(
    store: &ArtifactStore,
    hashes: &[String],
    observed_bytes: &mut usize,
) -> Result<Vec<PkgPublishObject>, PublishLoadError> {
    hashes
        .iter()
        .map(|hash| load_object(store, hash, observed_bytes))
        .collect()
}

fn load_object(
    store: &ArtifactStore,
    hash: &str,
    observed_bytes: &mut usize,
) -> Result<PkgPublishObject, PublishLoadError> {
    if !lowercase_hash(hash) {
        return Err(PublishLoadError::BadHash(hash.to_string()));
    }
    if !store.path_for(hash).is_file() {
        return Err(PublishLoadError::Missing(hash.to_string()));
    }
    let bytes = store
        .get_bytes_limited(hash, MAX_PUBLISH_OBJECT_BYTES)
        .map_err(|error| PublishLoadError::Read(hash.to_string(), error.to_string()))?;
    *observed_bytes = observed_bytes.saturating_add(bytes.len());
    if *observed_bytes > MAX_PUBLISH_TOTAL_BYTES {
        return Err(PublishLoadError::TotalBytes(*observed_bytes));
    }
    let source = std::str::from_utf8(&bytes)
        .map_err(|_| PublishLoadError::Term(hash.to_string(), "not UTF-8".to_string()))?;
    let term = gc_coreform::parse_term(source)
        .map_err(|error| PublishLoadError::Term(hash.to_string(), error.to_string()))?;
    Ok(PkgPublishObject {
        hash: hash.to_string(),
        bytes,
        term,
    })
}

fn append_authority_result(
    sync_out: Value,
    commit: String,
    refname: String,
    provenance: Term,
) -> Result<Value, EffectsError> {
    let Value::Data(term) = sync_out else {
        return Ok(sync_out);
    };
    let Term::Map(mut fields) = term.as_ref().clone() else {
        return Err(EffectsError::Log(
            "core/sync::push returned non-map success to package publish".to_string(),
        ));
    };
    for (name, value) in [
        (":commit", Term::Str(commit)),
        (":provenance", provenance),
        (":ref", Term::Str(refname)),
    ] {
        let key = TermOrdKey(Term::symbol(name));
        if let Some(existing) = fields.get(&key)
            && existing != &value
        {
            return Err(EffectsError::Log(format!(
                "core/sync::push result contradicted authority field {name}"
            )));
        }
        fields.insert(key, value);
    }
    Ok(Value::data(Term::Map(fields)))
}

fn lowercase_hash(value: &str) -> bool {
    gc_vcs::validate_hex_hash(value).is_ok()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn term_map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

enum PublishLoadError {
    BadHash(String),
    Missing(String),
    Read(String, String),
    Term(String, String),
    TotalBytes(usize),
}

impl PublishLoadError {
    fn seal(self, error_tok: SealId, op: &str) -> Value {
        match self {
            Self::BadHash(hash) => mk_error(
                error_tok,
                "core/pkg/bad-authority-request",
                format!("authority requested malformed artifact hash: {hash}"),
                Some(op),
            ),
            Self::Missing(hash) => mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {hash}"),
                Some(op),
            ),
            Self::Read(hash, message) => mk_error(
                error_tok,
                "core/store/io-error",
                format!("cannot read artifact {hash}: {message}"),
                Some(op),
            ),
            Self::Term(hash, message) => mk_error(
                error_tok,
                "core/store/bad-term",
                format!("artifact {hash} is not a CoreForm term: {message}"),
                Some(op),
            ),
            Self::TotalBytes(observed) => mk_resource_limit_error(
                error_tok,
                op,
                "publish artifact bytes",
                observed,
                MAX_PUBLISH_TOTAL_BYTES,
            ),
        }
    }
}
