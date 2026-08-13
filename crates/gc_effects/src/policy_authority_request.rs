use super::*;

pub(super) fn term(
    op: &str,
    baseline: &[String],
    override_value: Term,
    gfx_policy: Term,
    gpu_policy: Term,
    xr_policy: Term,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":baseline")),
                Term::Vector(baseline.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/effect-policy-authority-request-v0.18".to_string()),
            ),
            (TermOrdKey(Term::symbol(":gfx-policy")), gfx_policy),
            (TermOrdKey(Term::symbol(":gpu-policy")), gpu_policy),
            (TermOrdKey(Term::symbol(":op")), Term::Str(op.to_string())),
            (TermOrdKey(Term::symbol(":override")), override_value),
            (
                TermOrdKey(Term::symbol(":platform-max-bytes")),
                Term::Int(usize::MAX.into()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(18.into())),
            (TermOrdKey(Term::symbol(":xr-policy")), xr_policy),
        ]
        .into_iter()
        .collect(),
    )
}
