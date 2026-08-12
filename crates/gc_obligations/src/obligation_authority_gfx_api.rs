#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GfxApiDefinitionObservation {
    pub(super) symbol: String,
    pub(super) expression_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GfxApiObservation {
    pub(super) definitions: Vec<GfxApiDefinitionObservation>,
    pub(super) exported_symbols: Vec<String>,
    pub(super) expected_symbols: Vec<String>,
    pub(super) expected_surface_hash: Option<String>,
}

fn gfx_api_inputs(observation: &GfxApiObservation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":definitions")),
                Term::Vector(
                    observation
                        .definitions
                        .iter()
                        .map(|definition| {
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":expr-h")),
                                        Term::Bytes(definition.expression_hash.to_vec().into()),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":sym")),
                                        Term::symbol(definition.symbol.clone()),
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
                TermOrdKey(Term::symbol(":expected-exports")),
                Term::Vector(
                    observation
                        .expected_symbols
                        .iter()
                        .cloned()
                        .map(Term::symbol)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":expected-surface-h")),
                observation
                    .expected_surface_hash
                    .as_ref()
                    .cloned()
                    .map(Term::Str)
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":exports")),
                Term::Vector(
                    observation
                        .exported_symbols
                        .iter()
                        .cloned()
                        .map(Term::symbol)
                        .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn gfx_api_expected(
    manifest: &PackageManifest,
    observation: &GfxApiObservation,
) -> (bool, Vec<String>, Term) {
    let mut errors = Vec::new();
    let mut definitions = BTreeMap::new();
    for definition in &observation.definitions {
        if let Some(previous) = definitions.insert(
            definition.symbol.clone(),
            definition.expression_hash,
        ) && previous != definition.expression_hash
        {
            errors.push(format!(
                "symbol {} has conflicting definitions across modules",
                definition.symbol
            ));
        }
    }

    let exported = observation
        .exported_symbols
        .iter()
        .filter(|symbol| symbol.starts_with("core/gfx/"))
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected = observation
        .expected_symbols
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let tracked = if expected.is_empty() {
        exported.clone()
    } else {
        expected.clone()
    };

    let mut definition_entries = Vec::new();
    let mut missing_definitions = Vec::new();
    for symbol in &tracked {
        if let Some(hash) = definitions.get(symbol) {
            definition_entries.push(Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":expr-h")),
                        Term::Bytes(hash.to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":sym")),
                        Term::symbol(symbol.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            ));
        } else {
            missing_definitions.push(symbol.clone());
        }
    }
    let surface = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":defs")),
                Term::Vector(definition_entries),
            ),
            (
                TermOrdKey(Term::symbol(":exports")),
                Term::Vector(tracked.iter().cloned().map(Term::symbol).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/gfx-api-surface-v0.2".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let surface_hash = hex32(hash_term(&surface));
    if observation
        .expected_surface_hash
        .as_ref()
        .is_some_and(|hash| {
            hash.len() != 64
                || !hash
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
        })
    {
        errors.push("gfx.api_surface_hash must be 64 lowercase hex chars".to_string());
    }
    if !expected.is_empty() && !expected.is_subset(&exported) {
        errors.push("missing exported gfx symbols".to_string());
    }
    if !expected.is_empty() && !exported.is_subset(&expected) {
        errors.push("unexpected exported gfx symbols".to_string());
    }
    if tracked.is_empty() {
        errors.push("no tracked gfx API exports found".to_string());
    }
    if expected.is_empty() && observation.expected_surface_hash.is_none() {
        errors.push(
            "gfx api stability requires gfx.api_exports and/or gfx.api_surface_hash configuration"
                .to_string(),
        );
    }
    if observation
        .expected_surface_hash
        .as_ref()
        .is_some_and(|expected_hash| expected_hash != &surface_hash)
    {
        errors.push("gfx API surface hash mismatch".to_string());
    }
    if !missing_definitions.is_empty() {
        errors.push("tracked API symbol has no defining def form".to_string());
    }
    let ok = errors.is_empty();
    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":expected-surface-h")),
                observation
                    .expected_surface_hash
                    .as_ref()
                    .cloned()
                    .map(Term::Str)
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/gfx-api-stability-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":surface")), surface),
            (
                TermOrdKey(Term::symbol(":surface-h")),
                Term::Str(surface_hash),
            ),
        ]
        .into_iter()
        .collect(),
    );
    (ok, errors, report)
}

fn decode_gfx_api_result(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    observation: &GfxApiObservation,
    request_hash: [u8; 32],
    term: Term,
) -> Result<ObligationResult, ObligationError> {
    let map = exact_map(
        &term,
        "gfx API authority result",
        &[
            ":errors",
            ":kind",
            ":name",
            ":ok",
            ":operation",
            ":report",
            ":request-h",
            ":v",
        ],
    )?;
    if string_field(map, ":kind", "gfx API authority result")?
        != "genesis/obligation-authority-result-v0.2"
        || string_field(map, ":name", "gfx API authority result")?
            != "core/obligation::gfx-api-stability"
        || !matches!(map_field(map, ":operation"), Some(Term::Symbol(value)) if value == ":gfx-api-stability")
        || string_field(map, ":request-h", "gfx API authority result")? != hex32(request_hash)
        || !matches!(map_field(map, ":v"), Some(Term::Int(value)) if value == &2.into())
    {
        return Err(authority_error("gfx API authority result identity mismatch"));
    }
    let expected = gfx_api_expected(manifest, observation);
    let errors = string_vector(
        required_field(map, ":errors", "gfx API authority result")?,
        "gfx API authority result :errors",
    )?;
    let report = required_field(map, ":report", "gfx API authority result")?;
    if bool_field(map, ":ok", "gfx API authority result")? != expected.0
        || errors != expected.1
        || report != &expected.2
    {
        return Err(authority_error(
            "gfx API authority result contradicts definition observations",
        ));
    }
    let artifact = store.put_term(report)?;
    Ok(ObligationResult {
        name: "core/obligation::gfx-api-stability".to_string(),
        ok: expected.0,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn evaluate_gfx_api_obligation_with_authority(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    observation: &GfxApiObservation,
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let request = authority_request_term(
        ObligationAuthorityOperation::GfxApiStability,
        &manifest.name,
        gfx_api_inputs(observation),
    );
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, frontend, limits)?;
    decode_gfx_api_result(store, manifest, observation, request_hash, result)
}
