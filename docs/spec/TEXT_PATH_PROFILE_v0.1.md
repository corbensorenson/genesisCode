# Text and Path Profile v0.1

Status: normative for text, bytes, and portable path material in
`genesis/language-profile/v0.2`.

The closed machine contract is `docs/spec/TEXT_PATH_PROFILE_v0.1.json`, validated against
`docs/spec/TEXT_PATH_PROFILE_v0.1.schema.json`. Unicode behavior is fixed to Unicode 17.0.0.
Changing a Rust toolchain, host locale, filesystem, Unicode table, optimizer, or backend cannot
silently change this profile.

## Strings and bytes

`Str` is an immutable sequence of Unicode scalar values encoded as well-formed UTF-8. Parsing,
construction from bytes, canonical printing, serialization, equality, ordering, and hashing preserve
the exact scalar sequence. GenesisCode does not normalize strings implicitly: `"é"` and
`"e\u{301}"` are distinct values and hashes even though they are canonically equivalent.

`Bytes` is an immutable sequence of octets. Byte strings use the `b"..."` reader form and preserve
every byte. `str/to-bytes-utf8` is exact UTF-8 encoding. `bytes/to-str-utf8` accepts only well-formed
UTF-8 and otherwise returns trusted sealed `core/type-error`; no replacement character is inserted.
Byte length, slicing, equality, ordering, serialization, and hashing are octet-exact.

## Length, normalization, and graphemes

The v0.2 APIs deliberately distinguish units:

- `str/len` and `core/str::len` return UTF-8 byte length for compatibility.
  `core/str::byte-len` is the preferred explicit spelling.
- `str/scalar-len` and `core/str::scalar-len` count Unicode scalar values.
- `str/grapheme-len` and `core/str::grapheme-len` count default extended grapheme clusters under
  Unicode 17.0.0 UAX #29 behavior.
- `str/grapheme-slice` and `core/str::grapheme-slice` index by those clusters and return the exact
  original UTF-8 substring without normalization. `start` and `len` are non-negative integers;
  overflow or a boundary beyond the cluster count returns trusted sealed `core/text-range-error`.
  An empty slice at the final boundary is valid.
- `str/nfc` and `core/str::nfc` explicitly apply Unicode 17.0.0 canonical composition under UAX #15.
  NFC is idempotent and preserves canonical meaning; NFD, NFKC, and NFKD are not Core v0.2 APIs.

The implementation tables are pinned by exact dependencies
`unicode-normalization/0.1.25` and `unicode-segmentation/1.13.3`. The former exposes and is checked
against Unicode version `(17, 0, 0)`; the latter is the reviewed Unicode 17.0.0 release. Updating
either dependency is a language-profile change, not routine maintenance.

## Case and locale

Core string and symbol equality, map-key identity, ordering, and hashing are case-sensitive and
scalar-exact. Core v0.2 has no ambient locale, locale-sensitive case conversion, collation,
case-folded identity, title casing, or host-library fallback. Locale-aware presentation belongs in a
separately versioned library/profile with explicit locale data. ASCII or Unicode case conversion may
not be inferred from the process locale.

## Portable path material

Paths are not a pure-kernel value kind. Capability and package boundaries carry a `Str` under an
explicit path contract. Canonical filesystem-effect input is:

- valid UTF-8 in Unicode 17 NFC;
- relative to the capability `base_dir`;
- `/` separated on every host;
- case-sensitive as language identity;
- either `.` for the base itself or non-empty components containing neither `.` nor `..`;
- free of empty components, a leading slash, a drive prefix, and backslashes.

Invalid material is rejected before host access. Absolute paths are policy configuration, never
language payloads. Host resolution still occurs under the filesystem sandbox and symlink rules.
Host filesystems may differ in permissions, case enforcement, and race behavior; those outcomes are
effect responses and are recorded for replay, but the runtime never case-folds a requested path.

Filesystem response paths and names must be representable as valid UTF-8 and are emitted as NFC,
base-relative `/` material. A non-UTF-8 name returns trusted sealed `core/path-encoding-error` rather
than a lossy replacement. Two host names that collapse to one NFC response identity return trusted
sealed `core/path-collision-error`; they are never silently merged.

## Errors, logs, and roots

An IO error payload contains `:base-dir "."`, a base-relative NFC `:path`, stable `:op`, and stable
`:io-kind`. It never contains the configured absolute base, current directory, home directory, or
an OS error message. A path outside the base is represented as `<outside-base>` and an
unrepresentable path as `<invalid-path>`. These sentinels disclose no host prefix.

Effect logs bind the canonical request and complete response. Equivalent runs under different
absolute workspace roots therefore produce the same path material when their relative topology and
effect outcomes agree. Replay consumes and strictly checks the recorded response rather than
re-resolving a host path.

## Resource behavior and execution tiers

Text operations are pure and deterministic. String and byte limits are measured in UTF-8 bytes and
octets respectively. NFC computes the exact output byte length, checks logical string limits, and
uses fallible allocation. Grapheme operations allocate only the returned substring. User-controlled
text cannot panic the host.

- The reference and compiled AST evaluators share the same Unicode implementation and must match
  values, trusted sealed failures, hashes, and resource observations.
- `gc_wasm` executes through `gc_kernel` and inherits this profile.
- Stage 1 preserves string and byte values exactly.
- Stage 2 supports translation-validated byte/scalar/grapheme lengths and NFC when string values are
  statically known. Dynamic values and grapheme slicing are unsupported in the current candidate
  tier and fail closed before artifact authority; the reference route remains available.
- Host bridges may receive paths only after capability validation and cannot redefine path identity.

## Change rule

A text or path change updates this prose, closed JSON and schema, exact Unicode dependencies,
parser/type/runtime/Prelude/effect paths, affected backends, positive and adversarial controls,
agent cards, migration guidance, and content identity in one reviewed transaction. A Unicode-version
change requires profile negotiation because grapheme boundaries, normalization tables, and resulting
artifacts can change.

## Nonclaims

- This profile does not provide locale-aware collation, casing, word or sentence segmentation,
  regex semantics, bidi layout, line breaking, or display-width measurement.
- It does not make host filesystem behavior identical; it makes language path material explicit,
  bounded, leak-free, and replayable.
- It does not claim Stage 2 supports every text program; unsupported candidates fail closed.
- It does not promote a self-host, backend, host sandbox, package, or release level.
