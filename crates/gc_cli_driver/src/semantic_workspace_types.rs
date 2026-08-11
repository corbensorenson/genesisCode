use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PathStep {
    Form(usize),
    PairCar,
    PairCdr,
}

#[derive(Clone, Debug)]
pub(super) struct SymbolOccurrence {
    pub(super) symbol: String,
}

#[derive(Clone, Debug)]
pub(super) struct DefinitionSite {
    pub(super) symbol: String,
    pub(super) symbol_path_repr: String,
    pub(super) node_id: Option<String>,
    pub(super) term_hash: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct ModuleAnalysis {
    pub(super) module_path: String,
    pub(super) forms: Vec<Term>,
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
