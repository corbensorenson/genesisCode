mod canon;
mod fixed_decimal;
mod parse;
mod print;
mod term;

pub use canon::{canonicalize_form, canonicalize_module};
pub use fixed_decimal::{FIXED_DEC_KIND, FixedDecimal};
pub use parse::{ParseError, parse_module, parse_term};
pub use print::{print_module, print_term, print_term_compact};
pub use term::{
    COREFORM_PROFILE_ID, HASH_DOMAIN_PREFIX, HASH_PROFILE_ID, LANGUAGE_PROFILE_ID, Term,
    TermOrdKey, hash_module, hash_term,
};

#[cfg(test)]
mod tests;
