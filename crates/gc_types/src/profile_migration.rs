use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{HASH_PROFILE_ID, Term, TermOrdKey, canonicalize_module, hash_module, hash_term};
use thiserror::Error;
use unicode_normalization::is_nfc;

use crate::{
    InferredEffects, ModuleForTypecheck, ProfileOffer, infer_effects,
    typecheck_package_with_profile_offer,
};

#[path = "profile_migration_encode.rs"]
mod encode;
#[path = "profile_migration_rewrite.rs"]
mod rewrite;

pub const MIGRATION_PROFILE_ID: &str = "genesis/migration-profile-v0.1";
pub const MIGRATION_PATCH_PROFILE_ID: &str = "genesis/patch-profile/v0.2";
pub const MIGRATION_PATCH_VERSION: u64 = 1;
pub const MAX_MIGRATION_MODULES: usize = 65_536;
pub const MAX_MIGRATION_STEPS: usize = 65_536;
pub const MAX_MIGRATION_ID_BYTES: usize = 1_024;
pub const MAX_MIGRATION_INTENT_BYTES: usize = 16_384;
pub const MAX_MIGRATION_PATH_OR_SYMBOL_BYTES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationProvenance {
    pub producer: String,
    pub source_artifact: String,
    pub parent_receipt: Option<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationPlan {
    pub migration_id: String,
    pub intent: String,
    pub expected_source_identity: [u8; 32],
    pub provenance: MigrationProvenance,
    pub steps: Vec<MigrationStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStep {
    RewriteSyntaxHead {
        module_path: String,
        from: String,
        to: String,
        expected_rewrites: usize,
    },
    RenameApiSymbol {
        from: String,
        to: String,
        expected_rewrites: usize,
    },
    ReplaceFormatField {
        module_path: String,
        field: String,
        expected: Option<Term>,
        replacement: Option<Term>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDelta {
    pub before: InferredEffects,
    pub after: InferredEffects,
    pub added: BTreeSet<String>,
    pub removed: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleMigrationDelta {
    pub path: String,
    pub before_identity: [u8; 32],
    pub after_identity: [u8; 32],
    pub changed_form_indices: Vec<usize>,
    pub effects: EffectDelta,
}

#[derive(Debug, Clone)]
pub struct MigrationDryRun {
    pub plan_identity: [u8; 32],
    pub patch_identity: [u8; 32],
    pub receipt_identity: [u8; 32],
    pub source_package_identity: [u8; 32],
    pub target_package_identity: [u8; 32],
    pub effects: EffectDelta,
    pub module_deltas: Vec<ModuleMigrationDelta>,
    pub patch: Term,
    pub receipt: Term,
    pub migrated_modules: Vec<ModuleForTypecheck>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MigrationError {
    #[error("invalid migration input: {0}")]
    InvalidInput(String),
    #[error("stale migration source: expected {expected}, got {actual}")]
    StaleSource { expected: String, actual: String },
    #[error("migration step failed: {0}")]
    Step(String),
    #[error("migration target failed typecheck: {0}")]
    InvalidTarget(String),
}

pub fn migration_package_identity(
    modules: &[ModuleForTypecheck],
) -> Result<[u8; 32], MigrationError> {
    validate_source_modules(modules)?;
    Ok(hash_term(&package_identity_term(modules)))
}

pub fn dry_run_migration(
    modules: &[ModuleForTypecheck],
    plan: &MigrationPlan,
) -> Result<MigrationDryRun, MigrationError> {
    dry_run_migration_with_profile_offer(modules, plan, &ProfileOffer::core_host())
}

pub fn dry_run_migration_with_profile_offer(
    modules: &[ModuleForTypecheck],
    plan: &MigrationPlan,
    offer: &ProfileOffer,
) -> Result<MigrationDryRun, MigrationError> {
    validate_plan(plan)?;
    let source_package_identity = migration_package_identity(modules)?;
    if source_package_identity != plan.expected_source_identity {
        return Err(MigrationError::StaleSource {
            expected: hex32(plan.expected_source_identity),
            actual: hex32(source_package_identity),
        });
    }

    let plan_term = encode::plan_to_term(plan);
    let plan_identity = hash_term(&plan_term);
    let mut migrated_modules = modules.to_vec();
    for step in &plan.steps {
        apply_step(&mut migrated_modules, step)?;
    }
    refresh_metadata(&mut migrated_modules)?;
    let target_typecheck = typecheck_package_with_profile_offer(&migrated_modules, offer);
    if !target_typecheck.ok {
        return Err(MigrationError::InvalidTarget(summarize_errors(
            &target_typecheck.errors,
        )));
    }

    let target_package_identity = migration_package_identity(&migrated_modules)?;
    if target_package_identity == source_package_identity {
        return Err(MigrationError::Step(
            "migration produced no package identity change".to_string(),
        ));
    }
    let module_deltas = module_deltas(modules, &migrated_modules);
    let effects = effect_delta(package_effects(modules), package_effects(&migrated_modules));
    let patch = encode::patch_term(
        modules,
        &migrated_modules,
        plan,
        plan_identity,
        source_package_identity,
        target_package_identity,
    )?;
    let patch_identity = hash_term(&patch);
    let report_payload = encode::report_payload_term(
        plan,
        plan_identity,
        patch_identity,
        source_package_identity,
        target_package_identity,
        &effects,
        &module_deltas,
        target_typecheck.to_term(),
    );
    let receipt_identity = hash_term(&report_payload);
    let receipt = map([
        (":receipt-h", bytes32(receipt_identity)),
        (":report", report_payload),
    ]);

    Ok(MigrationDryRun {
        plan_identity,
        patch_identity,
        receipt_identity,
        source_package_identity,
        target_package_identity,
        effects,
        module_deltas,
        patch,
        receipt,
        migrated_modules,
    })
}

fn validate_source_modules(modules: &[ModuleForTypecheck]) -> Result<(), MigrationError> {
    if modules.is_empty() {
        return Err(MigrationError::InvalidInput(
            "package must contain at least one module".to_string(),
        ));
    }
    if modules.len() > MAX_MIGRATION_MODULES {
        return Err(MigrationError::InvalidInput(format!(
            "package has {} modules; maximum is {MAX_MIGRATION_MODULES}",
            modules.len()
        )));
    }
    let mut paths = BTreeSet::new();
    for module in modules {
        validate_module_path(&module.path)?;
        if !paths.insert(module.path.clone()) {
            return Err(MigrationError::InvalidInput(format!(
                "duplicate module path {}",
                module.path
            )));
        }
        let canonical = canonicalize_module(module.forms.clone()).map_err(|error| {
            MigrationError::InvalidInput(format!(
                "module {} cannot be canonicalized: {error:#}",
                module.path
            ))
        })?;
        if canonical != module.forms {
            return Err(MigrationError::InvalidInput(format!(
                "module {} is not canonical CoreForm",
                module.path
            )));
        }
        let derived =
            rewrite::module_metadata(&module.forms).map_err(MigrationError::InvalidInput)?;
        if derived != module.meta {
            return Err(MigrationError::InvalidInput(format!(
                "module {} metadata does not match its canonical forms",
                module.path
            )));
        }
    }
    Ok(())
}

fn validate_plan(plan: &MigrationPlan) -> Result<(), MigrationError> {
    validate_text_id(
        "migration id",
        &plan.migration_id,
        false,
        MAX_MIGRATION_ID_BYTES,
    )?;
    validate_text_id("intent", &plan.intent, true, MAX_MIGRATION_INTENT_BYTES)?;
    validate_text_id(
        "provenance producer",
        &plan.provenance.producer,
        false,
        MAX_MIGRATION_ID_BYTES,
    )?;
    validate_text_id(
        "provenance source artifact",
        &plan.provenance.source_artifact,
        false,
        MAX_MIGRATION_PATH_OR_SYMBOL_BYTES,
    )?;
    if plan.steps.is_empty() {
        return Err(MigrationError::InvalidInput(
            "migration plan must contain at least one step".to_string(),
        ));
    }
    if plan.steps.len() > MAX_MIGRATION_STEPS {
        return Err(MigrationError::InvalidInput(format!(
            "migration has {} steps; maximum is {MAX_MIGRATION_STEPS}",
            plan.steps.len()
        )));
    }
    let mut previous = None;
    let mut sources = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for step in &plan.steps {
        validate_step(step)?;
        let key = TermOrdKey(step_key(step));
        if previous.as_ref().is_some_and(|prior| prior >= &key) {
            return Err(MigrationError::InvalidInput(
                "migration steps must be unique and in canonical syntax/API/format order"
                    .to_string(),
            ));
        }
        previous = Some(key);
        if let Some((from, to)) = step_symbols(step) {
            sources.insert(from.to_string());
            targets.insert(to.to_string());
        }
    }
    if sources.iter().any(|source| targets.contains(source)) {
        return Err(MigrationError::InvalidInput(
            "symbol rewrite chains are ambiguous; use separate migration receipts".to_string(),
        ));
    }
    Ok(())
}

fn validate_step(step: &MigrationStep) -> Result<(), MigrationError> {
    match step {
        MigrationStep::RewriteSyntaxHead {
            module_path,
            from,
            to,
            expected_rewrites,
        } => {
            validate_module_path(module_path)?;
            validate_symbol("syntax source", from, false)?;
            validate_symbol("syntax target", to, false)?;
            validate_rewrite(from, to, *expected_rewrites)
        }
        MigrationStep::RenameApiSymbol {
            from,
            to,
            expected_rewrites,
        } => {
            validate_symbol("API source", from, true)?;
            validate_symbol("API target", to, true)?;
            validate_rewrite(from, to, *expected_rewrites)
        }
        MigrationStep::ReplaceFormatField {
            module_path,
            field,
            expected,
            replacement,
        } => {
            validate_module_path(module_path)?;
            if !field.starts_with(':') || field.len() == 1 {
                return Err(MigrationError::InvalidInput(
                    "format field must be a non-empty keyword symbol".to_string(),
                ));
            }
            validate_symbol("format field", field, false)?;
            if expected == replacement {
                return Err(MigrationError::InvalidInput(
                    "format field replacement must change the expected value".to_string(),
                ));
            }
            Ok(())
        }
    }
}

fn validate_rewrite(from: &str, to: &str, expected: usize) -> Result<(), MigrationError> {
    if from == to {
        return Err(MigrationError::InvalidInput(
            "rewrite source and target must differ".to_string(),
        ));
    }
    if expected == 0 || expected > i64::MAX as usize {
        return Err(MigrationError::InvalidInput(
            "expected rewrite count must be in 1..=i64::MAX".to_string(),
        ));
    }
    Ok(())
}

fn validate_text_id(
    label: &str,
    value: &str,
    allow_spaces: bool,
    max_bytes: usize,
) -> Result<(), MigrationError> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.trim() != value
        || value.chars().any(char::is_control)
        || (!allow_spaces && value.chars().any(char::is_whitespace))
    {
        return Err(MigrationError::InvalidInput(format!(
            "{label} must be non-empty, trimmed, portable, and at most {max_bytes} bytes"
        )));
    }
    Ok(())
}

fn validate_symbol(label: &str, value: &str, qualified: bool) -> Result<(), MigrationError> {
    validate_text_id(label, value, false, MAX_MIGRATION_PATH_OR_SYMBOL_BYTES)?;
    if qualified {
        let mut parts = value.split("::");
        if parts.next().is_none_or(str::is_empty)
            || parts.next().is_none_or(str::is_empty)
            || parts.next().is_some()
        {
            return Err(MigrationError::InvalidInput(format!(
                "{label} must contain exactly one non-empty :: separator"
            )));
        }
    }
    Ok(())
}

fn validate_module_path(path: &str) -> Result<(), MigrationError> {
    validate_text_id(
        "module path",
        path,
        false,
        MAX_MIGRATION_PATH_OR_SYMBOL_BYTES,
    )?;
    if path.starts_with('/')
        || !is_nfc(path)
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path
            .split('/')
            .next()
            .is_some_and(|part| part.ends_with(':'))
    {
        return Err(MigrationError::InvalidInput(format!(
            "module path {path} must be portable and base-relative"
        )));
    }
    Ok(())
}

fn apply_step(
    modules: &mut [ModuleForTypecheck],
    step: &MigrationStep,
) -> Result<(), MigrationError> {
    match step {
        MigrationStep::RewriteSyntaxHead {
            module_path,
            from,
            to,
            expected_rewrites,
        } => {
            let module = module_mut(modules, module_path)?;
            let count = rewrite::rewrite_syntax_heads(&mut module.forms, from, to);
            require_count(step, count, *expected_rewrites)?;
            recanonicalize(module)
        }
        MigrationStep::RenameApiSymbol {
            from,
            to,
            expected_rewrites,
        } => {
            rewrite::reject_api_definition_collision(modules, from, to)
                .map_err(MigrationError::Step)?;
            let count = modules
                .iter_mut()
                .map(|module| rewrite::rename_api_symbol(&mut module.forms, from, to))
                .sum();
            require_count(step, count, *expected_rewrites)?;
            for module in modules {
                recanonicalize(module)?;
            }
            Ok(())
        }
        MigrationStep::ReplaceFormatField {
            module_path,
            field,
            expected,
            replacement,
        } => {
            let module = module_mut(modules, module_path)?;
            rewrite::replace_metadata_field(
                &mut module.forms,
                field,
                expected.as_ref(),
                replacement.as_ref(),
            )
            .map_err(MigrationError::Step)?;
            recanonicalize(module)
        }
    }
}

fn module_mut<'a>(
    modules: &'a mut [ModuleForTypecheck],
    path: &str,
) -> Result<&'a mut ModuleForTypecheck, MigrationError> {
    modules
        .iter_mut()
        .find(|module| module.path == path)
        .ok_or_else(|| MigrationError::Step(format!("module {path} is not in the package")))
}

fn require_count(
    step: &MigrationStep,
    actual: usize,
    expected: usize,
) -> Result<(), MigrationError> {
    if actual != expected {
        return Err(MigrationError::Step(format!(
            "{} expected {expected} rewrites, found {actual}",
            step_label(step)
        )));
    }
    Ok(())
}

fn recanonicalize(module: &mut ModuleForTypecheck) -> Result<(), MigrationError> {
    module.forms = canonicalize_module(module.forms.clone()).map_err(|error| {
        MigrationError::Step(format!(
            "module {} is invalid after rewrite: {error:#}",
            module.path
        ))
    })?;
    Ok(())
}

fn refresh_metadata(modules: &mut [ModuleForTypecheck]) -> Result<(), MigrationError> {
    for module in modules {
        module.meta = rewrite::module_metadata(&module.forms).map_err(MigrationError::Step)?;
    }
    Ok(())
}

fn package_identity_term(modules: &[ModuleForTypecheck]) -> Term {
    map([
        (":hash-profile", Term::symbol(HASH_PROFILE_ID)),
        (":kind", Term::symbol(MIGRATION_PROFILE_ID)),
        (
            ":modules",
            Term::Vector(
                modules
                    .iter()
                    .map(|module| {
                        map([
                            (":content-h", bytes32(hash_module(&module.forms))),
                            (":path", Term::Str(module.path.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn step_key(step: &MigrationStep) -> Term {
    match step {
        MigrationStep::RewriteSyntaxHead {
            module_path,
            from,
            to,
            ..
        } => Term::Vector(vec![
            Term::Int(0.into()),
            Term::Str(module_path.clone()),
            Term::Symbol(from.clone()),
            Term::Symbol(to.clone()),
        ]),
        MigrationStep::RenameApiSymbol { from, to, .. } => Term::Vector(vec![
            Term::Int(1.into()),
            Term::Symbol(from.clone()),
            Term::Symbol(to.clone()),
        ]),
        MigrationStep::ReplaceFormatField {
            module_path, field, ..
        } => Term::Vector(vec![
            Term::Int(2.into()),
            Term::Str(module_path.clone()),
            Term::Symbol(field.clone()),
        ]),
    }
}

fn step_symbols(step: &MigrationStep) -> Option<(&str, &str)> {
    match step {
        MigrationStep::RewriteSyntaxHead { from, to, .. }
        | MigrationStep::RenameApiSymbol { from, to, .. } => Some((from, to)),
        MigrationStep::ReplaceFormatField { .. } => None,
    }
}

fn step_label(step: &MigrationStep) -> &'static str {
    match step {
        MigrationStep::RewriteSyntaxHead { .. } => "rewrite-syntax-head",
        MigrationStep::RenameApiSymbol { .. } => "rename-api-symbol",
        MigrationStep::ReplaceFormatField { .. } => "replace-format-field",
    }
}

fn module_deltas(
    before: &[ModuleForTypecheck],
    after: &[ModuleForTypecheck],
) -> Vec<ModuleMigrationDelta> {
    before
        .iter()
        .zip(after)
        .map(|(source, target)| ModuleMigrationDelta {
            path: source.path.clone(),
            before_identity: hash_module(&source.forms),
            after_identity: hash_module(&target.forms),
            changed_form_indices: source
                .forms
                .iter()
                .zip(&target.forms)
                .enumerate()
                .filter_map(|(index, (old, new))| (old != new).then_some(index))
                .collect(),
            effects: effect_delta(infer_effects(&source.forms), infer_effects(&target.forms)),
        })
        .collect()
}

fn package_effects(modules: &[ModuleForTypecheck]) -> InferredEffects {
    let mut out = InferredEffects {
        ops: BTreeSet::new(),
        unknown: false,
    };
    for module in modules {
        let inferred = infer_effects(&module.forms);
        out.ops.extend(inferred.ops);
        out.unknown |= inferred.unknown;
    }
    out
}

fn effect_delta(before: InferredEffects, after: InferredEffects) -> EffectDelta {
    EffectDelta {
        added: after.ops.difference(&before.ops).cloned().collect(),
        removed: before.ops.difference(&after.ops).cloned().collect(),
        before,
        after,
    }
}

fn bytes32(value: [u8; 32]) -> Term {
    Term::Bytes(value.to_vec().into())
}

fn map<const N: usize>(entries: [(&str, Term); N]) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn hex32(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn summarize_errors(errors: &[String]) -> String {
    const MAX_ERRORS: usize = 8;
    const MAX_CHARS_PER_ERROR: usize = 512;
    let mut selected = errors
        .iter()
        .take(MAX_ERRORS)
        .map(|error| error.chars().take(MAX_CHARS_PER_ERROR).collect::<String>())
        .collect::<Vec<_>>();
    if errors.len() > MAX_ERRORS {
        selected.push(format!(
            "{} additional errors omitted",
            errors.len() - MAX_ERRORS
        ));
    }
    selected.join(" | ")
}
