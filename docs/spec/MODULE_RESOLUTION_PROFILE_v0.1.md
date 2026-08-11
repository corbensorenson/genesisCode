# Module Resolution Profile v0.1

Status: normative for modules that opt into
`genesis/module-resolution-profile-v0.1` under `genesis/language-profile/v0.2`.

The closed machine contract is `docs/spec/MODULE_RESOLUTION_PROFILE_v0.1.json`, validated
against `docs/spec/MODULE_RESOLUTION_PROFILE_v0.1.schema.json`. The profile freezes module
content identity, imports, exports, visibility, order and cycles, exact profile constraints,
workspace replacement, and package boundaries. Host directory enumeration, locale, current
working directory, dependency discovery, and version-label inference are never resolution inputs.

## Activation and metadata

The profile is opt-in until its self-hosted authority migration. If any module in a supplied
package closure declares `:module-profile`, every module in that closure must declare exactly:

~~~clojure
:module-profile genesis/module-resolution-profile-v0.1
:requires-profiles {
  genesis/coreform-profile genesis/coreform/v0.2
  genesis/hash-profile genesis/hash-profile/gcv0.2-blake3
  genesis/language-profile genesis/language-profile/v0.2
  genesis/module-resolution-profile genesis/module-resolution-profile-v0.1}
:imports [package/module::symbol ...]
:exports [package/module::symbol ...]
~~~

The four `:requires-profiles` bindings are a closed exact map. Missing, additional, gradual,
range-based, or version-inferred bindings fail before typechecking or execution. R5.3.c may add a
new negotiated package profile, but cannot reinterpret this v0.1 identity.

## Content and graph identity

Each module identity is `hash_module(canonicalize_module(parse_module(source)))` under
`genesis/hash-profile/gcv0.2-blake3` and the `GCv0.2` domain. Source spelling that canonicalizes to
the same CoreForm has the same identity; any canonical form, metadata, import, export, or profile
change changes it.

The resolution identity is the hash of canonical CoreForm data containing the exact profile ID and
the manifest-ordered module records. Each record binds its base-relative `/` path, module content
hash, sorted import set, sorted export set, and exact required-profile map. Invalid closures have no
resolution identity. Package artifact and lock identities additionally bind manifest order,
dependencies, obligations, and their exact content hashes as specified by `PACKAGE_TOML.md` and
`LOCKFILE.md`.

Paths are portable material under `TEXT_PATH_PROFILE_v0.1`: valid UTF-8, NFC, `/` separated,
base-relative, and free of empty, dot, parent, drive, or backslash components. Absolute host paths
never enter module, package, diagnostic, or graph identity.

## Resolution order and cycles

The `package.toml` module vector is semantic order. Modules evaluate from first to last. An import
may resolve only to a uniquely owned public export of an earlier module. The order is bound into the
package and resolution identities; no filesystem walk, hash-map iteration, or opportunistic
topological sort may replace it.

Forward imports and self-imports are rejected. Therefore every directed local import edge points to
a lower manifest index and the graph is acyclic by construction. Any cycle necessarily contains a
forward or self edge and fails before execution. Recursive and mutually recursive definitions
remain valid only within one module under `MODULE_SCOPE.md`; they do not create module import edges.

## Imports, exports, and visibility

`:imports` and `:exports` are mathematical sets serialized as vectors. Entries are symbols and must
be unique. Their canonical graph representation is lexical. A local definition must not be listed
as an import. Every export must be defined by its declaring module, and one symbol may have only one
module owner in a package closure.

Visibility is deliberately binary:

- A top-level definition listed in `:exports` is public.
- Every other top-level definition is private to its module.

A cross-module reference must name a declared import, whose unique owner is an earlier module and
whose owner exports it. Declaring an import cannot expose a private definition. Lexical `fn` and
`let` bindings shadow matching qualified symbols and do not become imports. Quoted symbols and map
keys are data, not references.

Package dependencies form an outer resolution layer. Only exports of manifest-declared,
hash-verified dependency packages enter the base environment; dependency private definitions never
cross the package boundary. Package-local `:imports` govern edges among the current manifest's
modules. A future module-level external-import index must be a new profile and must bind dependency
package identity; this profile does not infer one from symbol spelling.

## Package closure and workspace replacement

Resolution starts only from the explicit root `package.toml` and `genesis.lock` closure:

- module and dependency paths are validated portable relative paths;
- every module canonical hash matches its manifest entry;
- every dependency artifact hash matches the manifest and lock;
- only dependency public exports enter consumers;
- duplicate dependency names or identities are errors rather than precedence choices;
- dependency order is explicit and identity-bound.

There is no ambient workspace override in v0.1. A sibling directory, editor workspace, environment
variable, registry cache, or newer version cannot shadow a locked dependency. Replacing a local or
registry dependency requires an explicit manifest/lock update or semantic patch that names the old
identity and the replacement identity. The resulting package and resolution closure receive new
identities and must be reverified. This is deterministic replacement, not an invisible override.

## Errors and deterministic reporting

Resolution errors are ordinary explicit tool errors, never host panics. Diagnostics are attributed
to base-relative module paths. Module paths and messages are sorted lexically when projected from
sets. At minimum, malformed metadata, duplicate paths or entries, duplicate ownership, undefined
exports, unresolved imports, private imports/references, undeclared cross-module references,
forward/self imports, profile drift, hash mismatch, and undeclared dependency replacement fail
closed.

Changing module input order may change validity and always changes the closure identity. Repeating
resolution with identical canonical modules, manifest order, dependency identities, and exact
profiles produces the same report and identity on every conforming host.

## Authority and migration

`gc_types::resolve_module_profile` is the current independently testable Rust stage0 realization.
`typecheck_package` enforces it whenever the opt-in marker is present. Existing unmarked modules
retain v0.2 legacy behavior until R4.2 moves frontend/type/package authority to reviewed `.gc`
implementations and R5.3.d provides a semantic migration. The opt-in does not itself claim H2,
change production self-host authority, rewrite a manifest, or authorize package compatibility.

A profile change updates this prose, closed JSON and schema, resolver and package source bindings,
positive and adversarial controls, agent cards, migration guidance, and content identity in one
reviewed transaction. An existing profile ID is immutable.

## Nonclaims

- Version labels, semantic-version ranges, or source compatibility do not imply profile
  compatibility; only exact profile identities do.
- This profile does not provide implicit re-exports, aliases, wildcard imports, friend visibility,
  runtime dynamic loading, or ambient workspace overrides.
- It does not switch production semantic authority before the corresponding R4.2 migration.
- It does not promote a package, registry, self-host, backend, benchmark, Foundry result, or release
  level.
