#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TranslationStage2Status {
    Complete,
    Failed,
    Unsupported,
}

impl TranslationStage2Status {
    fn symbol(self) -> &'static str {
        match self {
            Self::Complete => ":complete",
            Self::Failed => ":failed",
            Self::Unsupported => ":unsupported",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TranslationStage2Observation {
    pub(super) status: TranslationStage2Status,
    pub(super) module_hash: [u8; 32],
    pub(super) wasm_hash: Option<[u8; 32]>,
    pub(super) value_kind: Option<String>,
    pub(super) original_value_hash: Option<[u8; 32]>,
    pub(super) result_equal: Option<bool>,
    pub(super) wasm_value_hash: Option<[u8; 32]>,
    pub(super) wasm_bytes_len: Option<u64>,
    pub(super) mechanism_errors: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TranslationModuleObservation {
    pub(super) path: String,
    pub(super) original_hash: [u8; 32],
    pub(super) optimized_hash: [u8; 32],
    pub(super) egg_runs: u64,
    pub(super) egg_iterations: u64,
    pub(super) egg_eclasses: u64,
    pub(super) egg_enodes: u64,
    pub(super) rewrites: BTreeMap<String, u64>,
    pub(super) stage2: TranslationStage2Observation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TranslationOriginalTestObservation {
    pub(super) suite: String,
    pub(super) name: String,
    pub(super) sealed_error: bool,
    pub(super) expected_hash: Option<[u8; 32]>,
    pub(super) actual_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TranslationTestObservation {
    pub(super) suite: String,
    pub(super) name: String,
    pub(super) original_hash: [u8; 32],
    pub(super) optimized_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TranslationObservation {
    pub(super) modules: Vec<TranslationModuleObservation>,
    pub(super) original_tests: Vec<TranslationOriginalTestObservation>,
    pub(super) optimized_tests: Vec<TranslationTestObservation>,
}

fn translation_stage2_term(observation: &TranslationStage2Observation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":mechanism-errors")),
                Term::Vector(
                    observation
                        .mechanism_errors
                        .iter()
                        .cloned()
                        .map(Term::Str)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":module-h")),
                Term::Bytes(observation.module_hash.to_vec().into()),
            ),
            (
                TermOrdKey(Term::symbol(":orig-value-h")),
                optional_hash_term(observation.original_value_hash),
            ),
            (
                TermOrdKey(Term::symbol(":result-equal")),
                observation.result_equal.map(Term::Bool).unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":status")),
                Term::symbol(observation.status.symbol()),
            ),
            (
                TermOrdKey(Term::symbol(":value-kind")),
                observation
                    .value_kind
                    .as_ref()
                    .map(Term::symbol)
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":wasm-bytes")),
                observation
                    .wasm_bytes_len
                    .map(|value| Term::Int(BigInt::from(value)))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":wasm-h")),
                optional_hash_term(observation.wasm_hash),
            ),
            (
                TermOrdKey(Term::symbol(":wasm-value-h")),
                optional_hash_term(observation.wasm_value_hash),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn translation_module_term(observation: &TranslationModuleObservation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":egg-eclasses")),
                Term::Int(BigInt::from(observation.egg_eclasses)),
            ),
            (
                TermOrdKey(Term::symbol(":egg-enodes")),
                Term::Int(BigInt::from(observation.egg_enodes)),
            ),
            (
                TermOrdKey(Term::symbol(":egg-iterations")),
                Term::Int(BigInt::from(observation.egg_iterations)),
            ),
            (
                TermOrdKey(Term::symbol(":egg-rewrites")),
                Term::Vector(
                    observation
                        .rewrites
                        .iter()
                        .map(|(name, count)| {
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":n")),
                                        Term::Int(BigInt::from(*count)),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":name")),
                                        Term::Str(name.clone()),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            )
                        })
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":egg-runs")),
                Term::Int(BigInt::from(observation.egg_runs)),
            ),
            (
                TermOrdKey(Term::symbol(":optimized-h")),
                Term::Bytes(observation.optimized_hash.to_vec().into()),
            ),
            (
                TermOrdKey(Term::symbol(":original-h")),
                Term::Bytes(observation.original_hash.to_vec().into()),
            ),
            (
                TermOrdKey(Term::symbol(":path")),
                Term::Str(observation.path.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":stage2")),
                translation_stage2_term(&observation.stage2),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn translation_original_test_term(observation: &TranslationOriginalTestObservation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":actual-h")),
                Term::Bytes(observation.actual_hash.to_vec().into()),
            ),
            (
                TermOrdKey(Term::symbol(":expected-h")),
                optional_hash_term(observation.expected_hash),
            ),
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str(observation.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":sealed-error")),
                Term::Bool(observation.sealed_error),
            ),
            (
                TermOrdKey(Term::symbol(":suite")),
                Term::symbol(observation.suite.clone()),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn translation_test_term(observation: &TranslationTestObservation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str(observation.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":optimized-h")),
                Term::Bytes(observation.optimized_hash.to_vec().into()),
            ),
            (
                TermOrdKey(Term::symbol(":original-h")),
                Term::Bytes(observation.original_hash.to_vec().into()),
            ),
            (
                TermOrdKey(Term::symbol(":suite")),
                Term::symbol(observation.suite.clone()),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn translation_inputs(observation: &TranslationObservation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(
                    observation
                        .modules
                        .iter()
                        .map(translation_module_term)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":optimized-tests")),
                Term::Vector(
                    observation
                        .optimized_tests
                        .iter()
                        .map(translation_test_term)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":original-tests")),
                Term::Vector(
                    observation
                        .original_tests
                        .iter()
                        .map(translation_original_test_term)
                        .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn evaluate_translation_obligation_with_authority(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    observation: &TranslationObservation,
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let request = authority_request_term(
        ObligationAuthorityOperation::TranslationValidation,
        &manifest.name,
        translation_inputs(observation),
    );
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, frontend, limits)?;
    decode_translation_result(store, manifest, observation, request_hash, result)
}
