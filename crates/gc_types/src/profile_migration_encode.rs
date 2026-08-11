use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{Term, TermOrdKey};

use super::{
    EffectDelta, MIGRATION_PATCH_PROFILE_ID, MIGRATION_PATCH_VERSION, MIGRATION_PROFILE_ID,
    MigrationError, MigrationPlan, MigrationProvenance, MigrationStep, ModuleForTypecheck,
    ModuleMigrationDelta,
};
use crate::InferredEffects;

pub(super) fn plan_to_term(plan: &MigrationPlan) -> Term {
    map([
        (":expected-source-h", bytes32(plan.expected_source_identity)),
        (":intent", Term::Str(plan.intent.clone())),
        (":kind", Term::symbol(MIGRATION_PROFILE_ID)),
        (":migration-id", Term::Str(plan.migration_id.clone())),
        (":provenance", provenance_term(&plan.provenance)),
        (
            ":steps",
            Term::Vector(plan.steps.iter().map(step_to_term).collect()),
        ),
    ])
}

fn step_to_term(step: &MigrationStep) -> Term {
    match step {
        MigrationStep::RewriteSyntaxHead {
            module_path,
            from,
            to,
            expected_rewrites,
        } => map([
            (":expected-rewrites", int(*expected_rewrites)),
            (":from", Term::symbol(from)),
            (":kind", Term::symbol(":rewrite-syntax-head")),
            (":module-path", Term::Str(module_path.clone())),
            (":to", Term::symbol(to)),
        ]),
        MigrationStep::RenameApiSymbol {
            from,
            to,
            expected_rewrites,
        } => map([
            (":expected-rewrites", int(*expected_rewrites)),
            (":from", Term::symbol(from)),
            (":kind", Term::symbol(":rename-api-symbol")),
            (":to", Term::symbol(to)),
        ]),
        MigrationStep::ReplaceFormatField {
            module_path,
            field,
            expected,
            replacement,
        } => map([
            (
                ":expected",
                expected
                    .clone()
                    .map(present_term)
                    .unwrap_or_else(absent_term),
            ),
            (":field", Term::symbol(field)),
            (":kind", Term::symbol(":replace-format-field")),
            (":module-path", Term::Str(module_path.clone())),
            (
                ":replacement",
                replacement
                    .clone()
                    .map(present_term)
                    .unwrap_or_else(absent_term),
            ),
        ]),
    }
}

pub(super) fn patch_term(
    before: &[ModuleForTypecheck],
    after: &[ModuleForTypecheck],
    plan: &MigrationPlan,
    plan_identity: [u8; 32],
    source_identity: [u8; 32],
    target_identity: [u8; 32],
) -> Result<Term, MigrationError> {
    let mut ops = Vec::new();
    for (source, target) in before.iter().zip(after) {
        if source.path != target.path || source.forms.len() != target.forms.len() {
            return Err(MigrationError::Step(
                "migration operations must preserve module paths and form counts".to_string(),
            ));
        }
        for (index, (old, new)) in source.forms.iter().zip(&target.forms).enumerate() {
            if old != new {
                ops.push(map([
                    (":module-path", Term::Str(source.path.clone())),
                    (":new", new.clone()),
                    (":op", Term::symbol(":replace-node")),
                    (
                        ":path",
                        Term::Vector(vec![Term::Vector(vec![Term::symbol(":form"), int(index)])]),
                    ),
                ]));
            }
        }
    }
    if ops.is_empty() {
        return Err(MigrationError::Step(
            "migration produced no semantic patch operations".to_string(),
        ));
    }
    let migration_provenance = map([
        (":migration-id", Term::Str(plan.migration_id.clone())),
        (":migration-profile", Term::symbol(MIGRATION_PROFILE_ID)),
        (":patch-profile", Term::symbol(MIGRATION_PATCH_PROFILE_ID)),
        (":plan-h", bytes32(plan_identity)),
        (":request-provenance", provenance_term(&plan.provenance)),
        (":source-package-h", bytes32(source_identity)),
        (":target-package-h", bytes32(target_identity)),
    ]);
    Ok(map([
        (":intent", Term::Str(plan.intent.clone())),
        (":ops", Term::Vector(ops)),
        (":provenance", migration_provenance),
        (":version", Term::Int(MIGRATION_PATCH_VERSION.into())),
    ]))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn report_payload_term(
    plan: &MigrationPlan,
    plan_identity: [u8; 32],
    patch_identity: [u8; 32],
    source_identity: [u8; 32],
    target_identity: [u8; 32],
    effects: &EffectDelta,
    modules: &[ModuleMigrationDelta],
    target_typecheck: Term,
) -> Term {
    map([
        (":dry-run", Term::Bool(true)),
        (":effects", effect_delta_term(effects)),
        (":kind", Term::symbol(MIGRATION_PROFILE_ID)),
        (":migration-id", Term::Str(plan.migration_id.clone())),
        (
            ":modules",
            Term::Vector(modules.iter().map(module_delta_term).collect()),
        ),
        (":patch-h", bytes32(patch_identity)),
        (":plan-h", bytes32(plan_identity)),
        (":provenance", provenance_term(&plan.provenance)),
        (":source-package-h", bytes32(source_identity)),
        (":target-package-h", bytes32(target_identity)),
        (":target-typecheck", target_typecheck),
    ])
}

fn module_delta_term(delta: &ModuleMigrationDelta) -> Term {
    map([
        (":after-h", bytes32(delta.after_identity)),
        (":before-h", bytes32(delta.before_identity)),
        (
            ":changed-forms",
            Term::Vector(
                delta
                    .changed_form_indices
                    .iter()
                    .copied()
                    .map(int)
                    .collect(),
            ),
        ),
        (":effects", effect_delta_term(&delta.effects)),
        (":path", Term::Str(delta.path.clone())),
    ])
}

fn effect_delta_term(delta: &EffectDelta) -> Term {
    map([
        (":added", symbols(&delta.added)),
        (":after", effects_term(&delta.after)),
        (":before", effects_term(&delta.before)),
        (":removed", symbols(&delta.removed)),
    ])
}

fn effects_term(effects: &InferredEffects) -> Term {
    map([
        (":ops", symbols(&effects.ops)),
        (":unknown", Term::Bool(effects.unknown)),
    ])
}

fn provenance_term(provenance: &MigrationProvenance) -> Term {
    map([
        (
            ":parent-receipt-h",
            provenance.parent_receipt.map(bytes32).unwrap_or(Term::Nil),
        ),
        (":producer", Term::Str(provenance.producer.clone())),
        (
            ":source-artifact",
            Term::Str(provenance.source_artifact.clone()),
        ),
    ])
}

fn present_term(value: Term) -> Term {
    map([(":present", Term::Bool(true)), (":value", value)])
}

fn absent_term() -> Term {
    map([(":present", Term::Bool(false)), (":value", Term::Nil)])
}

fn symbols(values: &BTreeSet<String>) -> Term {
    Term::Vector(values.iter().cloned().map(Term::Symbol).collect())
}

fn bytes32(value: [u8; 32]) -> Term {
    Term::Bytes(value.to_vec().into())
}

fn int(value: usize) -> Term {
    Term::Int((value as i64).into())
}

fn map<const N: usize>(entries: [(&str, Term); N]) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value))
            .collect::<BTreeMap<_, _>>(),
    )
}
