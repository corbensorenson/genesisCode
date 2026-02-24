use super::*;

pub(super) fn report_term(
    patch: &Patch,
    ok: bool,
    package_artifact: &Option<String>,
    acceptance: Option<&PackageTestResult>,
    semantic_edits: &[AppliedSemanticEdit],
) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("genesis/patch-apply-v0.2".to_string()),
    );
    m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
    m.insert(
        TermOrdKey(Term::symbol(":intent")),
        Term::Str(patch.intent.clone()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":provenance")),
        patch.provenance.clone(),
    );
    m.insert(
        TermOrdKey(Term::symbol(":ops-count")),
        Term::Int((patch.ops.len() as i64).into()),
    );
    if let Some(p) = package_artifact {
        m.insert(
            TermOrdKey(Term::symbol(":package-artifact")),
            Term::Str(p.clone()),
        );
    }
    if let Some(a) = acceptance {
        m.insert(
            TermOrdKey(Term::symbol(":acceptance-artifact")),
            Term::Str(a.acceptance_artifact.clone()),
        );
    }
    if !semantic_edits.is_empty() {
        let mut edits = Vec::with_capacity(semantic_edits.len());
        for edit in semantic_edits {
            let mut em = BTreeMap::new();
            em.insert(
                TermOrdKey(Term::symbol(":op")),
                Term::Symbol(edit.op.to_string()),
            );
            em.insert(
                TermOrdKey(Term::symbol(":module-path")),
                Term::Str(edit.module_path.clone()),
            );
            if let Some(node_id) = &edit.node_id {
                em.insert(
                    TermOrdKey(Term::symbol(":node-id")),
                    Term::Str(node_id.clone()),
                );
            }
            if let Some(path) = &edit.path {
                em.insert(
                    TermOrdKey(Term::symbol(":path")),
                    path_steps_to_term(path).unwrap_or(Term::Vector(Vec::new())),
                );
            }
            if let Some(new_term_hash) = &edit.new_term_hash {
                em.insert(
                    TermOrdKey(Term::symbol(":new-term-h")),
                    Term::Str(new_term_hash.clone()),
                );
            }
            if let Some(before_h) = &edit.before_module_hash {
                em.insert(
                    TermOrdKey(Term::symbol(":before-module-h")),
                    Term::Str(before_h.clone()),
                );
            }
            if let Some(after_h) = &edit.after_module_hash {
                em.insert(
                    TermOrdKey(Term::symbol(":after-module-h")),
                    Term::Str(after_h.clone()),
                );
            }
            if let Some(detail) = &edit.detail {
                em.insert(TermOrdKey(Term::symbol(":detail")), detail.clone());
            }
            edits.push(Term::Map(em));
        }
        m.insert(
            TermOrdKey(Term::symbol(":semantic-edits")),
            Term::Vector(edits),
        );
    }
    Term::Map(m)
}
