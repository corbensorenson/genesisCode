#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypecheckDiagnostic {
    pub id: String,
    pub code: String,
    pub severity: String,
    pub module_path: String,
    pub ordinal: u64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypecheckExportEffectReport {
    pub name: String,
    pub ops: BTreeSet<String>,
    pub unknown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypecheckExportTypeReport {
    pub name: String,
    pub declared: Option<Term>,
    pub inferred: Term,
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypecheckModuleReport {
    pub path: String,
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub inferred_ops: BTreeSet<String>,
    pub unknown_ops: bool,
    pub export_effects: Vec<TypecheckExportEffectReport>,
    pub export_types: Vec<TypecheckExportTypeReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypecheckModuleInput {
    pub path: String,
    pub forms: Vec<Term>,
    pub meta: Option<Term>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeTypecheckReport {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub diagnostics: Vec<TypecheckDiagnostic>,
    pub modules: Vec<TypecheckModuleReport>,
    term: Term,
}

impl AuthoritativeTypecheckReport {
    pub fn to_term(&self) -> Term {
        self.term.clone()
    }
}

fn typecheck_error(message: impl Into<String>) -> ObligationError {
    ObligationError::Typecheck(message.into())
}

fn typecheck_request_term(modules: &[TypecheckModuleInput]) -> Term {
    let request_modules = modules
        .iter()
        .map(|module| {
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":forms")),
                        Term::Vector(module.forms.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":meta")),
                        module.meta.clone().unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":path")),
                        Term::Str(module.path.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/typecheck-request-v0.1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(request_modules),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn selfhost_typecheck_report(
    modules: &[TypecheckModuleInput],
    config: &SelfhostFrontendConfig,
    limits: KernelLimits,
) -> Result<AuthoritativeTypecheckReport, ObligationError> {
    let mut ctx = EvalCtx::with_step_limit(None);
    ctx.set_mem_limits(limits.mem_limits);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    load_selfhost_coreform_toolchain_v1_with_mode(
        &mut ctx,
        &mut env,
        config.bootstrap_mode,
        config.artifact.as_deref(),
    )
    .map_err(|error| typecheck_error(format!("selfhost/init: {error}")))?;
    ctx.steps = 0;
    ctx.step_limit = limits.step_limit.resolve();

    let checker = env.get("core/cli::typecheck-package").ok_or_else(|| {
        typecheck_error("missing required production binding core/cli::typecheck-package")
    })?;
    let request = typecheck_request_term(modules);
    let value = checker
        .apply(&mut ctx, Value::data(request))
        .map_err(|error| typecheck_error(format!("core/cli::typecheck-package: {error}")))?;
    if let Some(error) = extract_protocol_error(&ctx, &value) {
        return Err(typecheck_error(format!(
            "selfhost core/cli::typecheck-package failed: {error}"
        )));
    }
    let term = value
        .as_data()
        .cloned()
        .unwrap_or_else(|| value.to_term_for_log(ctx.protocol.map(|protocol| protocol.error)));
    decode_typecheck_report(term, modules)
}

pub fn typecheck_modules_with_authority(
    modules: &[TypecheckModuleInput],
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<AuthoritativeTypecheckReport, ObligationError> {
    enforce_frontend_allowed(frontend, "type/effect check")?;
    if frontend_is_rust(frontend) {
        #[cfg(not(feature = "parity-oracle"))]
        return Err(typecheck_error(
            "Rust type/effect oracle is not compiled into production; use a dedicated parity harness binary",
        ));
        #[cfg(feature = "parity-oracle")]
        {
        let rust_modules = modules
            .iter()
            .map(|module| gc_types::ModuleForTypecheck {
                path: module.path.clone(),
                forms: module.forms.clone(),
                meta: module.meta.clone(),
            })
            .collect::<Vec<_>>();
        return decode_typecheck_report(
            gc_types::typecheck_package(&rust_modules).to_term(),
            modules,
        );
        }
    }
    let CoreformFrontend::Selfhost(config) = frontend else {
        return Err(typecheck_error("invalid typecheck frontend dispatch"));
    };
    selfhost_typecheck_report(
        modules,
        config,
        KernelLimits {
            step_limit,
            mem_limits,
        },
    )
}
