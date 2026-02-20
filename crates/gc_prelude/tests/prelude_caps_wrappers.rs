use gc_coreform::{canonicalize_module, parse_module};
use gc_kernel::{EffectProgram, EffectRequest, EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn get_req(v: Value) -> EffectRequest {
    let Value::EffectProgram(p) = v else {
        panic!("expected effect program, got {}", v.debug_repr());
    };
    let EffectProgram::Perform { request } = p.as_ref() else {
        panic!("expected perform");
    };
    let Value::Sealed { payload, .. } = request.as_ref() else {
        panic!("expected sealed request");
    };
    let Value::EffectRequest(req) = payload.as_ref() else {
        panic!("expected effect request payload");
    };
    req.clone()
}

#[test]
fn prelude_capability_wrappers_construct_expected_requests() {
    let src = r#"
      {
        :store_get (core/store::get "abc")
        :refs_set (core/refs::set "refs/heads/main" "h" "p")
        :refs_set_cas (core/refs::set-cas "refs/heads/main" "h" "p" nil)
        :vcs_log (core/vcs::log "refs/heads/main" 10)
        :pkg_init (core/pkg::init "genesis.lock" "ws" nil nil)
        :gc_plan (core/gc::plan "genesis.lock" ".genesis/pins.toml" 200 true true)
        :task_scope (core/task::scope "agent/rewrite")
        :task_spawn (((core/task::spawn "agent/rewrite") "compile") {:module "m.gc"})
        :task_status (core/task::status "task-1")
        :task_await (core/task::await "task-1")
        :task_cancel (core/task::cancel "task-1")
        :task_await_all (core/task::await-all ["task-1" "task-2"])
        :task_all (core/task::all ["task-1" "task-2"])
        :task_race (core/task::race ["task-1" "task-2"])
        :task_map_bounded
          (((((core/task::map-bounded "agent/rewrite") "compile") ["a" "b"]) 2)
            (fn (x) {:module x}))
        :gfx_submit (core/gfx/gpu::submit-frame-graph {:render-passes [] :compute-passes []})
        :gfx_resize (((core/gfx/window::resize-surface "main-surface") 1280) 720)
        :editor_clip_set ((core/editor/clipboard::set "text/plain") "hello")
        :editor_task_spawn (((core/editor/task::spawn 'editor/task::lint) {:path "a.gc"}) 50)
        :editor_watch_sub ((core/editor/watch::subscribe "/ws") ["*.gc" "*.gcpkg"])
        :editor_lint_panel_from_acceptance
          (core/editor/action::lint-panel-from-acceptance
            {
              :obligations [
                {
                  :name core/obligation::lint
                  :artifact "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                }
              ]
            })
        :editor_vcs_refs_panel (core/editor/action::vcs-refs-panel "refs/heads/")
        :editor_vcs_log_panel ((core/editor/action::vcs-log-panel "refs/heads/main") 10)
        :editor_vcs_diff_panel ((core/editor/action::vcs-diff-panel "base-h") "to-h")
        :editor_vcs_apply_panel ((core/editor/action::vcs-apply-panel "base-h") "patch-h")
        :editor_vcs_merge3_panel (((core/editor/action::vcs-merge3-panel "base-h") "left-h") "right-h")
        :editor_vcs_resolve_conflict_panel (core/editor/action::vcs-resolve-conflict-panel "conflict-h")
        :editor_vcs_resolve_conflict_with_panel
          (((core/editor/action::vcs-resolve-conflict-with-panel "conflict-h") nil) nil)
        :editor_vcs_conflict_panel (core/editor/action::vcs-conflict-panel "conflict-h")
        :editor_vcs_commit_panel (core/editor/action::vcs-commit-panel "commit-h")
        :editor_vcs_evidence_panel (core/editor/action::vcs-evidence-panel "evidence-h")
        :editor_vcs_evidence_list_panel (core/editor/action::vcs-evidence-list-panel "commit-h")
        :editor_vcs_blame_panel ((core/editor/action::vcs-blame-panel "snapshot-h") "pkg/mod::x")
        :editor_vcs_blame_panel_with_path
          (((core/editor/action::vcs-blame-panel-with-path "snapshot-h") "pkg/mod::x") "[:defs pkg/mod::x]")
        :editor_vcs_why_panel ((core/editor/action::vcs-why-panel "snapshot-h") "pkg/mod::x")
        :editor_vcs_why_panel_with_op
          (((core/editor/action::vcs-why-panel-with-op "snapshot-h") "pkg/mod::x") "pkg/mod::op")
        :editor_format_file_task (core/editor/action::format-file-task "a.gc")
        :editor_parse_file_task (core/editor/action::parse-file-task "a.gc")
        :editor_lint_module_task ((core/editor/action::lint-module-task "a.gc") [pkg/a::x])
        :editor_lint_module_task_from_sources
          (((core/editor/action::lint-module-task-from-sources "a.gc")
            "(def ::meta (quote {:exports [pkg/a::x] :types {pkg/a::x ?}})) (def pkg/a::x 1)")
            "(def ::meta (quote {:exports [pkg/a::y] :types {pkg/a::y ?}})) (def pkg/a::y 2)")
        :editor_plugin_perform_allowed
          (((core/editor/plugin::perform {:allow [core/store::get] :deny []}) (quote core/store::get))
            {:hash "abc"})
        :editor_agent_store_session_log
          (core/editor/agent::store-session-log (core/editor/agent::session-empty "agent-1"))
        :editor_agent_propose_patch ((core/editor/action::agent-propose-patch "base-h") "to-h")
        :editor_agent_apply_patch_with_obligations
          ((((core/editor/action::agent-apply-patch-with-obligations "base-h") "patch-h")
            "genesis.lock")
            false)
        :editor_typecheck_pkg_task (core/editor/action::typecheck-pkg-task "package.toml")
        :editor_optimize_module_task ((core/editor/action::optimize-module-task "a.gc") "a.opt.gc")
        :editor_test_pkg_task ((core/editor/action::test-pkg-task "package.toml") "caps.toml")
        :editor_pkg_list_panel (core/editor/action::pkg-list-panel "genesis.lock")
        :editor_pkg_info_panel ((core/editor/action::pkg-info-panel "genesis.lock") "my-lib")
        :editor_pkg_lock_panel (core/editor/action::pkg-lock-panel "genesis.lock")
        :editor_pkg_update_panel (core/editor/action::pkg-update-panel "genesis.lock")
        :editor_pkg_install_panel (((core/editor/action::pkg-install-panel "genesis.lock") true) false)
        :editor_pkg_verify_panel ((core/editor/action::pkg-verify-panel "genesis.lock") false)
        :editor_pkg_snapshot_panel (core/editor/action::pkg-snapshot-panel "package.toml")
        :editor_gpk_export_panel (((((core/editor/action::gpk-export-panel "root-h") "pkg.gpk") (quote :shallow)) 0) [])
        :editor_gpk_import_panel (core/editor/action::gpk-import-panel "pkg.gpk")
        :editor_sync_pull_panel ((((((core/editor/action::sync-pull-panel "origin") []) []) 0) false) false)
        :editor_sync_push_panel ((((core/editor/action::sync-push-panel "origin") []) []) 0)
      }
    "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();

    let Value::Map(m) = v else {
        panic!("expected map, got {}", v.debug_repr());
    };

    let store_get = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":store_get",
        )))
        .unwrap()
        .clone();
    let req = get_req(store_get);
    assert_eq!(req.op, "core/store::get");
    assert!(matches!(
        req.payload,
        gc_coreform::Term::Map(ref mm)
            if mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash")))
                == Some(&gc_coreform::Term::Str("abc".to_string()))
    ));

    let refs_set = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":refs_set",
        )))
        .unwrap()
        .clone();
    let req = get_req(refs_set);
    assert_eq!(req.op, "core/refs::set");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert!(
        !mm.contains_key(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":expected-old"
        )))
    );

    let refs_set_cas = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":refs_set_cas",
        )))
        .unwrap()
        .clone();
    let req = get_req(refs_set_cas);
    assert_eq!(req.op, "core/refs::set");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert!(
        mm.contains_key(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":expected-old"
        )))
    );

    let vcs_log = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":vcs_log",
        )))
        .unwrap()
        .clone();
    let req = get_req(vcs_log);
    assert_eq!(req.op, "core/vcs::log");

    let pkg_init = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":pkg_init",
        )))
        .unwrap()
        .clone();
    let req = get_req(pkg_init);
    assert_eq!(req.op, "core/pkg-low::init");

    let gc_plan = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":gc_plan",
        )))
        .unwrap()
        .clone();
    let req = get_req(gc_plan);
    assert_eq!(req.op, "core/gc::plan");

    let task_scope = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task_scope",
        )))
        .unwrap()
        .clone();
    let req = get_req(task_scope);
    assert_eq!(req.op, "core/task::scope");

    let task_spawn = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task_spawn",
        )))
        .unwrap()
        .clone();
    let req = get_req(task_spawn);
    assert_eq!(req.op, "core/task::spawn");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":label"
        ))),
        Some(&gc_coreform::Term::Str("compile".to_string()))
    );

    let task_status = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task_status",
        )))
        .unwrap()
        .clone();
    let req = get_req(task_status);
    assert_eq!(req.op, "core/task::status");

    let task_await = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task_await",
        )))
        .unwrap()
        .clone();
    let req = get_req(task_await);
    assert_eq!(req.op, "core/task::await");

    let task_cancel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task_cancel",
        )))
        .unwrap()
        .clone();
    let req = get_req(task_cancel);
    assert_eq!(req.op, "core/task::cancel");

    let task_await_all = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task_await_all",
        )))
        .unwrap()
        .clone();
    let req = get_req(task_await_all);
    assert_eq!(req.op, "core/task::await");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task-id"
        ))),
        Some(&gc_coreform::Term::Str("task-1".to_string()))
    );

    let task_all = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task_all",
        )))
        .unwrap()
        .clone();
    let req = get_req(task_all);
    assert_eq!(req.op, "core/task::await");

    let task_race = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task_race",
        )))
        .unwrap()
        .clone();
    let req = get_req(task_race);
    assert_eq!(req.op, "core/task::await");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task-id"
        ))),
        Some(&gc_coreform::Term::Str("task-1".to_string()))
    );

    let task_map_bounded = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task_map_bounded",
        )))
        .unwrap()
        .clone();
    let req = get_req(task_map_bounded);
    assert_eq!(req.op, "core/task::spawn");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":scope"
        ))),
        Some(&gc_coreform::Term::Str("agent/rewrite".to_string()))
    );
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":label"
        ))),
        Some(&gc_coreform::Term::Str("compile".to_string()))
    );

    let gfx_submit = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":gfx_submit",
        )))
        .unwrap()
        .clone();
    let req = get_req(gfx_submit);
    assert_eq!(req.op, "gfx/gpu::submit-frame-graph");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert!(
        mm.contains_key(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":graph"
        )))
    );

    let gfx_resize = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":gfx_resize",
        )))
        .unwrap()
        .clone();
    let req = get_req(gfx_resize);
    assert_eq!(req.op, "gfx/window::resize-surface");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":width"
        ))),
        Some(&gc_coreform::Term::Int(1280.into()))
    );
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":height"
        ))),
        Some(&gc_coreform::Term::Int(720.into()))
    );

    let editor_clip_set = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_clip_set",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_clip_set);
    assert_eq!(req.op, "editor/clipboard::set");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":mime"))),
        Some(&gc_coreform::Term::Str("text/plain".to_string()))
    );

    let editor_task_spawn = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_task_spawn",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_task_spawn);
    assert_eq!(req.op, "editor/task::spawn");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task-kind"
        ))),
        Some(&gc_coreform::Term::symbol("editor/task::lint"))
    );

    let editor_watch_sub = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_watch_sub",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_watch_sub);
    assert_eq!(req.op, "editor/watch::subscribe");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    let Some(gc_coreform::Term::Vector(globs)) = mm.get(&gc_coreform::TermOrdKey(
        gc_coreform::Term::symbol(":globs"),
    )) else {
        panic!("missing :globs vector");
    };
    assert_eq!(globs.len(), 2);

    let editor_lint_panel_from_acceptance = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_lint_panel_from_acceptance",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_lint_panel_from_acceptance);
    assert_eq!(req.op, "core/store::get");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":hash"))),
        Some(&gc_coreform::Term::Str(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()
        ))
    );

    let editor_vcs_refs_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_refs_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_refs_panel);
    assert_eq!(req.op, "core/refs::list");

    let editor_vcs_log_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_log_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_log_panel);
    assert_eq!(req.op, "core/vcs::log");

    let editor_vcs_diff_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_diff_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_diff_panel);
    assert_eq!(req.op, "core/vcs::diff");

    let editor_vcs_apply_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_apply_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_apply_panel);
    assert_eq!(req.op, "core/vcs::apply");

    let editor_vcs_merge3_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_merge3_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_merge3_panel);
    assert_eq!(req.op, "core/vcs::merge3");

    let editor_vcs_resolve_conflict_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_resolve_conflict_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_resolve_conflict_panel);
    assert_eq!(req.op, "core/vcs::resolve-conflict");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":conflict"
        ))),
        Some(&gc_coreform::Term::Str("conflict-h".to_string()))
    );

    let editor_vcs_resolve_conflict_with_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_resolve_conflict_with_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_resolve_conflict_with_panel);
    assert_eq!(req.op, "core/vcs::resolve-conflict");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert!(
        mm.contains_key(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":strategy"
        )))
    );
    assert!(
        mm.contains_key(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":resolutions"
        )))
    );

    let editor_vcs_conflict_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_conflict_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_conflict_panel);
    assert_eq!(req.op, "core/store::get");

    let editor_vcs_commit_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_commit_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_commit_panel);
    assert_eq!(req.op, "core/store::get");

    let editor_vcs_evidence_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_evidence_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_evidence_panel);
    assert_eq!(req.op, "core/store::get");

    let editor_vcs_evidence_list_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_evidence_list_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_evidence_list_panel);
    assert_eq!(req.op, "core/store::get");

    let editor_vcs_blame_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_blame_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_blame_panel);
    assert_eq!(req.op, "core/vcs::blame");

    let editor_vcs_blame_panel_with_path = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_blame_panel_with_path",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_blame_panel_with_path);
    assert_eq!(req.op, "core/vcs::blame");

    let editor_vcs_why_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_why_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_why_panel);
    assert_eq!(req.op, "core/vcs::why");

    let editor_vcs_why_panel_with_op = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_vcs_why_panel_with_op",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_vcs_why_panel_with_op);
    assert_eq!(req.op, "core/vcs::why");

    let editor_format_file_task = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_format_file_task",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_format_file_task);
    assert_eq!(req.op, "editor/task::spawn");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task-kind"
        ))),
        Some(&gc_coreform::Term::symbol("editor/task::fmt-coreform"))
    );

    let editor_parse_file_task = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_parse_file_task",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_parse_file_task);
    assert_eq!(req.op, "editor/task::spawn");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    assert_eq!(
        mm.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":task-kind"
        ))),
        Some(&gc_coreform::Term::symbol("editor/task::parse-module"))
    );

    let editor_lint_module_task = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_lint_module_task",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_lint_module_task);
    assert_eq!(req.op, "editor/task::spawn");

    let editor_lint_module_task_from_sources = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_lint_module_task_from_sources",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_lint_module_task_from_sources);
    assert_eq!(req.op, "editor/task::spawn");
    let gc_coreform::Term::Map(mm) = req.payload else {
        panic!("expected map payload");
    };
    let Some(gc_coreform::Term::Map(input)) = mm.get(&gc_coreform::TermOrdKey(
        gc_coreform::Term::symbol(":input"),
    )) else {
        panic!("spawn input map expected");
    };
    let Some(gc_coreform::Term::Vector(changed)) = input.get(&gc_coreform::TermOrdKey(
        gc_coreform::Term::symbol(":changed-syms"),
    )) else {
        panic!("changed-syms vector expected");
    };
    assert!(
        changed
            .iter()
            .any(|t| matches!(t, gc_coreform::Term::Symbol(s) if s == "pkg/a::x"))
    );
    assert!(
        changed
            .iter()
            .any(|t| matches!(t, gc_coreform::Term::Symbol(s) if s == "pkg/a::y"))
    );
    assert!(
        changed
            .iter()
            .any(|t| matches!(t, gc_coreform::Term::Symbol(s) if s == "::meta"))
    );

    let editor_plugin_perform_allowed = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_plugin_perform_allowed",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_plugin_perform_allowed);
    assert_eq!(req.op, "core/store::get");

    let editor_agent_store_session_log = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_agent_store_session_log",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_agent_store_session_log);
    assert_eq!(req.op, "core/store::put");

    let editor_agent_propose_patch = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_agent_propose_patch",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_agent_propose_patch);
    assert_eq!(req.op, "core/vcs::diff");

    let editor_agent_apply_patch_with_obligations = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_agent_apply_patch_with_obligations",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_agent_apply_patch_with_obligations);
    assert_eq!(req.op, "core/vcs::apply");

    let editor_typecheck_pkg_task = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_typecheck_pkg_task",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_typecheck_pkg_task);
    assert_eq!(req.op, "editor/task::spawn");

    let editor_optimize_module_task = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_optimize_module_task",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_optimize_module_task);
    assert_eq!(req.op, "editor/task::spawn");

    let editor_test_pkg_task = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_test_pkg_task",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_test_pkg_task);
    assert_eq!(req.op, "editor/task::spawn");

    let editor_pkg_list_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_pkg_list_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_pkg_list_panel);
    assert_eq!(req.op, "core/pkg-low::list");

    let editor_pkg_info_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_pkg_info_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_pkg_info_panel);
    assert_eq!(req.op, "core/pkg-low::info");

    let editor_pkg_lock_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_pkg_lock_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_pkg_lock_panel);
    assert_eq!(req.op, "core/pkg-low::lock");

    let editor_pkg_update_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_pkg_update_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_pkg_update_panel);
    assert_eq!(req.op, "core/pkg-low::update");

    let editor_pkg_install_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_pkg_install_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_pkg_install_panel);
    assert_eq!(req.op, "core/pkg-low::install");

    let editor_pkg_verify_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_pkg_verify_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_pkg_verify_panel);
    assert_eq!(req.op, "core/pkg-low::verify");

    let editor_pkg_snapshot_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_pkg_snapshot_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_pkg_snapshot_panel);
    assert_eq!(req.op, "core/pkg-low::snapshot");

    let editor_gpk_export_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_gpk_export_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_gpk_export_panel);
    assert_eq!(req.op, "core/gpk::export");

    let editor_gpk_import_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_gpk_import_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_gpk_import_panel);
    assert_eq!(req.op, "core/gpk::import");

    let editor_sync_pull_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_sync_pull_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_sync_pull_panel);
    assert_eq!(req.op, "core/sync::pull");

    let editor_sync_push_panel = m
        .get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(
            ":editor_sync_push_panel",
        )))
        .unwrap()
        .clone();
    let req = get_req(editor_sync_push_panel);
    assert_eq!(req.op, "core/sync::push");
}
