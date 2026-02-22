use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PathStep {
    Form(usize),
    PairCar,
    PairCdr,
    Vec(usize),
    Map(Term),
}

#[derive(Clone, Debug)]
pub(super) struct SymbolOccurrence {
    pub(super) module_path: String,
    pub(super) symbol: String,
    pub(super) path: Vec<PathStep>,
    pub(super) path_repr: String,
}

#[derive(Clone, Debug)]
pub(super) struct DefinitionSite {
    pub(super) module_path: String,
    pub(super) symbol: String,
    pub(super) expr: Term,
    pub(super) form_path: Vec<PathStep>,
    pub(super) form_path_repr: String,
    pub(super) symbol_path_repr: String,
    pub(super) node_id: Option<String>,
    pub(super) term_hash: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct ModuleAnalysis {
    pub(super) module_path: String,
    pub(super) defs: BTreeMap<String, DefinitionSite>,
    pub(super) occurrences: Vec<SymbolOccurrence>,
    pub(super) node_count: usize,
}

#[derive(Clone, Debug)]
pub(super) struct WorkspaceAnalysis {
    pub(super) pkg_dir: PathBuf,
    pub(super) modules: Vec<ModuleAnalysis>,
    pub(super) owners: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub(super) struct RefactorConflict {
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) module_path: Option<String>,
    pub(super) path_repr: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) enum PlannedOp {
    AddModule {
        module_path: String,
        forms: Vec<Term>,
    },
    ReplaceNode {
        module_path: String,
        path: Vec<PathStep>,
        path_repr: String,
        new_term: Term,
    },
}
