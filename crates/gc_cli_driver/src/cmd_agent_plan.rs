use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

const KIND_AGENT_PLAN: &str = "genesis/agent-plan-v0.1";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentIntent {
    #[serde(default)]
    schema: Option<String>,
    #[serde(default)]
    goal: String,
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default)]
    required_workflows: Vec<String>,
    #[serde(default)]
    exclude_workflows: Vec<String>,
    #[serde(default)]
    required_ops: Vec<String>,
    #[serde(default)]
    max_workflows: Option<usize>,
}

#[derive(Debug, Clone)]
struct WorkflowCandidate {
    name: String,
    rel_dir: String,
    rel_script: String,
    script_exists: bool,
    script_hash_blake3: Option<String>,
    tags: BTreeSet<String>,
    required_ops: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PlannerFailure {
    code: String,
    severity: String,
    message: String,
    repair_hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<serde_json::Value>,
}

#[derive(Debug)]
struct PolicyPrecheck {
    caps_path: String,
    checked: bool,
    ok: bool,
    denied_ops: Vec<String>,
    error: Option<String>,
}

pub(super) fn cmd_agent_plan(
    cli: &Cli,
    intent_path: &Path,
    caps_path: &Path,
    max_workflows_cli: usize,
) -> Result<CmdOut, CliError> {
    let cwd = std::env::current_dir().map_err(|e| cli_err(EX_IO, "io/cwd", format!("{e}")))?;
    let repo_root = resolve_repo_root(&cwd);
    let examples_dir = repo_root.join("examples");
    let workflow_catalog = collect_workflow_catalog(&examples_dir);

    let mut failures = Vec::<PlannerFailure>::new();
    let mut intent_raw = serde_json::json!({});
    let mut intent = AgentIntent {
        schema: None,
        goal: String::new(),
        domains: Vec::new(),
        required_workflows: Vec::new(),
        exclude_workflows: Vec::new(),
        required_ops: Vec::new(),
        max_workflows: None,
    };
    match load_agent_intent(intent_path) {
        Ok((raw, parsed)) => {
            intent_raw = raw;
            intent = normalize_intent(parsed);
            if let Some(schema) = intent.schema.as_deref()
                && schema != "genesis/agent-intent-v0.1"
            {
                failures.push(planner_failure(
                    "agent-plan/intent-schema-unsupported",
                    &format!(
                        "intent schema `{schema}` is unsupported (expected genesis/agent-intent-v0.1)"
                    ),
                    vec![
                        "set `schema` to `genesis/agent-intent-v0.1`".to_string(),
                        "or remove `schema` to accept default planner contract".to_string(),
                    ],
                    None,
                ));
            }
            if intent.goal.is_empty() {
                failures.push(planner_failure(
                    "agent-plan/intent-missing-goal",
                    "intent `goal` must be a non-empty string",
                    vec![
                        "set `goal` to the concrete outcome the workflow must produce".to_string(),
                        "include `domains` and/or `required_workflows` to narrow planner choices"
                            .to_string(),
                    ],
                    None,
                ));
            }
        }
        Err(e) => failures.push(planner_failure(
            "agent-plan/intent-invalid",
            &e,
            vec![
                "provide a valid JSON intent contract (`genesis/agent-intent-v0.1`)".to_string(),
                "example: {\"goal\":\"build service\",\"domains\":[\"service\"]}".to_string(),
            ],
            None,
        )),
    }

    let selected = select_workflows(&intent, &workflow_catalog, max_workflows_cli, &mut failures);
    let mut selected_required_ops: BTreeSet<String> = intent
        .required_ops
        .iter()
        .filter(|op| looks_like_effect_op(op))
        .cloned()
        .collect();
    for wf in &selected {
        selected_required_ops.extend(wf.required_ops.iter().cloned());
        if !wf.script_exists {
            failures.push(planner_failure(
                "agent-plan/workflow-script-missing",
                &format!(
                    "selected workflow `{}` is missing script `{}`",
                    wf.name, wf.rel_script
                ),
                vec![
                    "restore workflow.sh under the selected workflow directory".to_string(),
                    "or exclude this workflow and rerun planning".to_string(),
                ],
                Some(serde_json::json!({
                    "workflow": wf.name,
                    "path": wf.rel_script,
                })),
            ));
        }
    }

    let policy = policy_precheck(caps_path, &selected_required_ops);
    if !policy.ok {
        let hints = if !policy.denied_ops.is_empty() {
            vec![
                "add denied ops to caps `allow = [...]` (or choose workflows requiring fewer ops)"
                    .to_string(),
                "rerun `genesis --json agent-plan ...` to confirm a fully policy-closed plan"
                    .to_string(),
            ]
        } else {
            vec![
                "fix or create the policy file, then rerun planning".to_string(),
                "ensure caps.toml parses and contains deterministic allowlist entries".to_string(),
            ]
        };
        failures.push(planner_failure(
            "agent-plan/policy-precheck-failed",
            "policy precheck failed for selected workflow capability surface",
            hints,
            Some(serde_json::json!({
                "caps": policy.caps_path,
                "denied_ops": policy.denied_ops,
                "error": policy.error,
            })),
        ));
    }

    failures.sort_by(|a, b| a.code.cmp(&b.code).then(a.message.cmp(&b.message)));
    let failures_json = serde_json::to_value(&failures).map_err(|e| {
        cli_err(
            EX_INTERNAL,
            "json/serialize",
            format!("failed to serialize planner failures: {e}"),
        )
    })?;

    let nodes = build_plan_nodes(&selected);
    let edges = build_plan_edges(&selected);
    let steps = build_execution_steps(&selected);

    let intent_hash_blake3 = blake3::hash(json_canonical_string(&intent_raw).as_bytes())
        .to_hex()
        .to_string();
    let catalog_hash_blake3 = catalog_hash_blake3(&workflow_catalog)?;
    let context_cards = if intent.goal.is_empty() {
        serde_json::Value::Null
    } else {
        cmd_agent_task_cards::select_task_cards(
            &intent.goal,
            &intent.domains,
            &intent.required_workflows,
            &intent.exclude_workflows,
            &intent.required_ops,
            intent.max_workflows,
        )
        .map_err(|error| cli_err(EX_INTERNAL, "agent-plan/task-cards", error))?
    };
    let mut plan_core = serde_json::json!({
        "schema": KIND_AGENT_PLAN,
        "intent_hash_blake3": intent_hash_blake3,
        "catalog_hash_blake3": catalog_hash_blake3,
        "selected_workflows": selected.iter().map(|w| w.name.clone()).collect::<Vec<_>>(),
        "nodes": nodes,
        "edges": edges,
        "required_ops": selected_required_ops.iter().cloned().collect::<Vec<_>>(),
        "policy": {
            "caps": policy.caps_path,
            "checked": policy.checked,
            "ok": policy.ok,
            "denied_ops": policy.denied_ops,
            "error": policy.error,
        },
        "failures": failures_json,
        "context_cards": context_cards,
    });
    let plan_hash_blake3 = blake3::hash(json_canonical_string(&plan_core).as_bytes())
        .to_hex()
        .to_string();
    if let Some(obj) = plan_core.as_object_mut() {
        obj.insert(
            "plan_hash_blake3".to_string(),
            serde_json::Value::String(plan_hash_blake3.clone()),
        );
    }

    let repair_hints: Vec<String> = failures
        .iter()
        .flat_map(|f| f.repair_hints.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let ok = failures.is_empty();
    let exit_code = if ok {
        EX_OK
    } else if failures
        .iter()
        .any(|f| f.code.starts_with("agent-plan/intent-"))
    {
        EX_PARSE
    } else {
        EX_VERIFY
    };

    let env = JsonEnvelope {
        ok,
        kind: KIND_AGENT_PLAN,
        data: Some(serde_json::json!({
            "schema": KIND_AGENT_PLAN,
            "runtime_profile": cli_schema::runtime_profile_token(runtime_profile()),
            "repo_root": repo_root.display().to_string(),
            "intent_path": intent_path.display().to_string(),
            "intent": intent_raw,
            "plan": plan_core,
            "execution": {
                "kind": "genesis/agent-workflow-dag-v0.1",
                "steps": steps,
                "effect_log_op": "agent-plan/execute",
            },
            "lineage": {
                "intent_hash_blake3": intent_hash_blake3,
                "catalog_hash_blake3": catalog_hash_blake3,
                "plan_hash_blake3": plan_hash_blake3,
                "evidence_targets": [
                    ".genesis/perf/agent_workflow_runtime_parity_report.json",
                    ".genesis/perf/agent_capability_gauntlet_release_confidence_report.json",
                ],
            },
            "failure_taxonomy": failures,
            "repair_hints": repair_hints,
        })),
        error: None,
    };
    let json = json_envelope_value(env)?;
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}

fn resolve_repo_root(start: &Path) -> PathBuf {
    for candidate in start.ancestors() {
        if candidate.join("docs/spec/CLI.md").is_file() && candidate.join("examples").is_dir() {
            return candidate.to_path_buf();
        }
    }
    start.to_path_buf()
}

fn planner_failure(
    code: &str,
    message: &str,
    repair_hints: Vec<String>,
    context: Option<serde_json::Value>,
) -> PlannerFailure {
    PlannerFailure {
        code: code.to_string(),
        severity: "error".to_string(),
        message: message.to_string(),
        repair_hints,
        context,
    }
}

fn load_agent_intent(intent_path: &Path) -> Result<(serde_json::Value, AgentIntent), String> {
    let mut src = String::new();
    if intent_path == Path::new("-") {
        std::io::stdin()
            .read_to_string(&mut src)
            .map_err(|e| format!("failed to read intent from stdin: {e}"))?;
    } else {
        src = std::fs::read_to_string(intent_path)
            .map_err(|e| format!("failed to read `{}`: {e}", intent_path.display()))?;
    }
    let raw: serde_json::Value =
        serde_json::from_str(&src).map_err(|e| format!("intent JSON parse error: {e}"))?;
    let parsed: AgentIntent = serde_json::from_value(raw.clone())
        .map_err(|e| format!("intent schema validation failed: {e}"))?;
    Ok((raw, parsed))
}

fn normalize_intent(mut intent: AgentIntent) -> AgentIntent {
    intent.goal = intent.goal.trim().to_string();
    intent.domains = normalize_symbol_list(&intent.domains);
    intent.required_workflows = normalize_symbol_list(&intent.required_workflows);
    intent.exclude_workflows = normalize_symbol_list(&intent.exclude_workflows);
    intent.required_ops = normalize_symbol_list(&intent.required_ops);
    intent
}

fn normalize_symbol_list(items: &[String]) -> Vec<String> {
    let mut out: Vec<String> = items
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

fn collect_workflow_catalog(examples_dir: &Path) -> Vec<WorkflowCandidate> {
    let mut names = Vec::<String>::new();
    if let Ok(rd) = std::fs::read_dir(examples_dir) {
        for entry in rd.flatten() {
            if let Ok(ft) = entry.file_type()
                && ft.is_dir()
            {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if name.starts_with("agent_") {
                    names.push(name);
                }
            }
        }
    }
    names.sort();
    names.dedup();

    names
        .into_iter()
        .map(|name| {
            let rel_dir = format!("examples/{name}");
            let rel_script = format!("{rel_dir}/workflow.sh");
            let base = examples_dir.join(&name);
            let script_path = base.join("workflow.sh");
            let script_exists = script_path.is_file();
            let script_hash_blake3 = std::fs::read(&script_path)
                .ok()
                .map(|bytes| blake3::hash(&bytes).to_hex().to_string());
            let required_ops = infer_workflow_required_ops(&base, &script_path);
            let tags = workflow_tags(&name);
            WorkflowCandidate {
                name,
                rel_dir,
                rel_script,
                script_exists,
                script_hash_blake3,
                tags,
                required_ops,
            }
        })
        .collect()
}

fn workflow_tags(name: &str) -> BTreeSet<String> {
    name.split('_')
        .filter(|token| *token != "agent" && *token != "workflow" && !token.is_empty())
        .map(|token| token.to_string())
        .collect()
}

fn infer_workflow_required_ops(workflow_dir: &Path, script_path: &Path) -> BTreeSet<String> {
    let caps_path = workflow_dir.join("caps.toml");
    if caps_path.is_file()
        && let Ok(src) = std::fs::read_to_string(&caps_path)
    {
        let mut ops = BTreeSet::new();
        if let Ok(toml::Value::Table(m)) = toml::from_str::<toml::Value>(&src)
            && let Some(toml::Value::Array(xs)) = m.get("allow")
        {
            for item in xs {
                if let Some(op) = item.as_str()
                    && looks_like_effect_op(op)
                {
                    ops.insert(op.to_string());
                }
            }
        }
        if !ops.is_empty() {
            return ops;
        }
    }
    infer_ops_from_script(script_path)
}

fn infer_ops_from_script(script_path: &Path) -> BTreeSet<String> {
    let Ok(src) = std::fs::read_to_string(script_path) else {
        return BTreeSet::new();
    };
    let mut ops = BTreeSet::new();
    for line in src.lines() {
        for lit in quoted_literals(line) {
            if looks_like_effect_op(&lit) {
                ops.insert(lit);
            }
        }
    }
    ops
}

fn quoted_literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for quote in ['"', '\''] {
        let mut cur = String::new();
        let mut in_quote = false;
        for ch in line.chars() {
            if ch == quote {
                if in_quote {
                    if !cur.is_empty() {
                        out.push(cur.clone());
                    }
                    cur.clear();
                    in_quote = false;
                } else {
                    in_quote = true;
                }
                continue;
            }
            if in_quote {
                cur.push(ch);
            }
        }
    }
    out
}

fn looks_like_effect_op(s: &str) -> bool {
    s.contains("::") && s.contains('/') && !s.contains(char::is_whitespace)
}

fn select_workflows(
    intent: &AgentIntent,
    catalog: &[WorkflowCandidate],
    max_workflows_cli: usize,
    failures: &mut Vec<PlannerFailure>,
) -> Vec<WorkflowCandidate> {
    let goal_tokens: BTreeSet<String> = intent
        .goal
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let exclude: BTreeSet<String> = intent.exclude_workflows.iter().cloned().collect();
    let catalog_by_name: BTreeMap<String, WorkflowCandidate> = catalog
        .iter()
        .cloned()
        .map(|w| (w.name.clone(), w))
        .collect();

    let mut selected = Vec::<WorkflowCandidate>::new();
    let mut selected_names = BTreeSet::<String>::new();
    for name in &intent.required_workflows {
        match catalog_by_name.get(name) {
            Some(wf) if !exclude.contains(name) => {
                selected.push(wf.clone());
                selected_names.insert(name.clone());
            }
            Some(_) => {
                failures.push(planner_failure(
                    "agent-plan/required-workflow-excluded",
                    &format!("required workflow `{name}` is excluded by intent"),
                    vec![
                        "remove workflow from `exclude_workflows` or from `required_workflows`"
                            .to_string(),
                    ],
                    Some(serde_json::json!({ "workflow": name })),
                ));
            }
            None => {
                failures.push(planner_failure(
                    "agent-plan/required-workflow-missing",
                    &format!("required workflow `{name}` was not found in examples/"),
                    vec![
                        "choose a workflow name from `genesis --json agent-index` `reference_workflows`"
                            .to_string(),
                    ],
                    Some(serde_json::json!({ "workflow": name })),
                ));
            }
        }
    }

    let mut scored = Vec::<(i64, WorkflowCandidate)>::new();
    for wf in catalog {
        if exclude.contains(&wf.name) || selected_names.contains(&wf.name) {
            continue;
        }
        let mut score = 0i64;
        for d in &intent.domains {
            if wf.tags.contains(d) {
                score += 10;
            }
        }
        for t in &goal_tokens {
            if wf.tags.contains(t) {
                score += 3;
            }
        }
        if score > 0 {
            scored.push((score, wf.clone()));
        }
    }
    scored.sort_by(|(sa, wa), (sb, wb)| sb.cmp(sa).then(wa.name.cmp(&wb.name)));

    let limit = intent.max_workflows.unwrap_or(max_workflows_cli).max(1);
    for (_, wf) in scored {
        if selected.len() >= limit {
            break;
        }
        selected.push(wf);
    }

    if selected.is_empty() {
        failures.push(planner_failure(
            "agent-plan/no-workflow-match",
            "planner could not map intent to any workflow",
            vec![
                "include at least one `required_workflows` entry".to_string(),
                "or add explicit `domains` matching available agent workflows".to_string(),
            ],
            Some(serde_json::json!({
                "available_workflows": catalog.iter().map(|w| w.name.clone()).collect::<Vec<_>>(),
            })),
        ));
    }
    selected
}

fn policy_precheck(caps_path: &Path, required_ops: &BTreeSet<String>) -> PolicyPrecheck {
    let caps_path_s = caps_path.display().to_string();
    match CapsPolicy::load(caps_path) {
        Ok(pol) => {
            let denied_ops: Vec<String> = required_ops
                .iter()
                .filter(|op| !pol.is_allowed(op))
                .cloned()
                .collect();
            PolicyPrecheck {
                caps_path: caps_path_s,
                checked: true,
                ok: denied_ops.is_empty(),
                denied_ops,
                error: None,
            }
        }
        Err(e) => PolicyPrecheck {
            caps_path: caps_path_s,
            checked: true,
            ok: false,
            denied_ops: Vec::new(),
            error: Some(e.to_string()),
        },
    }
}

fn build_plan_nodes(selected: &[WorkflowCandidate]) -> Vec<serde_json::Value> {
    selected
        .iter()
        .map(|wf| {
            serde_json::json!({
                "id": format!("wf/{}", wf.name),
                "workflow": wf.name,
                "path": wf.rel_dir,
                "script": wf.rel_script,
                "tags": wf.tags.iter().cloned().collect::<Vec<_>>(),
                "required_ops": wf.required_ops.iter().cloned().collect::<Vec<_>>(),
                "script_hash_blake3": wf.script_hash_blake3,
                "ready": wf.script_exists,
            })
        })
        .collect()
}

fn build_plan_edges(selected: &[WorkflowCandidate]) -> Vec<serde_json::Value> {
    let mut edges = Vec::new();
    for pair in selected.windows(2) {
        edges.push(serde_json::json!({
            "from": format!("wf/{}", pair[0].name),
            "to": format!("wf/{}", pair[1].name),
            "kind": "sequential",
        }));
    }
    edges
}

fn build_execution_steps(selected: &[WorkflowCandidate]) -> Vec<serde_json::Value> {
    selected
        .iter()
        .enumerate()
        .map(|(idx, wf)| {
            serde_json::json!({
                "id": format!("step-{:03}", idx + 1),
                "workflow": wf.name,
                "cmd": ["bash", wf.rel_script.clone()],
                "cwd": ".",
                "effect_log_op": format!("agent-plan/execute/{}", wf.name),
            })
        })
        .collect()
}

fn catalog_hash_blake3(catalog: &[WorkflowCandidate]) -> Result<String, CliError> {
    let entries: Vec<serde_json::Value> = catalog
        .iter()
        .map(|wf| {
            serde_json::json!({
                "name": wf.name,
                "script_hash_blake3": wf.script_hash_blake3,
                "required_ops": wf.required_ops.iter().cloned().collect::<Vec<_>>(),
                "tags": wf.tags.iter().cloned().collect::<Vec<_>>(),
            })
        })
        .collect();
    let payload = serde_json::to_value(entries).map_err(|e| {
        cli_err(
            EX_INTERNAL,
            "json/serialize",
            format!("failed to serialize workflow catalog: {e}"),
        )
    })?;
    Ok(blake3::hash(json_canonical_string(&payload).as_bytes())
        .to_hex()
        .to_string())
}
