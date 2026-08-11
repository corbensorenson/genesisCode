use super::*;

pub(super) fn refactor_kind_token(kind: RefactorKind) -> &'static str {
    match kind {
        RefactorKind::Rename => "rename",
        RefactorKind::Move => "move",
        RefactorKind::Extract => "extract",
    }
}
