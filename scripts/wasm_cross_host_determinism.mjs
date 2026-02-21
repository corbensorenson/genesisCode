import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import path from "node:path";
import process from "node:process";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const startMs = Date.now();

function isHex32(s) {
  return typeof s === "string" && /^[0-9a-f]{64}$/.test(s);
}

function runNative() {
  const stdout = execFileSync(
    "cargo",
    ["run", "-p", "gc_wasm", "--example", "native_effect_hashes", "--quiet"],
    { encoding: "utf8" },
  );
  const o = JSON.parse(stdout);
  for (const k of [
    "module_h",
    "payload_h",
    "cont_h",
    "req_h",
    "resp_h",
    "final_value_h",
  ]) {
    assert.ok(isHex32(o[k]), `native ${k} must be 64-hex`);
  }
  return o;
}

function runNodeWasm(modPath) {
  // wasm-bindgen (--target nodejs) emits a CommonJS module.
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  const wasm = require(modPath);

  const src = `
    (core/effect::perform
      'sys/time::now
      nil
      (fn (t) (core/effect::pure t)))
  `;

  const rt = new wasm.Runtime(0);
  const step = rt.eval_module(src);
  assert.equal(step.kind, "effect");

  const resumed = rt.respond_denied();
  assert.equal(resumed.next.kind, "done");

  const out = {
    module_h: step.module_h,
    payload_h: step.payload_h,
    cont_h: step.cont_h,
    req_h: step.req_h,
    resp_h: resumed.resp_h,
    final_value_h: resumed.next.value_h,
  };
  for (const [k, v] of Object.entries(out)) {
    assert.ok(isHex32(v), `wasm ${k} must be 64-hex`);
  }
  return out;
}

const modPath = path.resolve(
  process.argv[2] ?? "target/wasm-bindgen/gc_wasm/gc_wasm.js",
);
const native = runNative();
const wasm = runNodeWasm(modPath);

for (const k of Object.keys(native)) {
  assert.equal(
    wasm[k],
    native[k],
    `cross-host mismatch for ${k}: wasm=${wasm[k]} native=${native[k]}`,
  );
}

const elapsedMs = Date.now() - startMs;
const reportPath =
  process.env.GENESIS_WASM_CROSS_HOST_PROFILE_REPORT ??
  ".genesis/perf/wasm_cross_host_profile_report.json";
const historyPath =
  process.env.GENESIS_WASM_CROSS_HOST_PROFILE_HISTORY ??
  ".genesis/perf/wasm_cross_host_profile_history.jsonl";
const budgetMs =
  process.env.GENESIS_WASM_CROSS_HOST_BUDGET_MS ?? "300000";
const minHistory =
  process.env.GENESIS_WASM_CROSS_HOST_MIN_HISTORY ?? "5";
const helperPath = path.resolve("scripts/lib/profile_runtime_budget.py");
execFileSync(
  "python3",
  [
    helperPath,
    "--profile",
    "wasm-cross-host",
    "--kind",
    "genesis/test-profile-runtime-v0.1",
    "--report",
    reportPath,
    "--history",
    historyPath,
    "--elapsed-ms",
    `${elapsedMs}`,
    "--budget-ms",
    `${budgetMs}`,
    "--min-history",
    `${minHistory}`,
    "--extra-json",
    '{"command":"node scripts/wasm_cross_host_determinism.mjs"}',
  ],
  { stdio: "inherit" },
);

process.stdout.write("ok\n");
