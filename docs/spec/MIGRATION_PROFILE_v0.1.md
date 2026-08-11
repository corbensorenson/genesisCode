# GenesisCode Canonical Migration Profile v0.1

Status: normative for `genesis/migration-profile-v0.1`.

This profile defines a pure, deterministic planner for upgrading canonical GenesisCode package
syntax, public API symbols, and metadata-backed formats. It emits the existing
`genesis/patch-profile/v0.2` version-1 semantic patch format. The planner never writes files,
executes a module, grants a capability, infers compatibility from a version label, or changes
semantic authority. Transactional snapshot verification, patch application, obligations, and
explicit promotion remain the responsibility of the existing patch/session machinery.

## Inputs and source identity

A migration consumes a non-empty manifest-ordered vector of `ModuleForTypecheck` values and a
`MigrationPlan`. Every module path must be unique, portable, base-relative, slash-separated, and
free of empty, dot, parent, drive, or backslash components and must be Unicode NFC. Forms must already be canonical
CoreForm, and the supplied metadata projection must exactly equal the unique `::meta` definition.
Malformed, detached, duplicate, or noncanonical input receives no migration identity.

A request contains at most 65,536 modules and 65,536 migration steps. Migration and producer IDs
are at most 1,024 UTF-8 bytes, intent is at most 16,384 bytes, and paths, symbols, and source-artifact
IDs are at most 4,096 bytes. Recursive rewrites use bounded stack growth equivalent to the CoreForm
canonicalizer. Target diagnostics retain at most eight errors and 512 Unicode scalar values per
error; metadata mismatch diagnostics expose canonical term hashes rather than attacker-sized terms.

The source package identity is BLAKE3 under `genesis/hash-profile/gcv0.2-blake3` over the migration
profile identifier and the manifest-ordered vector of portable module paths plus canonical module
hashes. Order is semantic. The plan names this exact identity; a mismatch is a stale-source error
before any rewrite or target analysis. Absolute paths, mtimes, process state, environment, caches,
and host-specific material are absent from every identity.

## Plan and canonical ordering

A plan contains a portable migration identifier, non-empty intent, exact expected source package
identity, typed provenance, and at least one operation. Operations are unique and strictly ordered
by the closed class order `rewrite-syntax-head`, `rename-api-symbol`, then
`replace-format-field`, followed by their canonical operands. Duplicate or out-of-order operations
fail. A symbol that is the target of one rewrite may not be the source of another in the same plan;
multi-hop rewrites require separately receipted migrations so intermediate identity and effects
cannot be hidden.

Each symbol rewrite declares an exact positive occurrence count. Count drift fails rather than
silently broadening or narrowing a migration. A format replacement declares exact presence or
absence and exact prior value, distinct from its replacement. A failed precondition produces no
patch or receipt.

## Closed operation semantics

### `rewrite-syntax-head`

This operation targets one named module and rewrites only exact proper-list head symbols in
executable CoreForm. It recursively visits executable list positions and map values, does not
rewrite vectors or quoted data, preserves form count and module path, and recanonicalizes the
result. It is intended for reviewed source-form migrations, not unrestricted textual replacement.

### `rename-api-symbol`

This package-wide operation requires qualified `namespace::name` source and target symbols. It
rewrites exact top-level definition names, unshadowed executable references, and exact symbols in
the canonical `::meta` data projection, including imports, exports, and type keys. It respects
`fn` and `let` lexical binders, leaves ordinary quoted program data unchanged, and rejects a target
that already has a package definition when the source is package-defined. Target typechecking and
module/profile rules remain authoritative for ownership, visibility, shape, effect, and contract
validity.

### `replace-format-field`

This operation targets one module's existing unique `::meta` map. The field is a non-empty keyword
symbol. Exact expected presence/value is checked before inserting, replacing, or removing the
field. Quoted versus direct metadata representation is preserved, then the module is canonicalized.
The operation does not interpret a version string as compatibility.

## Dry-run, patch, and receipt

Dry-run clones all modules and mutates only the private clone. After the complete canonical plan,
it re-derives metadata, requires the target package to pass the current typechecker and the explicit
profile offer (the Core host offer by default), and rejects a no-op package identity. It then emits one deterministic `:replace-node`
operation for every changed top-level form, ordered by manifest module order and form index. Each
operation replaces the complete final canonical form at path `[[:form i]]`; no textual edit or
hidden executor semantics are introduced.

The patch provenance binds the migration and patch profile IDs, migration ID, complete plan hash,
exact source and target package hashes, and the request provenance. The dry-run receipt binds those
facts plus patch hash, every before/after module hash, changed form indices, per-module and package
effect snapshots and added/removed operation sets, and the complete successful target typecheck
report. Unknown-effect flags remain explicit. The receipt envelope contains the hash of its report
payload; the hash never includes itself.

Re-running the same canonical inputs produces byte-identical plan, patch, target, and receipt
identities. Changing intent, provenance, parent receipt, operation order, operands, preconditions,
source forms, module order, or target semantics changes or invalidates the corresponding identity.
An output may be independently replayed by recomputing the dry-run and then applying the emitted
patch only inside an exact-snapshot Genesis session.

## Provenance

Typed request provenance contains a non-empty portable producer ID, a non-empty source-artifact
identity, and either an exact 32-byte parent receipt hash or `nil`. The complete value is included
unchanged in both plan identity and output artifacts. A follow-on migration names the prior receipt
to form an explicit chain; there is no ambient author, clock, repository, or branch inference.

## Failure and authority boundary

Invalid paths, canonicality or metadata mismatch, stale source identity, malformed or ambiguous
plans, count drift, missing fields, collisions, canonicalization failure, no-op output, and target
typecheck failure return explicit `MigrationError` values before file mutation. User input cannot
panic the host.

This profile does not:

- make a compatibility or deprecation promise;
- infer compatibility from semver, filenames, prose, or model output;
- apply a patch, grant write authority, bypass session snapshots, or satisfy obligations;
- rewrite arbitrary quoted user data or perform textual search-and-replace;
- switch Rust/GenesisCode semantic ownership or satisfy the R4.2 authority migration;
- stabilize a v1 patch format, bytecode, package, deployment target, or reader;
- promote a package, benchmark, Foundry result, model, assurance level, or release level.

Any new operation semantics, identity input, accepted patch version, provenance field, or target
admission rule requires a new migration profile and an explicit migration record.
