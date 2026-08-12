use super::*;
use crate::obligation_authority::{
    GfxApiDefinitionObservation, GfxApiObservation, evaluate_gfx_api_obligation_with_authority,
};

pub(super) fn obligation_gfx_api_stability(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let mut definitions = Vec::new();
    let mut exported_symbols = BTreeSet::new();
    for module in modules {
        for form in &module.forms {
            if let Some((symbol, expression)) = parse_def(form) {
                definitions.push(GfxApiDefinitionObservation {
                    symbol,
                    expression_hash: hash_term(&expression),
                });
            }
        }
        if let Some(meta) = extract_meta_static(&module.forms)
            && let Some(exports) = meta_exports(&meta)
        {
            exported_symbols.extend(exports);
        }
    }
    let observation = GfxApiObservation {
        definitions,
        exported_symbols: exported_symbols.into_iter().collect(),
        expected_symbols: manifest
            .gfx
            .api_exports
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        expected_surface_hash: manifest
            .gfx
            .api_surface_hash
            .as_ref()
            .map(|hash| hash.to_ascii_lowercase()),
    };
    evaluate_gfx_api_obligation_with_authority(store, manifest, &observation, frontend, limits)
}
