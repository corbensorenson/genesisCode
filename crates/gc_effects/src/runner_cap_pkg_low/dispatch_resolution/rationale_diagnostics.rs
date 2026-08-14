use super::*;

fn extract_available_tags(ctx: &Term) -> Vec<String> {
    let mut tags = Vec::new();
    let Term::Map(map) = ctx else {
        return tags;
    };
    let Some(Term::Vector(values)) = map.get(&TermOrdKey(Term::symbol(":available-tags"))) else {
        return tags;
    };
    for value in values {
        if let Term::Str(tag) = value {
            tags.push(tag.clone());
        }
    }
    tags
}

fn resolution_repair_hints(
    req: &gc_pkg::Requirement,
    error_code: &str,
    existing_ctx: &Term,
) -> Term {
    let (error_class, candidate_fix, next_safe_action) = match error_code {
        "core/pkg/semver-no-match" => (
            ":selector-conflict",
            "adjust semver range or tag_policy to match an available tag",
            "inspect available tags and rerun gcpm add/update with an explicit selector",
        ),
        "core/pkg/ref-not-found" => (
            ":missing-ref",
            "switch selector to an existing ref or commit hash",
            "list refs for the registry and retry with refs/heads/* or refs/tags/*",
        ),
        "core/pkg/registry-not-found" => (
            ":registry-alias",
            "set registry alias to default or add alias under [registries] in genesis.lock",
            "repair lock registries map then rerun gcpm lock",
        ),
        "core/pkg/bad-selector" => (
            ":invalid-selector",
            "rewrite selector using commit:, snapshot:, ref:, refs/*, or semver:<range>",
            "normalize selector syntax and rerun gcpm add/lock",
        ),
        _ => (
            ":resolution-failure",
            "inspect resolver context and choose a deterministic selector override",
            "run gcpm doctor and retry gcpm lock/update after applying one fix",
        ),
    };

    let available_tags = extract_available_tags(existing_ctx);
    let candidate_selectors =
        if error_code == "core/pkg/semver-no-match" && !available_tags.is_empty() {
            available_tags
                .into_iter()
                .map(|tag| Term::Str(format!("ref:{tag}")))
                .collect()
        } else {
            vec![Term::Str(req.selector.clone())]
        };

    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":error-class")),
                Term::symbol(error_class),
            ),
            (
                TermOrdKey(Term::symbol(":candidate-fix")),
                Term::Str(candidate_fix.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":next-safe-action")),
                Term::Str(next_safe_action.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":candidate-selectors")),
                Term::Vector(candidate_selectors),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn annotate_requirement_resolution_error(
    err: Value,
    name: &str,
    req: &gc_pkg::Requirement,
) -> Value {
    let Value::Sealed { token, payload } = err else {
        return err;
    };
    let mut mm = match payload.as_ref() {
        Value::Data(t) => match t.as_ref() {
            Term::Map(mm) => mm.clone(),
            _ => return Value::Sealed { token, payload },
        },
        _ => return Value::Sealed { token, payload },
    };
    let existing_ctx = mm
        .get(&TermOrdKey(Term::symbol(":error/context")))
        .cloned()
        .unwrap_or(Term::Nil);
    let error_code = mm
        .get(&TermOrdKey(Term::symbol(":error/code")))
        .and_then(|t| match t {
            Term::Str(s) => Some(s.as_str()),
            _ => None,
        })
        .unwrap_or("core/pkg/resolution-error");
    let repair_hints = resolution_repair_hints(req, error_code, &existing_ctx);

    let mut ctx = BTreeMap::new();
    ctx.insert(
        TermOrdKey(Term::symbol(":name")),
        Term::Str(name.to_string()),
    );
    ctx.insert(
        TermOrdKey(Term::symbol(":selector")),
        Term::Str(req.selector.clone()),
    );
    ctx.insert(
        TermOrdKey(Term::symbol(":strategy")),
        Term::symbol(format!(":{}", req.strategy.as_str())),
    );
    ctx.insert(
        TermOrdKey(Term::symbol(":registry")),
        req.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
    );
    ctx.insert(TermOrdKey(Term::symbol(":repair-hints")), repair_hints);
    ctx.insert(TermOrdKey(Term::symbol(":inner")), existing_ctx);
    mm.insert(TermOrdKey(Term::symbol(":error/context")), Term::Map(ctx));
    Value::Sealed {
        token,
        payload: Box::new(Value::data(Term::Map(mm))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotate_requirement_resolution_error_adds_repair_hints() {
        let mut err_map = BTreeMap::new();
        err_map.insert(
            TermOrdKey(Term::symbol(":error/code")),
            Term::Str("core/pkg/semver-no-match".to_string()),
        );
        err_map.insert(
            TermOrdKey(Term::symbol(":error/context")),
            Term::Map(
                [(
                    TermOrdKey(Term::symbol(":available-tags")),
                    Term::Vector(vec![Term::Str("refs/tags/v1.2.3".to_string())]),
                )]
                .into_iter()
                .collect(),
            ),
        );
        let err = Value::Sealed {
            token: SealId(91),
            payload: Box::new(Value::data(Term::Map(err_map))),
        };
        let req = gc_pkg::Requirement {
            selector: "semver:^2.0.0".to_string(),
            update_policy: gc_pkg::UpdatePolicy::Auto,
            registry: Some("default".to_string()),
            strategy: gc_pkg::ResolutionStrategy::TagPolicy,
            tag_policy: Some("highest".to_string()),
        };
        let out = annotate_requirement_resolution_error(err, "demo", &req);
        let Value::Sealed { payload, .. } = out else {
            panic!("expected sealed value");
        };
        let Some(Term::Map(mm)) = payload.as_ref().as_data() else {
            panic!("expected map payload");
        };
        let Some(Term::Map(ctx)) = mm.get(&TermOrdKey(Term::symbol(":error/context"))) else {
            panic!("expected error/context map");
        };
        let Some(Term::Map(hints)) = ctx.get(&TermOrdKey(Term::symbol(":repair-hints"))) else {
            panic!("expected repair-hints map");
        };
        assert_eq!(
            hints.get(&TermOrdKey(Term::symbol(":error-class"))),
            Some(&Term::symbol(":selector-conflict"))
        );
        let Some(Term::Vector(candidates)) =
            hints.get(&TermOrdKey(Term::symbol(":candidate-selectors")))
        else {
            panic!("expected candidate-selectors");
        };
        assert_eq!(candidates.len(), 1);
    }
}
