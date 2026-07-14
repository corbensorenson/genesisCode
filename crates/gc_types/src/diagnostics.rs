use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};

#[derive(Debug, Clone)]
pub struct TypecheckDiagnostic {
    pub id: String,
    pub code: &'static str,
    pub severity: &'static str,
    pub module_path: String,
    pub ordinal: u64,
    pub message: String,
}

pub(crate) fn module_diagnostics(
    module_path: &str,
    errors: &[String],
    warnings: &[String],
) -> Vec<TypecheckDiagnostic> {
    errors
        .iter()
        .enumerate()
        .map(|(ordinal, message)| TypecheckDiagnostic {
            id: format!("{module_path}#error#{ordinal}"),
            code: "typecheck/error",
            severity: "error",
            module_path: module_path.to_string(),
            ordinal: u64::try_from(ordinal).unwrap_or(u64::MAX),
            message: message.clone(),
        })
        .chain(
            warnings
                .iter()
                .enumerate()
                .map(|(ordinal, message)| TypecheckDiagnostic {
                    id: format!("{module_path}#warning#{ordinal}"),
                    code: "typecheck/warning",
                    severity: "warning",
                    module_path: module_path.to_string(),
                    ordinal: u64::try_from(ordinal).unwrap_or(u64::MAX),
                    message: message.clone(),
                }),
        )
        .collect()
}

impl TypecheckDiagnostic {
    pub fn to_term(&self) -> Term {
        let mut fields = BTreeMap::new();
        fields.insert(TermOrdKey(Term::symbol(":id")), Term::Str(self.id.clone()));
        fields.insert(
            TermOrdKey(Term::symbol(":code")),
            Term::Symbol(self.code.to_string()),
        );
        fields.insert(
            TermOrdKey(Term::symbol(":severity")),
            Term::Symbol(self.severity.to_string()),
        );
        fields.insert(
            TermOrdKey(Term::symbol(":domain")),
            Term::Symbol("typechecker".to_string()),
        );
        fields.insert(
            TermOrdKey(Term::symbol(":module")),
            Term::Str(self.module_path.clone()),
        );
        fields.insert(
            TermOrdKey(Term::symbol(":ordinal")),
            Term::Int(self.ordinal.into()),
        );
        fields.insert(
            TermOrdKey(Term::symbol(":message")),
            Term::Str(self.message.clone()),
        );
        Term::Map(fields)
    }
}

impl crate::TypecheckReport {
    pub fn to_term(&self) -> Term {
        let mut fields = BTreeMap::new();
        fields.insert(
            TermOrdKey(Term::symbol(":kind")),
            Term::Str("genesis/typecheck-v0.2".to_string()),
        );
        fields.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(self.ok));
        fields.insert(
            TermOrdKey(Term::symbol(":errors")),
            Term::Vector(self.errors.iter().cloned().map(Term::Str).collect()),
        );
        fields.insert(
            TermOrdKey(Term::symbol(":warnings")),
            Term::Vector(self.warnings.iter().cloned().map(Term::Str).collect()),
        );
        fields.insert(
            TermOrdKey(Term::symbol(":diagnostics")),
            Term::Vector(
                self.diagnostics
                    .iter()
                    .map(TypecheckDiagnostic::to_term)
                    .collect(),
            ),
        );
        fields.insert(
            TermOrdKey(Term::symbol(":modules")),
            Term::Vector(
                self.modules
                    .iter()
                    .map(crate::ModuleReport::to_term)
                    .collect(),
            ),
        );
        Term::Map(fields)
    }
}
