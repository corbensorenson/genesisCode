import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import http from "node:http";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { readFile, writeFile } from "node:fs/promises";

import { chromium } from "playwright";

function isHex32(s) {
  return typeof s === "string" && /^[0-9a-f]{64}$/.test(s);
}

function runNativeEffectHashes() {
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

function contentTypeFor(p) {
  if (p.endsWith(".html")) return "text/html; charset=utf-8";
  if (p.endsWith(".js")) return "text/javascript; charset=utf-8";
  if (p.endsWith(".wasm")) return "application/wasm";
  if (p.endsWith(".map")) return "application/json; charset=utf-8";
  return "application/octet-stream";
}

function mustUnderRoot(rootDir, relUrlPath) {
  const rel = relUrlPath.replaceAll("\\", "/").replace(/^\//, "");
  const full = path.resolve(rootDir, rel);
  const root = path.resolve(rootDir);
  if (!full.startsWith(root + path.sep) && full !== root) {
    throw new Error("path traversal");
  }
  return full;
}

async function writeHarnessHtml(outDir) {
  const html = `<!doctype html>
<html lang="en">
  <meta charset="utf-8">
  <title>Genesis WASM Web Smoke</title>
  <body>
    <pre id="out">running</pre>
    <script type="module">
      function isHex32(s) {
        return typeof s === "string" && /^[0-9a-f]{64}$/.test(s);
      }

      async function main() {
        const outEl = document.getElementById("out");
        try {
          const mod = await import("./gc_wasm.js");
          const init = mod.default;
          await init();

          if (typeof mod.fmt_coreform_term !== "function") throw new Error("missing fmt_coreform_term");
          if (typeof mod.hash_coreform_term !== "function") throw new Error("missing hash_coreform_term");
          if (typeof mod.fmt_coreform_module !== "function") throw new Error("missing fmt_coreform_module");
          if (typeof mod.hash_coreform_module !== "function") throw new Error("missing hash_coreform_module");
          if (typeof mod.fmt_coreform_module_selfhost !== "function") throw new Error("missing fmt_coreform_module_selfhost");
          if (typeof mod.hash_coreform_module_selfhost !== "function") throw new Error("missing hash_coreform_module_selfhost");
          if (typeof mod.Runtime !== "function") throw new Error("missing Runtime");

          const t0 = "{:b 2 :a 1}";
          const fmt1 = mod.fmt_coreform_term(t0);
          const fmt2 = mod.fmt_coreform_term(fmt1);
          if (fmt2 !== fmt1) throw new Error("fmt_coreform_term not idempotent");

          const h0 = mod.hash_coreform_term(t0);
          const h1 = mod.hash_coreform_term(fmt1);
          if (!isHex32(h0)) throw new Error("hash_coreform_term must be 64-hex");
          if (h1 !== h0) throw new Error("hash_coreform_term should canonicalize inputs");

          const m0 = \`
            ; messy module input (canonical output must be stable)
            (def  m::x   1)
            (def m::y (prim int/add m::x 2))
            m::y
          \`;
          const mfmtRust = mod.fmt_coreform_module(m0);
          const mfmtSelf = mod.fmt_coreform_module_selfhost(m0, 5000000);
          if (mfmtSelf !== mfmtRust) throw new Error("selfhost fmt must match rust fmt");

          const mhRust = mod.hash_coreform_module(m0);
          const mhSelf = mod.hash_coreform_module_selfhost(m0, 5000000);
          if (!isHex32(mhRust)) throw new Error("hash_coreform_module must be 64-hex");
          if (mhSelf !== mhRust) throw new Error("selfhost hash must match rust hash");

          const src = \`
            (core/effect::perform
              'sys/time::now
              nil
              (fn (t) (core/effect::pure t)))
          \`;

          const rt = new mod.Runtime(0);
          const step = rt.eval_module(src);
          if (step.kind !== "effect") throw new Error("expected effect step");
          if (step.op !== "sys/time::now") throw new Error("unexpected op");
          if (step.payload !== "nil") throw new Error("unexpected payload");
          for (const k of ["module_h","payload_h","cont_h","req_h"]) {
            if (!isHex32(step[k])) throw new Error("bad hash " + k);
          }

          const resumed = rt.respond_denied();
          if (!isHex32(resumed.resp_h)) throw new Error("bad resp_h");
          if (resumed.next.kind !== "done") throw new Error("expected done");
          if (!isHex32(resumed.next.value_h)) throw new Error("bad final value hash");
          if (!String(resumed.next.value).includes("core/caps/denied")) throw new Error("expected caps denied error payload");

          const out = {
            module_h: step.module_h,
            payload_h: step.payload_h,
            cont_h: step.cont_h,
            req_h: step.req_h,
            resp_h: resumed.resp_h,
            final_value_h: resumed.next.value_h,
          };

          window.__GENESIS_WEB_SMOKE__ = { ok: true, out };
          outEl.textContent = "ok";
        } catch (e) {
          window.__GENESIS_WEB_SMOKE__ = { ok: false, error: String(e && e.stack ? e.stack : e) };
          outEl.textContent = String(e);
        }
      }

      main();
    </script>
  </body>
</html>
`;
  await writeFile(path.join(outDir, "index.html"), html, "utf8");
}

async function startStaticServer(rootDir) {
  const server = http.createServer(async (req, res) => {
    try {
      const u = new URL(req.url ?? "/", "http://localhost");
      const urlPath = u.pathname === "/" ? "/index.html" : u.pathname;
      const fullPath = mustUnderRoot(rootDir, decodeURIComponent(urlPath));
      const bytes = await readFile(fullPath);
      res.statusCode = 200;
      res.setHeader("content-type", contentTypeFor(fullPath));
      res.end(bytes);
    } catch (e) {
      res.statusCode = 404;
      res.setHeader("content-type", "text/plain; charset=utf-8");
      res.end("not found");
    }
  });

  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const addr = server.address();
  if (!addr || typeof addr === "string") throw new Error("bad server address");
  const url = `http://127.0.0.1:${addr.port}/index.html`;
  return {
    url,
    close: () =>
      new Promise((resolve, reject) => server.close((e) => (e ? reject(e) : resolve()))),
  };
}

async function main() {
  const defaultOut = path.resolve("target/wasm-bindgen-web/gc_wasm");
  const outDir = path.resolve(process.argv[2] ?? defaultOut);

  // Ensure we are running from repo root if invoked from other cwd.
  // If called via `node scripts/wasm_web_smoke.mjs`, this is already true.
  const self = fileURLToPath(import.meta.url);
  const selfDir = path.dirname(self);
  const rootDir = path.resolve(selfDir, "..");
  process.chdir(rootDir);

  await writeHarnessHtml(outDir);

  const native = runNativeEffectHashes();
  const srv = await startStaticServer(outDir);

  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.goto(srv.url, { waitUntil: "load" });
    await page.waitForFunction(
      () => typeof window.__GENESIS_WEB_SMOKE__ === "object",
      null,
      { timeout: 30_000 },
    );
    const result = await page.evaluate(() => window.__GENESIS_WEB_SMOKE__);
    if (!result || typeof result !== "object") throw new Error("missing result");
    if (!result.ok) {
      throw new Error(result.error ?? "web smoke failed");
    }
    const out = result.out;
    for (const k of Object.keys(native)) {
      assert.equal(
        out[k],
        native[k],
        `cross-host mismatch for ${k}: web=${out[k]} native=${native[k]}`,
      );
    }
  } finally {
    await browser.close();
    await srv.close();
  }

  process.stdout.write("ok\n");
}

main().catch((e) => {
  // Keep one-line failure messages for CI.
  process.stderr.write(String(e && e.stack ? e.stack : e) + "\n");
  process.exitCode = 1;
});
