//! Explicit, bounded structural identity for runtime terms, including improper pairs.
//! This staged profile is separate from canonical store identity and legacy value hashing.

use blake3::Hasher;
use gc_coreform::{Term, TermOrdKey};
use num_bigint::Sign;

use crate::{KernelError, KernelErrorKind};

pub const RUNTIME_TERM_HASH_PROFILE_ID: &str = "genesis/runtime-term-hash/v0.3";
const DOMAIN: &[u8] = b"GCvalue-v0.3\0term\0";

/// Explicit per-operation ceilings; no sentinel or ambient default disables a bound.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeTermHashLimits {
    pub max_nodes: u64,
    pub max_encoded_bytes: u64,
    pub max_pending_frames: usize,
}

enum Frame<'a> {
    Term(&'a Term),
    Vector(std::slice::Iter<'a, Term>),
    Map(std::collections::btree_map::Iter<'a, TermOrdKey, Term>),
}

struct Encoder {
    hasher: Hasher,
    limits: RuntimeTermHashLimits,
    nodes: u64,
    bytes: u64,
}

impl Encoder {
    fn charge_bytes(&mut self, additional: u64) -> Result<(), KernelError> {
        self.bytes = self.bytes.checked_add(additional).ok_or_else(|| {
            KernelError::new(
                KernelErrorKind::MemoryLimit,
                "runtime hash byte counter overflow",
            )
        })?;
        if self.bytes > self.limits.max_encoded_bytes {
            return Err(KernelError::memory_limit(
                "runtime-hash-bytes",
                self.bytes,
                self.limits.max_encoded_bytes,
            ));
        }
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), KernelError> {
        self.charge_bytes(bytes.len() as u64)?;
        self.hasher.update(bytes);
        Ok(())
    }

    fn count(&mut self, count: usize) -> Result<(), KernelError> {
        self.write(&(count as u64).to_le_bytes())
    }

    fn blob(&mut self, bytes: &[u8]) -> Result<(), KernelError> {
        self.count(bytes.len())?;
        self.write(bytes)
    }

    fn node(&mut self) -> Result<(), KernelError> {
        self.nodes = self.nodes.checked_add(1).ok_or_else(|| {
            KernelError::new(
                KernelErrorKind::MemoryLimit,
                "runtime hash node counter overflow",
            )
        })?;
        if self.nodes > self.limits.max_nodes {
            return Err(KernelError::memory_limit(
                "runtime-hash-nodes",
                self.nodes,
                self.limits.max_nodes,
            ));
        }
        Ok(())
    }

    fn push<'a>(&self, stack: &mut Vec<Frame<'a>>, frame: Frame<'a>) -> Result<(), KernelError> {
        if stack.len() == self.limits.max_pending_frames {
            return Err(KernelError::memory_limit(
                "runtime-hash-frames",
                (stack.len() as u64).saturating_add(1),
                self.limits.max_pending_frames as u64,
            ));
        }
        if stack.len() == stack.capacity() {
            let growth = stack
                .capacity()
                .max(4)
                .min(self.limits.max_pending_frames - stack.len());
            stack.try_reserve_exact(growth).map_err(|_| {
                KernelError::new(
                    KernelErrorKind::MemoryLimit,
                    "runtime hash stack allocation failed",
                )
            })?;
        }
        stack.push(frame);
        Ok(())
    }
}

/// Hash the lossless tagged term encoding under the explicit v0.3 runtime-term profile.
/// Does not admit a term for serialization or select a value/effect-log profile.
pub fn runtime_term_hash(
    term: &Term,
    limits: RuntimeTermHashLimits,
) -> Result<[u8; 32], KernelError> {
    let mut encoder = Encoder {
        hasher: Hasher::new(),
        limits,
        nodes: 0,
        bytes: 0,
    };
    encoder.write(DOMAIN)?;
    let mut stack = Vec::new();
    encoder.push(&mut stack, Frame::Term(term))?;
    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Vector(mut items) => {
                if let Some(item) = items.next() {
                    if !items.as_slice().is_empty() {
                        encoder.push(&mut stack, Frame::Vector(items))?;
                    }
                    encoder.push(&mut stack, Frame::Term(item))?;
                }
            }
            Frame::Map(mut entries) => {
                if let Some((key, value)) = entries.next() {
                    if entries.len() != 0 {
                        encoder.push(&mut stack, Frame::Map(entries))?;
                    }
                    encoder.push(&mut stack, Frame::Term(value))?;
                    encoder.push(&mut stack, Frame::Term(&key.0))?;
                }
            }
            Frame::Term(term) => {
                encoder.node()?;
                match term {
                    Term::Nil => encoder.write(&[0])?,
                    Term::Bool(value) => encoder.write(&[1, u8::from(*value)])?,
                    Term::Int(value) => {
                        let sign = match value.sign() {
                            Sign::NoSign => 0,
                            Sign::Plus => 1,
                            Sign::Minus => 2,
                        };
                        encoder.write(&[2, sign])?;
                        let digits = value.iter_u32_digits();
                        encoder.count(digits.len())?;
                        let bytes = (digits.len() as u64).checked_mul(4).ok_or_else(|| {
                            KernelError::new(
                                KernelErrorKind::MemoryLimit,
                                "runtime hash integer length overflow",
                            )
                        })?;
                        encoder.charge_bytes(bytes)?;
                        for digit in digits {
                            encoder.hasher.update(&digit.to_le_bytes());
                        }
                    }
                    Term::Str(value) => {
                        encoder.write(&[3])?;
                        encoder.blob(value.as_bytes())?;
                    }
                    Term::Bytes(value) => {
                        encoder.write(&[4])?;
                        encoder.blob(value)?;
                    }
                    Term::Symbol(value) => {
                        encoder.write(&[5])?;
                        encoder.blob(value.as_bytes())?;
                    }
                    Term::Pair(car, cdr) => {
                        encoder.write(&[6])?;
                        encoder.push(&mut stack, Frame::Term(cdr))?;
                        encoder.push(&mut stack, Frame::Term(car))?;
                    }
                    Term::Vector(values) => {
                        encoder.write(&[7])?;
                        encoder.count(values.len())?;
                        if !values.is_empty() {
                            encoder.push(&mut stack, Frame::Vector(values.iter()))?;
                        }
                    }
                    Term::Map(values) => {
                        encoder.write(&[8])?;
                        encoder.count(values.len())?;
                        if !values.is_empty() {
                            encoder.push(&mut stack, Frame::Map(values.iter()))?;
                        }
                    }
                }
            }
        }
    }
    Ok(*encoder.hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests;
