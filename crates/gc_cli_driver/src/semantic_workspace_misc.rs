use super::*;

pub(super) fn refactor_kind_symbol(kind: RefactorKind) -> &'static str {
    match kind {
        RefactorKind::Rename => ":rename",
        RefactorKind::Move => ":move",
        RefactorKind::Extract => ":extract",
    }
}

pub(super) fn refactor_kind_token(kind: RefactorKind) -> &'static str {
    match kind {
        RefactorKind::Rename => "rename",
        RefactorKind::Move => "move",
        RefactorKind::Extract => "extract",
    }
}

pub(super) fn term_tag(term: &Term) -> &'static str {
    match term {
        Term::Nil => "nil",
        Term::Bool(_) => "bool",
        Term::Int(_) => "int",
        Term::Str(_) => "str",
        Term::Bytes(_) => "bytes",
        Term::Symbol(_) => "sym",
        Term::Pair(_, _) => "pair",
        Term::Vector(_) => "vec",
        Term::Map(_) => "map",
    }
}
