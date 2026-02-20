#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

echo "fuzz-differential-hardening: parser/canonicalizer fuzz invariants"
cargo test -p gc_coreform --test fuzz_parse_print --quiet

echo "fuzz-differential-hardening: patch schema fuzz invariants"
cargo test -p gc_patches --test fuzz_patch --quiet

echo "fuzz-differential-hardening: effect log fuzz invariants"
cargo test -p gc_effects --test fuzz_log --quiet

echo "fuzz-differential-hardening: optimizer rewrite fuzz invariants"
cargo test -p gc_opt --test fuzz_optimizer --quiet

echo "fuzz-differential-hardening: malformed/adversarial differential corpus"
cargo test -p gc_cli --test cli_differential_adversarial --quiet

echo "fuzz-differential-hardening: ok"
