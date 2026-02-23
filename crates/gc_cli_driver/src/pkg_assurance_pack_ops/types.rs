use std::path::Path;

use gc_coreform::Term;

pub(crate) struct AssurancePackArgs<'a> {
    pub pkg: &'a Path,
    pub assurance_profile: &'a str,
    pub commit: Option<&'a str>,
    pub snapshot: &'a str,
    pub policy: Option<&'a str>,
    pub trace_spec: &'a str,
    pub qualification_spec: &'a str,
    pub coverage_specs: &'a [String],
    pub object_equivalence_spec: Option<&'a str>,
    pub independence_attestations: &'a [String],
    pub independent_verifier_run_specs: &'a [String],
    pub out: &'a Path,
    pub bundle_dir: Option<&'a Path>,
    pub no_store: bool,
}

#[derive(Debug, Clone)]
pub(super) struct LoadedTerm {
    pub(super) term: Term,
    pub(super) hash: String,
    pub(super) canonical_src: String,
    pub(super) source: String,
}

#[derive(Debug, Clone)]
pub(super) struct CoverageExport {
    pub(super) loaded: LoadedTerm,
    pub(super) profile: String,
    pub(super) ok: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ObjectEquivalenceEvidence {
    pub(super) loaded: LoadedTerm,
    pub(super) source_artifact: String,
    pub(super) object_artifact: String,
    pub(super) method: String,
}

#[derive(Debug, Clone)]
pub(super) struct IndependentVerifierRun {
    pub(super) loaded: LoadedTerm,
    pub(super) run_id: String,
    pub(super) runner: String,
    pub(super) roles: Vec<String>,
}
