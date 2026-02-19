#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT="${GENESIS_RUNTIME_MICROBENCH_OUT:-.genesis/perf/runtime_microbench_metrics.json}"

echo "runtime-microbench: running benchmark suite"
cargo run -p gc_runtime_bench -- --out "$OUT"

echo "runtime-microbench: metrics"
cat "$OUT"
