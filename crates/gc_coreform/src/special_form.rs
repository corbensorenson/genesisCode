/// Closed inventory of v0.2 executable special-form heads.
///
/// Data terms and general application are represented separately because they
/// are not selected by a reserved head symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialForm {
    Quote,
    Def,
    Fn,
    If,
    Begin,
    Let,
    Prim,
    Seal,
    Unseal,
}

impl SpecialForm {
    pub const ALL: [Self; 9] = [
        Self::Quote,
        Self::Def,
        Self::Fn,
        Self::If,
        Self::Begin,
        Self::Let,
        Self::Prim,
        Self::Seal,
        Self::Unseal,
    ];

    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Quote => "quote",
            Self::Def => "def",
            Self::Fn => "fn",
            Self::If => "if",
            Self::Begin => "begin",
            Self::Let => "let",
            Self::Prim => "prim",
            Self::Seal => "seal",
            Self::Unseal => "unseal",
        }
    }

    pub fn from_symbol(symbol: &str) -> Option<Self> {
        match symbol {
            "quote" => Some(Self::Quote),
            "def" => Some(Self::Def),
            "fn" => Some(Self::Fn),
            "if" => Some(Self::If),
            "begin" => Some(Self::Begin),
            "let" => Some(Self::Let),
            "prim" => Some(Self::Prim),
            "seal" => Some(Self::Seal),
            "unseal" => Some(Self::Unseal),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SpecialForm;

    #[test]
    fn inventory_roundtrips_without_aliases() {
        let mut symbols = std::collections::BTreeSet::new();
        for form in SpecialForm::ALL {
            assert_eq!(SpecialForm::from_symbol(form.symbol()), Some(form));
            assert!(symbols.insert(form.symbol()));
        }
        assert_eq!(SpecialForm::from_symbol("application"), None);
    }
}
