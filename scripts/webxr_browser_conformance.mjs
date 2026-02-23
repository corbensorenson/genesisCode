import crypto from "node:crypto";
import http from "node:http";
import path from "node:path";
import process from "node:process";
import { writeFile } from "node:fs/promises";

import { chromium } from "playwright";

const OUT_PATH = path.resolve(
  process.env.GENESIS_WEBXR_BROWSER_CONFORMANCE_OUT ??
    ".genesis/perf/webxr_browser_conformance_report.json",
);
const TIMEOUT_MS = Number.parseInt(
  process.env.GENESIS_WEBXR_BROWSER_CONFORMANCE_TIMEOUT_MS ?? "8000",
  10,
);

const XR_FLAGS = [
  "--enable-blink-features=WebXR,WebXRHandInput,WebXRHitTest,WebXRAnchors,WebXRLayers",
  "--enable-features=WebXR,OpenXR,WebXRIncubations",
  "--webxr-force-runtime=orientation",
  "--use-angle=swiftshader",
  "--use-gl=angle",
  "--ignore-gpu-blocklist",
];

function classifyError(message) {
  const m = String(message || "").toLowerCase();
  if (!m) return "unknown";
  if (m.includes("no xr hardware")) return "no-hardware";
  if (m.includes("not supported")) return "not-supported";
  if (m.includes("timeout")) return "timeout";
  if (m.includes("denied")) return "denied";
  return "runtime-error";
}

function canonicalize(value) {
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }
  if (value && typeof value === "object") {
    const out = {};
    for (const key of Object.keys(value).sort()) {
      out[key] = canonicalize(value[key]);
    }
    return out;
  }
  return value;
}

function hashCapture(capture) {
  const canonical = JSON.stringify(canonicalize(capture));
  return crypto.createHash("sha256").update(canonical).digest("hex");
}

async function startServer() {
  const html =
    "<!doctype html><meta charset='utf-8'><title>genesis-webxr-conformance</title><body>webxr</body>";
  const server = http.createServer((req, res) => {
    res.statusCode = 200;
    res.setHeader("content-type", "text/html; charset=utf-8");
    res.end(html);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const addr = server.address();
  if (!addr || typeof addr === "string") {
    throw new Error("webxr-conformance: invalid server address");
  }
  return {
    url: `http://127.0.0.1:${addr.port}/`,
    close: () =>
      new Promise((resolve, reject) => server.close((e) => (e ? reject(e) : resolve()))),
  };
}

async function evaluateWithTimeout(page, fn, timeoutMs, label) {
  return await Promise.race([
    page.evaluate(fn),
    new Promise((_, reject) => {
      const timer = setTimeout(
        () => reject(new Error(`webxr-conformance: ${label} timeout`)),
        timeoutMs,
      );
      timer.unref?.();
    }),
  ]);
}

async function runPass(browser, url) {
  const page = await browser.newPage();
  try {
    await page.goto(url, { waitUntil: "load" });
    return await evaluateWithTimeout(
      page,
      async () => {
        const withTimeout = async (promise, ms) => {
          return await Promise.race([
            promise
              .then((value) => ({ ok: true, value }))
              .catch((e) => ({ ok: false, error: String(e && e.message ? e.message : e) })),
            new Promise((resolve) => setTimeout(() => resolve({ ok: false, error: "timeout" }), ms)),
          ]);
        };

        const attachRenderLayer = async (session) => {
          try {
            const canvas = document.createElement("canvas");
            canvas.width = 64;
            canvas.height = 64;
            canvas.style.display = "none";
            document.body.appendChild(canvas);
            const gl2 = canvas.getContext("webgl2", {
              xrCompatible: true,
              antialias: false,
              alpha: false,
            });
            const gl =
              gl2 ||
              canvas.getContext("webgl", {
                xrCompatible: true,
                antialias: false,
                alpha: false,
              });
            if (!gl) {
              return {
                status: "error",
                context: "none",
                error_code: "webgl-context-unavailable",
              };
            }
            if (typeof gl.makeXRCompatible === "function") {
              await withTimeout(gl.makeXRCompatible(), 1000);
            }
            if (typeof XRWebGLLayer !== "function") {
              return {
                status: "error",
                context: gl2 ? "webgl2" : "webgl",
                error_code: "xr-webgl-layer-unavailable",
              };
            }
            const layer = new XRWebGLLayer(session, gl);
            session.updateRenderState({ baseLayer: layer });
            return {
              status: "ok",
              context: gl2 ? "webgl2" : "webgl",
              error_code: null,
            };
          } catch (e) {
            return {
              status: "error",
              context: "none",
              error_code: String(e && e.message ? e.message : e),
            };
          }
        };

        const probeInlineFrame = async (session) => {
          const refRes = await withTimeout(session.requestReferenceSpace("viewer"), 1200);
          if (!refRes.ok) {
            return { ok: false, reason: "reference-space-error", detail: refRes.error };
          }
          const refSpace = refRes.value;
          const frameRes = await withTimeout(
            new Promise((resolve) => {
              session.requestAnimationFrame((_, frame) => {
                const pose = frame.getViewerPose(refSpace);
                resolve({
                  status: pose && pose.views.length > 0 ? "ok" : "degraded",
                  pose: Boolean(pose),
                  view_count: pose ? pose.views.length : 0,
                });
              });
            }),
            1200,
          );
          if (!frameRes.ok) {
            return {
              ok: false,
              reason: frameRes.error === "timeout" ? "frame-timeout" : "frame-error",
              detail: frameRes.error,
            };
          }
          return { ok: frameRes.value.status === "ok", frame: frameRes.value };
        };

        const runtime = {
          secure_context: window.isSecureContext,
          user_agent: navigator.userAgent,
          navigator_xr: Boolean(navigator.xr),
        };
        if (!runtime.navigator_xr) {
          return {
            ok: false,
            runtime,
            error_code: "navigator-xr-unavailable",
          };
        }

        const supportedInline = await navigator.xr
          .isSessionSupported("inline")
          .catch(() => false);
        const supportedImmersive = await navigator.xr
          .isSessionSupported("immersive-vr")
          .catch(() => false);

        const capture = {
          runtime,
          supports: {
            inline: Boolean(supportedInline),
            immersive_vr: Boolean(supportedImmersive),
          },
          session: { status: "not-run", mode: "inline" },
          render_layer: { status: "not-run", context: "none", error_code: null },
          reference_space: { status: "not-run", type: "viewer" },
          frame: { status: "not-run", pose: false, view_count: 0 },
          input: { status: "not-run", count: 0, sources: [] },
          haptics: { status: "not-run", accepted: false, reason: "not-run" },
          session_close: { status: "not-run", mode: "explicit-end", error_code: null },
        };

        if (!supportedInline) {
          return {
            ok: false,
            capture,
            error_code: "inline-session-unsupported",
          };
        }

        const openRes = await withTimeout(navigator.xr.requestSession("inline"), 1200);
        if (!openRes.ok) {
          capture.session = {
            status: "error",
            mode: "inline",
            error_code: openRes.error === "timeout" ? "timeout" : "session-open-error",
          };
          return {
            ok: false,
            capture,
            error_code: capture.session.error_code,
          };
        }

        const session = openRes.value;
        capture.session.status = "opened";

        // Ensure XR session render state is initialized with a WebGL layer so
        // requestAnimationFrame produces real XR frames in headless lanes.
        capture.render_layer = await attachRenderLayer(session);

        const refRes = await withTimeout(session.requestReferenceSpace("viewer"), 1200);
        if (!refRes.ok) {
          capture.reference_space = {
            status: "error",
            type: "viewer",
            error_code: refRes.error === "timeout" ? "timeout" : "reference-space-error",
          };
          await withTimeout(session.end(), 800);
          capture.session_close.status = "closed";
          return { ok: false, capture, error_code: capture.reference_space.error_code };
        }

        const refSpace = refRes.value;
        capture.reference_space.status = "ok";

        const frameRes = await withTimeout(
          new Promise((resolve) => {
            session.requestAnimationFrame((_, frame) => {
              const pose = frame.getViewerPose(refSpace);
              resolve({
                status: pose && pose.views.length > 0 ? "ok" : "degraded",
                pose: Boolean(pose),
                view_count: pose ? pose.views.length : 0,
              });
            });
          }),
          1500,
        );
        if (frameRes.ok) {
          capture.frame = frameRes.value;
        } else {
          capture.frame = {
            status: frameRes.error === "timeout" ? "timeout" : "error",
            pose: false,
            view_count: 0,
          };
        }

        const sources = Array.from(session.inputSources).map((source) => ({
          handedness: source.handedness || "",
          target_ray_mode: source.targetRayMode || "",
          has_haptics: Boolean(
            source.gamepad &&
              Array.isArray(source.gamepad.hapticActuators) &&
              source.gamepad.hapticActuators.length > 0,
          ),
        }));
        sources.sort((a, b) =>
          `${a.handedness}:${a.target_ray_mode}`.localeCompare(`${b.handedness}:${b.target_ray_mode}`),
        );
        capture.input = {
          status: "ok",
          count: sources.length,
          sources,
        };

        const hapticSource = Array.from(session.inputSources).find(
          (source) =>
            source.gamepad &&
            Array.isArray(source.gamepad.hapticActuators) &&
            source.gamepad.hapticActuators.length > 0,
        );
        if (!hapticSource) {
          capture.haptics = {
            status: "ok",
            accepted: false,
            reason: "no-haptics-source",
          };
        } else {
          const pulseRes = await withTimeout(hapticSource.gamepad.hapticActuators[0].pulse(0.2, 20), 500);
          if (!pulseRes.ok) {
            capture.haptics = {
              status: "error",
              accepted: false,
              reason: pulseRes.error === "timeout" ? "timeout" : "pulse-error",
            };
          } else {
            capture.haptics = {
              status: "ok",
              accepted: Boolean(pulseRes.value),
              reason: pulseRes.value ? "pulse-accepted" : "pulse-rejected",
            };
          }
        }

        const closeRes = await withTimeout(session.end(), 800);
        if (closeRes.ok) {
          capture.session_close = {
            status: "closed",
            mode: "explicit-end",
            error_code: null,
          };
        } else if (closeRes.error === "timeout") {
          // Chromium orientation runtime can leave `session.end()` unresolved while
          // still quiescing session frame delivery. Treat close as functional only
          // when quiescence is observed and a new inline session can be opened + framed.
          const oldSessionFrameRes = await withTimeout(
            new Promise((resolve) => {
              session.requestAnimationFrame(() => resolve({ fired: true }));
            }),
            800,
          );
          const oldSessionQuiesced = !oldSessionFrameRes.ok;

          const reopenRes = await withTimeout(navigator.xr.requestSession("inline"), 1200);
          let reopenFrameOk = false;
          if (reopenRes.ok) {
            const reopened = reopenRes.value;
            await attachRenderLayer(reopened);
            const probe = await probeInlineFrame(reopened);
            reopenFrameOk = Boolean(probe.ok);
            await withTimeout(reopened.end(), 400);
          }
          capture.session_close =
            oldSessionQuiesced && reopenRes.ok && reopenFrameOk
              ? {
                  status: "closed-quiesced",
                  mode: "explicit-end-timeout-recovery",
                  error_code: null,
                }
              : {
                  status: "timeout",
                  mode: "explicit-end",
                  error_code: "end-timeout",
                };
        } else {
          capture.session_close = {
            status: "error",
            mode: "explicit-end",
            error_code: "end-error",
          };
        }

        return {
          ok: true,
          capture,
          functional_pass:
            capture.frame.status === "ok" &&
            (capture.session_close.status === "closed" ||
              capture.session_close.status === "closed-quiesced"),
        };
      },
      TIMEOUT_MS,
      "browser pass",
    );
  } finally {
    await page.close();
  }
}

async function main() {
  const server = await startServer();
  const browser = await chromium.launch({ headless: true, args: XR_FLAGS });
  try {
    const runA = await runPass(browser, server.url);
    const runB = await runPass(browser, server.url);

    if (!runA.ok || !runB.ok) {
      const report = {
        kind: "genesis/webxr-browser-conformance-v0.1",
        ok: false,
        failure: {
          run_a: runA,
          run_b: runB,
          classified_error: classifyError(
            runA?.error_code || runB?.error_code || runA?.error || runB?.error || "",
          ),
        },
      };
      await writeFile(OUT_PATH, JSON.stringify(report, null, 2) + "\n", "utf8");
      throw new Error("webxr-conformance: browser runtime pass failed");
    }

    const hashA = hashCapture(runA.capture);
    const hashB = hashCapture(runB.capture);
    const deterministic = hashA === hashB;
    const functionalPass =
      Boolean(runA.functional_pass) && Boolean(runB.functional_pass);
    const report = {
      kind: "genesis/webxr-browser-conformance-v0.1",
      ok: deterministic,
      functional_pass: functionalPass,
      replay_rule: "capture_hash(run_a)==capture_hash(run_b)",
      run_a_hash: hashA,
      run_b_hash: hashB,
      deterministic_replay: deterministic,
      run_a_capture: runA.capture,
      run_b_capture: runB.capture,
      browser_flags: XR_FLAGS,
    };
    await writeFile(OUT_PATH, JSON.stringify(report, null, 2) + "\n", "utf8");
    if (!deterministic) {
      throw new Error("webxr-conformance: capture hash mismatch");
    }
    process.stdout.write(`webxr-conformance: ok report=${OUT_PATH}\n`);
  } finally {
    await browser.close();
    await server.close();
  }
}

main().catch((e) => {
  process.stderr.write(String(e && e.stack ? e.stack : e) + "\n");
  process.exitCode = 1;
});
