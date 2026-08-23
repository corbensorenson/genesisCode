use super::*;

const MAX_BULK_OPS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BulkSetMode {
    CompareAndSet,
    SameOrAbsent,
    Unconditional,
}

impl BulkSetMode {
    fn symbol(self) -> &'static str {
        match self {
            Self::CompareAndSet => ":cas",
            Self::SameOrAbsent => ":same-or-absent",
            Self::Unconditional => ":unconditional",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BulkSetInput {
    pub(crate) name: String,
    pub(crate) new_hash: Option<String>,
    pub(crate) expected_old: Option<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BulkSetResult {
    Updated,
    Conflict {
        name: String,
        current: Option<String>,
    },
}

impl RefsAuthority {
    pub(crate) fn set_many(
        &mut self,
        refs: &RefsDb,
        ops: &[BulkSetInput],
        mode: BulkSetMode,
    ) -> Result<BulkSetResult, EffectsError> {
        for _ in 0..MAX_RETRIES {
            let snapshot = refs.snapshot()?;
            let payload = map([
                (":mode", Term::symbol(mode.symbol())),
                (
                    ":ops",
                    Term::Vector(ops.iter().map(bulk_input_term).collect()),
                ),
                (":refs", refs_term(&snapshot)),
            ]);
            let term = self.evaluate(":set-many", payload)?;
            match decode_bulk_set(term, &snapshot, ops, mode)? {
                BulkAuthorityDecision::Conflict { name, current } => {
                    return Ok(BulkSetResult::Conflict { name, current });
                }
                BulkAuthorityDecision::Write(replacement) => {
                    if refs.replace_if_unchanged(&snapshot, &replacement)? {
                        return Ok(BulkSetResult::Updated);
                    }
                }
            }
        }
        Err(authority_error(format!(
            "reference snapshot changed during all {MAX_RETRIES} authorized bulk-write attempts"
        )))
    }
}

enum BulkAuthorityDecision {
    Conflict {
        name: String,
        current: Option<String>,
    },
    Write(BTreeMap<String, String>),
}

fn bulk_input_term(input: &BulkSetInput) -> Term {
    map([
        (
            ":expected-old",
            input
                .expected_old
                .as_ref()
                .and_then(|value| value.as_ref())
                .map(|value| Term::Str(value.clone()))
                .unwrap_or(Term::Nil),
        ),
        (
            ":expected-old-present",
            Term::Bool(input.expected_old.is_some()),
        ),
        (":name", Term::Str(input.name.clone())),
        (
            ":new-hash",
            input
                .new_hash
                .as_ref()
                .map(|value| Term::Str(value.clone()))
                .unwrap_or(Term::Nil),
        ),
    ])
}

fn decode_bulk_set(
    result: DecodedResult,
    snapshot: &BTreeMap<String, String>,
    ops: &[BulkSetInput],
    mode: BulkSetMode,
) -> Result<BulkAuthorityDecision, EffectsError> {
    validate_bulk_inputs(ops, mode)?;
    if result.value.is_some() {
        return Err(authority_error("bulk set result value must be nil"));
    }
    let expected_conflict = ops.iter().find_map(|input| {
        let current = snapshot.get(&input.name).cloned();
        (!bulk_expected_matches(input, current.as_deref(), mode))
            .then(|| (input.name.clone(), current))
    });
    match result.action.as_str() {
        ":conflict" => {
            if result.refs.is_some() || result.entries.len() != 1 {
                return Err(authority_error("bulk conflict result shape contradiction"));
            }
            let (expected_name, expected_current) =
                expected_conflict.ok_or_else(|| authority_error("bulk false conflict decision"))?;
            let entry = &result.entries[0];
            if result.current != expected_current
                || entry.name != expected_name
                || entry.hash != expected_current
            {
                return Err(authority_error("bulk conflict attribution contradiction"));
            }
            Ok(BulkAuthorityDecision::Conflict {
                name: expected_name,
                current: expected_current,
            })
        }
        ":write" => {
            if result.current.is_some() || !result.entries.is_empty() {
                return Err(authority_error("bulk write result shape contradiction"));
            }
            if expected_conflict.is_some() {
                return Err(authority_error("bulk false write decision"));
            }
            let replacement = result
                .refs
                .ok_or_else(|| authority_error("bulk write result missing refs snapshot"))?;
            let mut expected = snapshot.clone();
            for input in ops {
                match &input.new_hash {
                    Some(hash) => {
                        expected.insert(input.name.clone(), hash.clone());
                    }
                    None => {
                        expected.remove(&input.name);
                    }
                }
            }
            if replacement != expected {
                return Err(authority_error("bulk replacement snapshot contradiction"));
            }
            Ok(BulkAuthorityDecision::Write(replacement))
        }
        _ => Err(authority_error(format!(
            "unsupported bulk set result action {}",
            result.action
        ))),
    }
}

fn validate_bulk_inputs(ops: &[BulkSetInput], mode: BulkSetMode) -> Result<(), EffectsError> {
    if ops.len() > MAX_BULK_OPS {
        return Err(authority_error(format!(
            "bulk ref update exceeds {MAX_BULK_OPS} operations"
        )));
    }
    for (index, input) in ops.iter().enumerate() {
        if input.name.is_empty()
            || input.new_hash.as_deref().is_some_and(|hash| !is_hash(hash))
            || input
                .expected_old
                .as_ref()
                .and_then(|value| value.as_deref())
                .is_some_and(|hash| !is_hash(hash))
            || (mode == BulkSetMode::SameOrAbsent && input.new_hash.is_none())
        {
            return Err(authority_error("invalid bulk ref input"));
        }
        if index > 0 && ops[index - 1].name >= input.name {
            return Err(authority_error(
                "bulk ref inputs must be strictly sorted and unique",
            ));
        }
    }
    Ok(())
}

fn bulk_expected_matches(input: &BulkSetInput, current: Option<&str>, mode: BulkSetMode) -> bool {
    match mode {
        BulkSetMode::CompareAndSet => input
            .expected_old
            .as_ref()
            .is_none_or(|expected| expected.as_deref() == current),
        BulkSetMode::SameOrAbsent => current.is_none() || input.new_hash.as_deref() == current,
        BulkSetMode::Unconditional => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: char) -> String {
        std::iter::repeat_n(byte, 64).collect()
    }

    fn input(
        name: &str,
        new_hash: Option<String>,
        expected_old: Option<Option<String>>,
    ) -> BulkSetInput {
        BulkSetInput {
            name: name.to_string(),
            new_hash,
            expected_old,
        }
    }

    fn decoded(action: &str) -> DecodedResult {
        DecodedResult {
            action: action.to_string(),
            current: None,
            entries: Vec::new(),
            refs: None,
            value: None,
        }
    }

    #[test]
    fn decoder_binds_first_cas_conflict_and_rejects_false_write() {
        let old = hash('a');
        let next = hash('b');
        let snapshot = BTreeMap::from([("refs/heads/main".to_string(), old.clone())]);
        let ops = vec![input("refs/heads/main", Some(next), Some(Some(hash('c'))))];
        let mut conflict = decoded(":conflict");
        conflict.current = Some(old.clone());
        conflict.entries.push(RefEntry {
            name: "refs/heads/main".to_string(),
            hash: Some(old),
        });
        assert!(matches!(
            decode_bulk_set(conflict, &snapshot, &ops, BulkSetMode::CompareAndSet),
            Ok(BulkAuthorityDecision::Conflict { .. })
        ));

        let mut false_write = decoded(":write");
        false_write.refs = Some(snapshot.clone());
        assert!(decode_bulk_set(false_write, &snapshot, &ops, BulkSetMode::CompareAndSet).is_err());
    }

    #[test]
    fn decoder_rejects_smuggled_bulk_write_and_unsorted_inputs() {
        let next = hash('b');
        let snapshot = BTreeMap::new();
        let ops = vec![input("refs/heads/main", Some(next.clone()), None)];
        let mut smuggled = decoded(":write");
        smuggled.refs = Some(BTreeMap::from([
            ("refs/heads/main".to_string(), next.clone()),
            ("refs/heads/smuggled".to_string(), hash('c')),
        ]));
        assert!(decode_bulk_set(smuggled, &snapshot, &ops, BulkSetMode::Unconditional).is_err());

        let unsorted = vec![
            input("refs/heads/main", Some(next.clone()), None),
            input("refs/heads/dev", Some(next), None),
        ];
        let mut write = decoded(":write");
        write.refs = Some(snapshot.clone());
        assert!(decode_bulk_set(write, &snapshot, &unsorted, BulkSetMode::Unconditional).is_err());
    }

    #[test]
    fn same_or_absent_and_unconditional_modes_are_distinct() {
        let current = hash('a');
        let remote = hash('b');
        let snapshot = BTreeMap::from([("refs/heads/main".to_string(), current.clone())]);
        let ops = vec![input("refs/heads/main", Some(remote.clone()), None)];

        let mut conflict = decoded(":conflict");
        conflict.current = Some(current.clone());
        conflict.entries.push(RefEntry {
            name: "refs/heads/main".to_string(),
            hash: Some(current),
        });
        assert!(matches!(
            decode_bulk_set(conflict, &snapshot, &ops, BulkSetMode::SameOrAbsent),
            Ok(BulkAuthorityDecision::Conflict { .. })
        ));

        let mut write = decoded(":write");
        write.refs = Some(BTreeMap::from([("refs/heads/main".to_string(), remote)]));
        assert!(matches!(
            decode_bulk_set(write, &snapshot, &ops, BulkSetMode::Unconditional),
            Ok(BulkAuthorityDecision::Write(_))
        ));
    }
}
