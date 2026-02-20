use std::collections::BTreeMap;

use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::{Term, TermOrdKey};

pub const FIXED_DEC_KIND: &str = ":fixed-decimal";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixedDecimal {
    unscaled: BigInt,
    scale: u32,
}

impl FixedDecimal {
    pub fn from_unscaled(unscaled: BigInt, scale: u32) -> Self {
        normalize(unscaled, scale)
    }

    pub fn from_int(i: BigInt) -> Self {
        Self {
            unscaled: i,
            scale: 0,
        }
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        if input.is_empty() {
            return Err("decimal parse: empty input".to_string());
        }
        let mut s = input;
        let negative = if let Some(rest) = s.strip_prefix('-') {
            s = rest;
            true
        } else if let Some(rest) = s.strip_prefix('+') {
            s = rest;
            false
        } else {
            false
        };
        if s.is_empty() {
            return Err("decimal parse: missing digits".to_string());
        }

        let (whole, frac, scale) = if let Some((w, f)) = s.split_once('.') {
            if w.is_empty() || f.is_empty() {
                return Err("decimal parse: expected digits on both sides of '.'".to_string());
            }
            if !is_ascii_digits(w) || !is_ascii_digits(f) {
                return Err("decimal parse: invalid digit".to_string());
            }
            let scale: u32 = f
                .len()
                .try_into()
                .map_err(|_| "decimal parse: too many fractional digits".to_string())?;
            (w, f, scale)
        } else {
            if !is_ascii_digits(s) {
                return Err("decimal parse: invalid digit".to_string());
            }
            (s, "", 0)
        };

        let mut digits = String::with_capacity(whole.len() + frac.len());
        digits.push_str(whole);
        digits.push_str(frac);
        let mut unscaled = digits
            .parse::<BigInt>()
            .map_err(|_| "decimal parse: invalid integer digits".to_string())?;
        if negative {
            unscaled = -unscaled;
        }
        Ok(normalize(unscaled, scale))
    }

    pub fn to_canonical_string(&self) -> String {
        if self.scale == 0 {
            return self.unscaled.to_string();
        }
        let negative = self.unscaled.is_negative();
        let abs = self.unscaled.abs();
        let mut digits = abs.to_string();
        let scale = self.scale as usize;
        if digits.len() <= scale {
            let zeros_needed = scale.saturating_sub(digits.len()).saturating_add(1);
            let mut padded = String::with_capacity(zeros_needed + digits.len());
            for _ in 0..zeros_needed {
                padded.push('0');
            }
            padded.push_str(&digits);
            digits = padded;
        }
        let split = digits.len().saturating_sub(scale);
        let whole = &digits[..split];
        let frac = &digits[split..];
        let mut out = String::new();
        if negative {
            out.push('-');
        }
        out.push_str(whole);
        out.push('.');
        out.push_str(frac);
        out
    }

    pub fn to_term(&self) -> Term {
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":num/kind")),
            Term::Symbol(FIXED_DEC_KIND.to_string()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":num/unscaled")),
            Term::Int(self.unscaled.clone()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":num/scale")),
            Term::Int(BigInt::from(self.scale)),
        );
        Term::Map(m)
    }

    pub fn from_term(t: &Term) -> Option<Self> {
        let Term::Map(m) = t else {
            return None;
        };
        match m.get(&TermOrdKey(Term::symbol(":num/kind"))) {
            Some(Term::Symbol(s)) if s == FIXED_DEC_KIND => {}
            _ => return None,
        }
        let unscaled = match m.get(&TermOrdKey(Term::symbol(":num/unscaled"))) {
            Some(Term::Int(i)) => i.clone(),
            _ => return None,
        };
        let scale = match m.get(&TermOrdKey(Term::symbol(":num/scale"))) {
            Some(Term::Int(i)) if i.sign() != num_bigint::Sign::Minus => i.to_u32()?,
            _ => return None,
        };
        Some(normalize(unscaled, scale))
    }

    pub fn add(&self, rhs: &Self) -> Self {
        let scale = self.scale.max(rhs.scale);
        let a = self.unscaled.clone() * ten_pow(scale.saturating_sub(self.scale));
        let b = rhs.unscaled.clone() * ten_pow(scale.saturating_sub(rhs.scale));
        normalize(a + b, scale)
    }

    pub fn sub(&self, rhs: &Self) -> Self {
        let scale = self.scale.max(rhs.scale);
        let a = self.unscaled.clone() * ten_pow(scale.saturating_sub(self.scale));
        let b = rhs.unscaled.clone() * ten_pow(scale.saturating_sub(rhs.scale));
        normalize(a - b, scale)
    }

    pub fn mul(&self, rhs: &Self) -> Option<Self> {
        let scale = self.scale.checked_add(rhs.scale)?;
        Some(normalize(
            self.unscaled.clone() * rhs.unscaled.clone(),
            scale,
        ))
    }

    pub fn lt(&self, rhs: &Self) -> bool {
        self < rhs
    }
}

impl PartialOrd for FixedDecimal {
    fn partial_cmp(&self, rhs: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(rhs))
    }
}

impl Ord for FixedDecimal {
    fn cmp(&self, rhs: &Self) -> std::cmp::Ordering {
        compare_scaled(self, rhs)
    }
}

fn compare_scaled(lhs: &FixedDecimal, rhs: &FixedDecimal) -> std::cmp::Ordering {
    let scale = lhs.scale.max(rhs.scale);
    let a = lhs.unscaled.clone() * ten_pow(scale.saturating_sub(lhs.scale));
    let b = rhs.unscaled.clone() * ten_pow(scale.saturating_sub(rhs.scale));
    a.cmp(&b)
}

fn normalize(mut unscaled: BigInt, mut scale: u32) -> FixedDecimal {
    if unscaled.is_zero() {
        return FixedDecimal { unscaled, scale: 0 };
    }
    while scale > 0 {
        let rem = &unscaled % 10u8;
        if !rem.is_zero() {
            break;
        }
        unscaled /= 10u8;
        scale = scale.saturating_sub(1);
    }
    FixedDecimal { unscaled, scale }
}

fn ten_pow(exp: u32) -> BigInt {
    let mut out = BigInt::from(1u8);
    for _ in 0..exp {
        out *= 10u8;
    }
    out
}

fn is_ascii_digits(s: &str) -> bool {
    !s.is_empty() && s.as_bytes().iter().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::FixedDecimal;

    #[test]
    fn parse_and_canonical_string_normalize_trailing_zeros() {
        let d = FixedDecimal::parse("001.2300").expect("parse");
        assert_eq!(d.to_canonical_string(), "1.23");
    }

    #[test]
    fn arithmetic_is_deterministic() {
        let a = FixedDecimal::parse("1.20").expect("a");
        let b = FixedDecimal::parse("2.345").expect("b");
        let c = a.add(&b);
        assert_eq!(c.to_canonical_string(), "3.545");
        let d = c.sub(&FixedDecimal::parse("0.545").expect("d"));
        assert_eq!(d.to_canonical_string(), "3");
        let e = d
            .mul(&FixedDecimal::parse("2.50").expect("e"))
            .expect("mul");
        assert_eq!(e.to_canonical_string(), "7.5");
    }

    #[test]
    fn term_roundtrip_stable() {
        let d = FixedDecimal::parse("-10.500").expect("parse");
        let t = d.to_term();
        let d2 = FixedDecimal::from_term(&t).expect("decode");
        assert_eq!(d2.to_canonical_string(), "-10.5");
        assert_eq!(d, d2);
    }
}
