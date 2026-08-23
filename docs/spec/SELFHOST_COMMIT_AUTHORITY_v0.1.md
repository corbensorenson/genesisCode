# Self-hosted Commit Authority v0.1

## Status and scope

This specification defines a partial `SD-COMMIT` production-authority slice under `R4.2.e`. For native `genesis commit new` and `genesis commit show`, the artifact-loaded `core/commit::authority` binding is the sole producer of canonical v1 commit construction and commit-object acceptance. The same binding is the sole commit-object admission authority for self-hosted `genesis vcs log`, `blame`, and `why` traversal and for direct `core/vcs-low::{log,blame,why}` capability execution. It owns the closed field inventory, required and optional field shapes, lowercase content identities, target kinds, obligation/evidence vectors, author metadata, and request-bound result verdict.

This slice remains H0. Ref resolution, patch loading/application, store, graph traversal, snapshot inspection, signing mechanisms, CLI transport, and command orchestration remain mechanisms outside this authority. Package, registry, GPK, and sync paths that construct or decode commits remain host-authoritative. This contract does not close `SD-COMMIT`, `SD-CANON-IDENTITY`, `SD-VCS`, `R4.2.e`, SH-C, bootstrap fixpoint, or release qualification.

## Exact protocol

Every request is an exact map with `:kind`, `:op`, `:payload`, and `:v`. The kind is `genesis/commit-authority-request-v0.1`, version is `1`, and the operation is `:make` or `:validate`. Open, mistyped, unsupported, or version-mismatched requests are rejected.

`:make` accepts exactly `:author`, `:base`, `:evidence`, `:message`, `:obligations`, `:parents`, `:patch`, `:result`, `:sign`, `:target-id`, `:target-kind`, and `:why`. It constructs the canonical object rather than approving a host-built candidate. Required identities are exactly 64 lowercase hexadecimal characters. Target kind is one of `:package`, `:module`, `:contract`, or `:workspace`; target identifier and message are nonempty after trimming. Optional author, signer ID, and rationale are nil or nonempty strings. Obligations are nonempty strings or symbols; parent, evidence, and attestation identities are lowercase hashes.

`:validate` accepts exactly `{:artifact value}` and returns the artifact only if it is a closed v1 commit. Required fields are `:attestations`, `:base`, `:evidence`, `:message`, `:obligations`, `:parents`, `:patch`, `:result`, `:type`, and `:v`; optional fields are `:author`, `:target`, and `:why`. Unknown fields, malformed optional maps, noncanonical hashes, wrong versions, and wrong types are rejected.

Every result is an exact map containing `:artifact`, `:code`, `:kind`, `:message`, `:ok`, `:request-h`, and `:v`. Its kind is `genesis/commit-authority-result-v0.1`, and `:request-h` is the canonical hash of the exact request. Success carries a commit map and nil error fields. Rejection carries nil artifact and nonempty code/message. Rust reifies runtime collections, rejects open or request-unbound results, and cannot substitute an artifact.

## Host boundary and evidence

Rust may load and apply the artifact binding under existing bounded evaluator limits, transport CLI arguments, resolve refs, invoke already-authorized store and patch mechanisms, traverse authority-admitted parent/result identities, independently hash the returned canonical artifact as a contradiction check, and render diagnostics. The self-hosted VCS query program may perform the same request-bound call inside GenesisCode and project fields only from the exact returned artifact. Neither route may construct an accepted commit, silently accept a missing authority, default an invalid result to success, normalize a rejected identity into acceptance, substitute a different artifact, or inspect a stored commit without authority validation.

`scripts/lib/selfhost_commit_authority.py` checks the source/profile identities, exact manifest custody, shared bounded adapter, native CLI route, self-hosted and low-level VCS history routes, strict runtime-value decoder, absence of the retired VCS parser on those routes, native roundtrip and open-object controls, truthful ledger scope, admitted artifact, and permanent source/route mutations. These checks prove only the named partial construction/inspection/history-admission slice; the host-authoritative residual inventory must be eliminated before promotion above H0.
