# Self-host Policy Alias Authority v0.1

Status: normative H2 evidence contract for `SD-POLICY-ALIAS` only.

`core/cli::policy-authority` is the sole production semantic producer for local
policy-config normalization, alias lookup, direct-hash normalization, default
selection, and default mutation. The Rust host may decode and encode TOML, read and
write the selected file, validate the closed authority response, and report errors.
It must not normalize, select, resolve, or silently fall back in a production build.

## Closed Protocol

The request kind is `genesis/policy-authority-request-v0.1` with exactly `:kind`,
`:v`, `:operation`, `:config`, and `:selector`. Operations are `:list`, `:resolve`,
and `:set-default`. Config has exactly `:version`, `:default`, and `:aliases`;
aliases are an ordered vector of exact `{:name ... :hash ...}` records.

Successful `genesis/policy-authority-result-v0.1` values carry `:ok true`, the exact
operation, normalized config facts, and only the operation-specific decision facts.
Expected user-controlled failures carry `:ok false`, one of `policy/parse`,
`policy/resolve`, or `policy/set-default`, and a nonempty message. Malformed host
requests, unknown operations, and open request records remain sealed protocol errors.
The host rejects unknown fields, operation drift, noncanonical hashes, duplicate
aliases, and every malformed success or denial result.

Policy names, selectors, defaults, and hash strings retain the previous Rust
`str::trim` behavior under the frozen Unicode text profile. Hashes accept exactly 32
decoded bytes of case-insensitive hexadecimal and emit lowercase hexadecimal.
Aliases that collide after trimming fail closed. `default` resolves once and cannot
refer to itself. A missing default and an unknown alias are verification denials.

## Authority And Verification

`policies/selfhost_policy_alias_authority_v0.1.json` is the closed machine profile.
`scripts/lib/selfhost_policy_alias_authority.py` is an independent standard-library
verifier and imports no GenesisCode or Rust implementation crate. It binds the exact
source module, artifact, binding, decision inventory, production entrypoints, error
taxonomy, compatibility-oracle feature, and runtime limits. Static checks reject a
missing manifest module or binding, an ungated Rust semantic producer, a production
package graph that activates `parity-harness`, and a production entrypoint that uses
the parity driver.

Runtime verification executes native and WASI production binaries over independent
temporary copies of the same inputs. It requires identical canonical list and
set-default decisions, Unicode trim behavior, parse and verification exit taxonomy,
and no policy-file mutation after denial. The self-test mutates every authority class
and must reject every mutation.

`SD-POLICY-ALIAS` may reach H2 only when the verifier, runtime controls, production
tests, reviewed artifact publication, strict boundary gate, and durable evidence all
bind the same revision and profile identity. This contract does not promote
`SD-EFFECT-POLICY`, `SD-REPLAY`, `SD-OBLIGATION`, `SD-SIGNING`, or
`SD-EVIDENCE-VERIFY`; it does not close R4.2.d, establish H3/H4, qualify a release, or
authorize a downstream product.
