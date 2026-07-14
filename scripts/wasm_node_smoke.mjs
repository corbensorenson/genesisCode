import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

function isHex32(s) {
  return typeof s === "string" && /^[0-9a-f]{64}$/.test(s);
}

const cargoTargetDir = process.env.CARGO_TARGET_DIR ?? "target";
const modPath = path.resolve(
  process.argv[2] ??
    path.join(cargoTargetDir, "wasm-bindgen", "gc_wasm", "gc_wasm.js"),
);
// wasm-bindgen (--target nodejs) emits a CommonJS module.
// eslint-disable-next-line @typescript-eslint/no-var-requires
const wasm = require(modPath);

assert.equal(typeof wasm.fmt_coreform_term, "function");
assert.equal(typeof wasm.hash_coreform_term, "function");
assert.equal(typeof wasm.fmt_coreform_module, "function");
assert.equal(typeof wasm.hash_coreform_module, "function");
assert.equal(typeof wasm.fmt_coreform_module_selfhost, "function");
assert.equal(typeof wasm.hash_coreform_module_selfhost, "function");
assert.equal(typeof wasm.fmt_coreform_module_selfhost_with_artifact, "function");
assert.equal(typeof wasm.hash_coreform_module_selfhost_with_artifact, "function");
assert.equal(typeof wasm.Runtime, "function");

const artifactPath = path.resolve(
  process.argv[3] ?? "selfhost/toolchain.gc",
);
const artifactSrc = fs.readFileSync(artifactPath, "utf8");

// Term fmt/hash idempotency and canonical hashing invariants.
const t0 = "{:b 2 :a 1}";
const fmt1 = wasm.fmt_coreform_term(t0);
const fmt2 = wasm.fmt_coreform_term(fmt1);
assert.equal(fmt2, fmt1, "fmt_coreform_term should be idempotent");

const h0 = wasm.hash_coreform_term(t0);
const h1 = wasm.hash_coreform_term(fmt1);
assert.ok(isHex32(h0), "hash_coreform_term should be 64-hex");
assert.equal(h1, h0, "hash_coreform_term should canonicalize inputs");

// Module fmt/hash equivalence between rust frontend and self-host toolchain.
const m0 = `
  ; messy module input (canonical output must be stable)
  (def  m::x   1)
  (def m::y (prim int/add m::x 2))
  m::y
`;
const mfmtRust = wasm.fmt_coreform_module(m0);
assert.throws(
  () => wasm.fmt_coreform_module_selfhost(m0, 5_000_000),
  /selfhost\/artifact-required/,
  "wasm selfhost fmt without explicit artifact should fail closed",
);
const mfmtSelf = wasm.fmt_coreform_module_selfhost_with_artifact(
  m0,
  artifactSrc,
  5_000_000,
);
assert.equal(mfmtSelf, mfmtRust, "selfhost module fmt must match rust module fmt");

const mhRust = wasm.hash_coreform_module(m0);
assert.throws(
  () => wasm.hash_coreform_module_selfhost(m0, 5_000_000),
  /selfhost\/artifact-required/,
  "wasm selfhost hash without explicit artifact should fail closed",
);
const mhSelf = wasm.hash_coreform_module_selfhost_with_artifact(
  m0,
  artifactSrc,
  5_000_000,
);
assert.ok(isHex32(mhRust), "hash_coreform_module should be 64-hex");
assert.equal(mhSelf, mhRust, "selfhost module hash must match rust module hash");

// Effectful stepping smoke test (host denies, kernel constructs sealed ERROR).
const src = `
  (core/effect::perform
    'sys/time::now
    nil
    (fn (t) (core/effect::pure t)))
`;

const rt = new wasm.Runtime(0);
const step = rt.eval_module(src);

assert.equal(step.kind, "effect");
assert.equal(step.op, "sys/time::now");
assert.equal(step.payload, "nil");
assert.ok(isHex32(step.module_h));
assert.ok(isHex32(step.payload_h));
assert.ok(isHex32(step.cont_h));
assert.ok(isHex32(step.req_h));

const resumed = rt.respond_denied();
assert.ok(isHex32(resumed.resp_h));
assert.equal(resumed.next.kind, "done");
assert.ok(
  resumed.next.value.includes("core/caps/denied"),
  "denied response should be a caps-denied ERROR payload in log form",
);
assert.ok(isHex32(resumed.next.value_h));

process.stdout.write("ok\n");
