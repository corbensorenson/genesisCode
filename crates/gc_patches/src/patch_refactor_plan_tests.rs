use super::*;

fn conflict_report(request_hash: String) -> Term {
    let conflict = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":code")),
                Term::Str("refactor/source-symbol-missing".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":message")),
                Term::Str("source symbol has no workspace definition".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":module-path")),
                Term::Str(String::new()),
            ),
            (
                TermOrdKey(Term::symbol(":path-repr")),
                Term::Str(String::new()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":conflicts")),
                Term::Vector(vec![conflict]),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(REPORT_KIND.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":module-count")),
                Term::Int(1.into()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(false)),
            (TermOrdKey(Term::symbol(":op-count")), Term::Int(0.into())),
            (
                TermOrdKey(Term::symbol(":op-identities")),
                Term::Vector(Vec::new()),
            ),
            (TermOrdKey(Term::symbol(":patch")), Term::Nil),
            (
                TermOrdKey(Term::symbol(":patch-h")),
                Term::Str(String::new()),
            ),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(PROFILE.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":replacement-count")),
                Term::Int(0.into()),
            ),
            (
                TermOrdKey(Term::symbol(":request-h")),
                Term::Str(request_hash),
            ),
            (
                TermOrdKey(Term::symbol(":safe-to-apply")),
                Term::Bool(false),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn fixture() -> (Term, Vec<SemanticRefactorModule>) {
    let modules = vec![SemanticRefactorModule {
        module_path: "a.gc".to_string(),
        forms: Vec::new(),
    }];
    let request = request_term("rename", "pkg::missing", "pkg::next", "", &modules);
    (request, modules)
}

fn topology_fixture(to_symbol: &str) -> (Patch, Vec<SemanticRefactorModule>) {
    let modules = vec![SemanticRefactorModule {
        module_path: "a.gc".to_string(),
        forms: gc_coreform::parse_module("(def pkg::old 1)").unwrap(),
    }];
    let patch = Patch {
        version: 1,
        intent: "semantic-refactor/rename".to_string(),
        provenance: Term::Nil,
        ops: vec![PatchOp::RenameSymbol {
            module_path: "a.gc".to_string(),
            from: "pkg::old".to_string(),
            to: to_symbol.to_string(),
        }],
        normalized_term: Term::Nil,
        semantic_hash: String::new(),
        source_hash: String::new(),
        op_hashes: Vec::new(),
    };
    (patch, modules)
}

#[test]
fn topology_uses_explicit_refactor_kind() {
    let (patch, modules) = topology_fixture("pkg::next");
    verify_operation_topology(
        &patch,
        "rename",
        "pkg::old",
        "pkg::next",
        &modules,
        "ignored-for-rename.gc",
        1,
    )
    .unwrap();
}

#[test]
fn topology_rejects_tampered_rename_operands() {
    let (patch, modules) = topology_fixture("pkg::attacker");
    let error =
        verify_operation_topology(&patch, "rename", "pkg::old", "pkg::next", &modules, "", 1)
            .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("invalid, duplicate, or unordered")
    );
}

#[test]
fn topology_rejects_split_from_non_definition_module() {
    let modules = vec![
        SemanticRefactorModule {
            module_path: "a.gc".to_string(),
            forms: gc_coreform::parse_module("(def pkg::old 1)").unwrap(),
        },
        SemanticRefactorModule {
            module_path: "b.gc".to_string(),
            forms: Vec::new(),
        },
    ];
    let patch = Patch {
        version: 1,
        intent: "semantic-refactor/move".to_string(),
        provenance: Term::Nil,
        ops: vec![
            PatchOp::RenameSymbol {
                module_path: "a.gc".to_string(),
                from: "pkg::old".to_string(),
                to: "pkg::next".to_string(),
            },
            PatchOp::SplitModule {
                from_module_path: "b.gc".to_string(),
                to_module_path: "moved.gc".to_string(),
                symbols: vec!["pkg::next".to_string()],
            },
            PatchOp::UpdateManifest {
                set: Some(expected_manifest_set(&modules, "b.gc", "moved.gc")),
                obligations_add: Vec::new(),
                obligations_remove: Vec::new(),
                tests_add: Vec::new(),
                tests_remove: Vec::new(),
                caps_policy: None,
            },
        ],
        normalized_term: Term::Nil,
        semantic_hash: String::new(),
        source_hash: String::new(),
        op_hashes: Vec::new(),
    };
    let error = verify_operation_topology(
        &patch,
        "move",
        "pkg::old",
        "pkg::next",
        &modules,
        "moved.gc",
        1,
    )
    .unwrap_err();
    assert!(error.to_string().contains("split-module topology mismatch"));
}

#[test]
fn decoder_rejects_tampered_request_identity() {
    let (request, modules) = fixture();
    let error = decode_report(
        conflict_report("0".repeat(64)),
        &request,
        "rename",
        "pkg::missing",
        "pkg::next",
        &modules,
        "",
    )
    .unwrap_err();
    assert!(error.to_string().contains("authority identity mismatch"));
}

#[test]
fn decoder_rejects_patch_authority_on_conflicted_report() {
    let (request, modules) = fixture();
    let mut report = conflict_report(hash32_hex(hash_term(&request)));
    let map = match &mut report {
        Term::Map(map) => map,
        _ => {
            assert!(false, "test fixture report must be a map");
            return;
        }
    };
    map.insert(
        TermOrdKey(Term::symbol(":patch")),
        Term::Map(BTreeMap::new()),
    );
    let error = decode_report(
        report,
        &request,
        "rename",
        "pkg::missing",
        "pkg::next",
        &modules,
        "",
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("conflicted report carries patch authority")
    );
}
