use super::*;

struct TaskSchema {
    optional: &'static [&'static str],
    output_keys: &'static [&'static str],
    required: &'static [&'static str],
}

struct TaskSpec {
    handler: fn(&Term) -> TaskOutcome,
    kind: &'static str,
    schema: TaskSchema,
}

const PARSE_OUTPUT_KEYS: &[&str] = &[
    ":ok",
    ":path",
    ":module-h",
    ":form-count",
    ":diagnostics",
    ":error-count",
    ":warn-count",
];
const FMT_OUTPUT_KEYS: &[&str] = &[
    ":ok",
    ":path",
    ":formatted",
    ":formatted-h",
    ":diagnostics",
    ":error-count",
    ":warn-count",
];
const LINT_OUTPUT_KEYS: &[&str] = &[
    ":ok",
    ":module-h",
    ":diagnostics",
    ":error-count",
    ":warn-count",
];
const OPTIMIZE_OUTPUT_KEYS: &[&str] = &[
    ":ok",
    ":path",
    ":module-h",
    ":optimized-h",
    ":changed",
    ":optimized",
    ":optimizer",
    ":diagnostics",
    ":error-count",
    ":warn-count",
];
const PKG_ANALYSIS_OUTPUT_KEYS: &[&str] = &[
    ":ok",
    ":path",
    ":module-count",
    ":test-declaration-count",
    ":defined-symbol-count",
    ":defined-symbols",
    ":diagnostics",
    ":error-count",
    ":warn-count",
];
const BUILD_OUTPUT_KEYS: &[&str] = &[
    ":ok",
    ":path",
    ":module-count",
    ":build/targets",
    ":build/artifact-h",
    ":build/mode",
    ":diagnostics",
    ":error-count",
    ":warn-count",
];
const RUN_OUTPUT_KEYS: &[&str] = &[
    ":ok",
    ":path",
    ":run/entry",
    ":run/launch-contract",
    ":diagnostics",
    ":error-count",
    ":warn-count",
];
const DEBUG_OUTPUT_KEYS: &[&str] = &[
    ":ok",
    ":path",
    ":debug/entry",
    ":debug/session-id",
    ":debug/breakpoints",
    ":diagnostics",
    ":error-count",
    ":warn-count",
];
const REFACTOR_OUTPUT_KEYS: &[&str] = &[
    ":ok",
    ":path",
    ":from",
    ":to",
    ":changed",
    ":module-h",
    ":updated-h",
    ":updated",
    ":diagnostics",
    ":error-count",
    ":warn-count",
];
const INDEX_OUTPUT_KEYS: &[&str] = &[
    ":ok",
    ":workspace/root",
    ":package-count",
    ":packages",
    ":diagnostics",
    ":error-count",
    ":warn-count",
];

fn task_specs() -> &'static [TaskSpec] {
    &[
        TaskSpec {
            kind: "editor/task::parse-module",
            handler: |input| TaskOutcome::immediate(task_parse_module(input)),
            schema: TaskSchema {
                required: &[],
                optional: &[":path", ":source"],
                output_keys: PARSE_OUTPUT_KEYS,
            },
        },
        TaskSpec {
            kind: "editor/task::fmt-coreform",
            handler: |input| TaskOutcome::immediate(task_fmt_coreform(input)),
            schema: TaskSchema {
                required: &[],
                optional: &[":path", ":source"],
                output_keys: FMT_OUTPUT_KEYS,
            },
        },
        TaskSpec {
            kind: "editor/task::lint-module",
            handler: |input| TaskOutcome::immediate(task_lint_module(input)),
            schema: TaskSchema {
                required: &[],
                optional: &[":input", ":path", ":source"],
                output_keys: LINT_OUTPUT_KEYS,
            },
        },
        TaskSpec {
            kind: "editor/task::optimize-module",
            handler: |input| TaskOutcome::immediate(task_optimize_module(input)),
            schema: TaskSchema {
                required: &[],
                optional: &[":out", ":path", ":source"],
                output_keys: OPTIMIZE_OUTPUT_KEYS,
            },
        },
        TaskSpec {
            kind: "editor/task::typecheck-pkg",
            handler: |input| TaskOutcome::immediate(task_typecheck_pkg(input)),
            schema: TaskSchema {
                required: &[":pkg"],
                optional: &[":path"],
                output_keys: PKG_ANALYSIS_OUTPUT_KEYS,
            },
        },
        TaskSpec {
            kind: "editor/task::test-pkg",
            handler: |input| TaskOutcome::immediate(task_test_pkg(input)),
            schema: TaskSchema {
                required: &[":pkg"],
                optional: &[":caps", ":path"],
                output_keys: PKG_ANALYSIS_OUTPUT_KEYS,
            },
        },
        TaskSpec {
            kind: "editor/task::build-pkg",
            handler: runner_editor_task_workflows::task_build_pkg,
            schema: TaskSchema {
                required: &[":pkg"],
                optional: &[":path", ":targets"],
                output_keys: BUILD_OUTPUT_KEYS,
            },
        },
        TaskSpec {
            kind: "editor/task::run-pkg",
            handler: runner_editor_task_workflows::task_run_pkg,
            schema: TaskSchema {
                required: &[":pkg"],
                optional: &[":args", ":entry", ":path"],
                output_keys: RUN_OUTPUT_KEYS,
            },
        },
        TaskSpec {
            kind: "editor/task::debug-pkg",
            handler: runner_editor_task_workflows::task_debug_pkg,
            schema: TaskSchema {
                required: &[":pkg"],
                optional: &[":breakpoints", ":entry", ":path"],
                output_keys: DEBUG_OUTPUT_KEYS,
            },
        },
        TaskSpec {
            kind: "editor/task::refactor-module",
            handler: runner_editor_task_workflows::task_refactor_module,
            schema: TaskSchema {
                required: &[":from", ":to"],
                optional: &[":path", ":source"],
                output_keys: REFACTOR_OUTPUT_KEYS,
            },
        },
        TaskSpec {
            kind: "editor/task::index-workspace",
            handler: runner_editor_task_workflows::task_index_workspace,
            schema: TaskSchema {
                required: &[":root"],
                optional: &[":max-packages", ":max-partials"],
                output_keys: INDEX_OUTPUT_KEYS,
            },
        },
    ]
}

fn task_spec(kind: &str) -> Option<&'static TaskSpec> {
    task_specs().iter().find(|spec| spec.kind == kind)
}

pub(super) fn execute_editor_task(kind: &str, input: &Term) -> TaskExecution {
    let Some(spec) = task_spec(kind) else {
        return unsupported_task_execution(kind);
    };
    if let Err(err) = validate_task_schema(spec, input) {
        return TaskExecution {
            contract: task_contract(spec),
            partials: Vec::new(),
            result: err,
        };
    }
    let out = (spec.handler)(input);
    TaskExecution {
        contract: task_contract(spec),
        partials: out.partials,
        result: out.result,
    }
}

fn unsupported_task_execution(kind: &str) -> TaskExecution {
    let supported = task_specs()
        .iter()
        .map(|spec| Term::symbol(spec.kind))
        .collect::<Vec<_>>();
    let supported_for_contract = supported.clone();
    let result = map_term(vec![
        (":ok", Term::Bool(false)),
        (":error/op", Term::symbol(kind)),
        (
            ":error/code",
            Term::Str("editor/first-party-task-kind-unsupported".to_string()),
        ),
        (
            ":error/message",
            Term::Str(format!("unsupported task kind: {kind}")),
        ),
        (":supported-task-kinds", Term::Vector(supported)),
        (
            ":diagnostics",
            Term::Vector(vec![diag_term(
                ":error",
                "editor/first-party-task-kind-unsupported",
                "<task>",
                format!("unsupported task kind: {kind}"),
            )]),
        ),
        (":error-count", Term::Int(1_i64.into())),
        (":warn-count", Term::Int(0_i64.into())),
    ]);
    let contract = map_term(vec![
        (":task-kind", Term::symbol(kind)),
        (":schema-version", Term::Int(1_i64.into())),
        (
            ":supported-task-kinds",
            Term::Vector(supported_for_contract),
        ),
    ]);
    TaskExecution {
        contract,
        partials: Vec::new(),
        result,
    }
}

fn validate_task_schema(spec: &TaskSpec, input: &Term) -> Result<(), Term> {
    let Some(input_map) = payload_map(input) else {
        return Err(task_diagnostic_error(
            spec.kind,
            "editor/task/schema-invalid-input",
            "<task>",
            "expected map input".to_string(),
        ));
    };
    for field in spec.schema.required {
        if !input_map.contains_key(&TermOrdKey(Term::symbol(*field))) {
            return Err(task_diagnostic_error(
                spec.kind,
                "editor/task/schema-missing-field",
                "<task>",
                format!("missing required field {field}"),
            ));
        }
    }
    Ok(())
}

fn task_contract(spec: &TaskSpec) -> Term {
    map_term(vec![
        (":task-kind", Term::symbol(spec.kind)),
        (":schema-version", Term::Int(1_i64.into())),
        (
            ":schema/required",
            Term::Vector(
                spec.schema
                    .required
                    .iter()
                    .map(|field| Term::symbol(*field))
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            ":schema/optional",
            Term::Vector(
                spec.schema
                    .optional
                    .iter()
                    .map(|field| Term::symbol(*field))
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            ":schema/output-keys",
            Term::Vector(
                spec.schema
                    .output_keys
                    .iter()
                    .map(|field| Term::symbol(*field))
                    .collect::<Vec<_>>(),
            ),
        ),
    ])
}
