use std::collections::BTreeSet;

use gc_coreform::Term;

use crate::diagnostics::TypecheckDiagnostic;
use crate::profile_negotiation::ProfileNegotiationReport;

#[derive(Debug, Clone)]
pub struct ModuleForTypecheck {
    pub path: String,
    pub forms: Vec<Term>,
    pub meta: Option<Term>, // expected to be a map datum
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InferredEffects {
    pub ops: BTreeSet<String>,
    pub unknown: bool,
}

#[derive(Debug, Clone)]
pub struct TypecheckReport {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub diagnostics: Vec<TypecheckDiagnostic>,
    pub modules: Vec<ModuleReport>,
    pub profile_negotiation: ProfileNegotiationReport,
}

#[derive(Debug, Clone)]
pub struct ModuleReport {
    pub path: String,
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub inferred_effects: InferredEffects,
    pub export_effects: Vec<ExportEffectReport>,
    pub export_types: Vec<ExportTypeReport>,
}

#[derive(Debug, Clone)]
pub struct ExportEffectReport {
    pub name: String,
    pub effects: InferredEffects,
}

#[derive(Debug, Clone)]
pub struct ExportTypeReport {
    pub name: String,
    pub declared: Option<Term>,
    pub inferred: Term,
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}
