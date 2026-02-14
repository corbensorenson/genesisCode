mod parse;
mod print;
mod term;
mod canon;

pub use canon::{canonicalize_form, canonicalize_module};
pub use parse::{parse_module, parse_term, ParseError};
pub use print::{print_module, print_term};
pub use term::{hash_module, hash_term, Term, TermOrdKey};

#[cfg(test)]
mod tests;
