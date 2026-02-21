use gc_coreform::{Term, TermOrdKey};
use gc_kernel::Value;
use num_traits::ToPrimitive;

pub(super) fn task_map<const N: usize>(pairs: [(&str, Term); N]) -> Term {
    Term::Map(
        pairs
            .into_iter()
            .map(|(k, v)| (TermOrdKey(Term::symbol(k)), v))
            .collect(),
    )
}

pub(super) fn map_field<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else {
        return None;
    };
    m.get(&TermOrdKey(Term::symbol(key)))
}

pub(super) fn map_field_str_or_symbol(t: &Term, key: &str) -> Option<String> {
    match map_field(t, key) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(super) fn map_field_int_u64(t: &Term, key: &str) -> Option<u64> {
    match map_field(t, key) {
        Some(Term::Int(i)) if i.sign() != num_bigint::Sign::Minus => i.to_u64(),
        _ => None,
    }
}

pub(super) fn value_data_map_field(v: &Value, key: &str) -> Option<String> {
    let Value::Data(Term::Map(m)) = v else {
        return None;
    };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}
