#!/usr/bin/env node
"use strict";

import assert from "node:assert/strict";
import { createServer } from "node:http";
import path from "node:path";
import process from "node:process";
import { readFile, stat } from "node:fs/promises";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SITE = path.join(ROOT, "_site");
const PREFIX = "/genesisCode/";

const CONTENT_TYPES = new Map([
  [".css", "text/css; charset=utf-8"],
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".png", "image/png"],
  [".svg", "image/svg+xml"],
  [".txt", "text/plain; charset=utf-8"],
  [".xml", "application/xml; charset=utf-8"],
]);

function resolveSitePath(rawPath) {
  let pathname = decodeURIComponent(rawPath);
  if (pathname === "/genesisCode") pathname = PREFIX;
  if (pathname.startsWith(PREFIX)) pathname = pathname.slice(PREFIX.length);
  else pathname = pathname.replace(/^\/+/, "");
  if (!pathname || pathname.endsWith("/")) pathname += "index.html";

  const resolved = path.resolve(SITE, pathname);
  if (resolved !== SITE && !resolved.startsWith(`${SITE}${path.sep}`)) {
    throw new Error("path traversal");
  }
  return resolved;
}

async function startServer() {
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url ?? "/", "http://127.0.0.1");
      let target = resolveSitePath(url.pathname);
      if ((await stat(target)).isDirectory()) target = path.join(target, "index.html");
      const bytes = await readFile(target);
      response.writeHead(200, {
        "cache-control": "no-store",
        "content-type": CONTENT_TYPES.get(path.extname(target)) ?? "application/octet-stream",
      });
      response.end(bytes);
    } catch {
      const fallback = await readFile(path.join(SITE, "404.html"));
      response.writeHead(404, { "content-type": "text/html; charset=utf-8" });
      response.end(fallback);
    }
  });

  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert(address && typeof address === "object");
  return { server, base: `http://127.0.0.1:${address.port}${PREFIX}` };
}

function collectRuntimeFailures(page) {
  const failures = [];
  page.on("pageerror", (error) => failures.push(`pageerror: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error") failures.push(`console: ${message.text()}`);
  });
  return failures;
}

async function inspectPage(page, url, { home = false } = {}) {
  const response = await page.goto(url, { waitUntil: "networkidle" });
  assert.equal(response?.status(), 200, `${url} must return HTTP 200`);

  const result = await page.evaluate(({ expectHome }) => {
    const visible = (element) => {
      const style = getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return style.display !== "none" && style.visibility !== "hidden" && rect.width > 0 && rect.height > 0;
    };
    const main = document.querySelector("main");
    const h1s = [...document.querySelectorAll("h1")].filter(visible);
    const imagesWithoutAlt = [...document.querySelectorAll("img")]
      .filter((image) => !image.hasAttribute("alt"));
    const unnamedButtons = [...document.querySelectorAll("button")]
      .filter((button) => !button.textContent.trim() && !button.getAttribute("aria-label") && !button.title);
    const hero = document.querySelector(".hero");
    const primaryAction = document.querySelector(".hero-actions a");
    const overflowElement = [...document.querySelectorAll("body *")]
      .map((element) => ({
        element,
        rect: element.getBoundingClientRect(),
      }))
      .filter(({ rect }) => rect.right > innerWidth + 1 || rect.left < -1)
      .sort((left, right) => right.rect.right - left.rect.right)[0];

    return {
      documentWidth: document.documentElement.scrollWidth,
      viewportWidth: innerWidth,
      mainVisible: Boolean(main && visible(main)),
      mainWidth: main?.getBoundingClientRect().width ?? 0,
      h1Count: h1s.length,
      title: document.title,
      imagesWithoutAlt: imagesWithoutAlt.length,
      unnamedButtons: unnamedButtons.length,
      hasCanonical: Boolean(document.querySelector('link[rel="canonical"]')),
      hasDescription: Boolean(document.querySelector('meta[name="description"]')),
      hasAgentIndex: Boolean(document.querySelector('link[rel="alternate"][type="text/plain"]')),
      socialImage: document.querySelector('meta[property="og:image"]')?.content ?? "",
      heroVisible: !expectHome || Boolean(hero && visible(hero)),
      primaryActionHeight: primaryAction?.getBoundingClientRect().height ?? 0,
      stylesheets: document.styleSheets.length,
      overflowElement: overflowElement ? {
        tag: overflowElement.element.tagName,
        id: overflowElement.element.id,
        className: String(overflowElement.element.className).slice(0, 100),
        parent: `${overflowElement.element.parentElement?.tagName ?? ""}.${String(overflowElement.element.parentElement?.className ?? "").slice(0, 80)}`,
        text: overflowElement.element.textContent.trim().slice(0, 120),
        left: overflowElement.rect.left,
        right: overflowElement.rect.right,
        width: overflowElement.rect.width,
      } : null,
    };
  }, { expectHome: home });

  assert(
    result.documentWidth <= result.viewportWidth + 1,
    `${url} has horizontal overflow (${result.documentWidth}px > ${result.viewportWidth}px; ` +
      `stylesheets=${result.stylesheets}; offender=${JSON.stringify(result.overflowElement)})`,
  );
  assert(result.mainVisible && result.mainWidth > 0, `${url} main content is not visible`);
  assert.equal(result.h1Count, 1, `${url} must expose exactly one visible h1`);
  assert(home || (result.title && result.title !== "GenesisCode"), `${url} needs a page-specific title`);
  assert.equal(result.imagesWithoutAlt, 0, `${url} has images without alt attributes`);
  assert.equal(result.unnamedButtons, 0, `${url} has unnamed buttons`);
  assert(result.hasCanonical, `${url} is missing a canonical URL`);
  assert(result.hasDescription, `${url} is missing a description`);
  assert(result.hasAgentIndex, `${url} is missing the llms.txt alternate link`);
  assert(result.heroVisible, `${url} home hero is not visible`);
  if (home) {
    assert(result.primaryActionHeight >= 44, `${url} primary action is below the 44px touch target floor`);
    assert(result.socialImage.endsWith("/site_assets/genesis-social-card.png"), `${url} social card is missing`);
  }
}

async function testViewport(browser, base, profile) {
  const context = await browser.newContext({ viewport: profile.viewport });
  const page = await context.newPage();
  const failures = collectRuntimeFailures(page);

  await inspectPage(page, base, { home: true });
  await inspectPage(page, `${base}learn/documentation-map.html`);
  await inspectPage(page, `${base}learn/quickstart.html`);
  await inspectPage(page, `${base}reference/index.html`);

  await page.goto(base, { waitUntil: "networkidle" });
  await page.keyboard.press("Tab");
  const skipState = await page.evaluate(() => {
    const active = document.activeElement;
    const rect = active?.getBoundingClientRect();
    return {
      className: active?.className ?? "",
      visibleTop: rect?.top ?? -1,
      hash: active instanceof HTMLAnchorElement ? new URL(active.href).hash : "",
    };
  });
  assert(String(skipState.className).includes("gc-skip-link"), `${profile.name}: skip link is not first`);
  assert(skipState.visibleTop >= 0, `${profile.name}: focused skip link remains offscreen`);
  assert.equal(skipState.hash, "#quarto-document-content", `${profile.name}: skip target drift`);
  const skipLink = page.locator(".gc-skip-link");
  assert.equal(await skipLink.count(), 1, `${profile.name}: skip link must be unique`);
  await skipLink.press("Enter");
  await page.waitForTimeout(50);
  assert.equal(new URL(page.url()).hash, "#quarto-document-content", `${profile.name}: skip link did not navigate`);
  const focusedId = await page.evaluate(() => document.activeElement?.id ?? "");
  assert.equal(focusedId, "quarto-document-content", `${profile.name}: skip link did not focus main content`);

  await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
  await page.waitForTimeout(80);
  const topButton = page.locator(".gc-to-top");
  await topButton.waitFor({ state: "visible" });
  assert(await topButton.isEnabled(), `${profile.name}: back-to-top control is disabled`);
  await topButton.click();
  await page.waitForFunction(() => scrollY <= 1, undefined, { timeout: 2_500 });

  assert.deepEqual(failures, [], `${profile.name}: browser runtime failures`);
  await context.close();
}

async function testSearch(browser, base) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  const failures = collectRuntimeFailures(page);
  await page.goto(base, { waitUntil: "networkidle" });

  const searchButton = page.locator('button[title="Search"]');
  assert.equal(await searchButton.count(), 1, "search trigger must be unique");
  await searchButton.click();
  const overlay = page.locator(".aa-DetachedOverlay");
  await overlay.waitFor({ state: "visible" });
  const input = page.locator(".aa-Input");
  assert.equal(await input.count(), 1, "search input must be unique");
  await input.fill("caps/denied");
  await page.waitForTimeout(150);
  assert.equal(await input.inputValue(), "caps/denied", "search query was not retained");
  await page.keyboard.press("Escape");
  await overlay.waitFor({ state: "hidden" });

  assert.deepEqual(failures, [], "search browser runtime failures");
  await context.close();
}

async function testReducedMotion(browser, base) {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  const failures = collectRuntimeFailures(page);
  await page.goto(base, { waitUntil: "networkidle" });
  const durations = await page.evaluate(() => {
    const hero = getComputedStyle(document.querySelector(".hero"));
    const card = getComputedStyle(document.querySelector(".path-card"));
    return [hero.animationDuration, hero.transitionDuration, card.animationDuration, card.transitionDuration];
  });
  const seconds = durations.map((value) => Number.parseFloat(value));
  assert(seconds.every((value) => Number.isFinite(value) && value <= 0.01), "reduced motion is not honored");
  assert.deepEqual(failures, [], "reduced-motion browser runtime failures");
  await context.close();
}

async function main() {
  await stat(path.join(SITE, "index.html"));
  const { server, base } = await startServer();
  const browser = await chromium.launch({ headless: true });
  const profiles = [
    { name: "desktop", viewport: { width: 1440, height: 900 } },
    { name: "tablet", viewport: { width: 820, height: 1180 } },
    { name: "mobile", viewport: { width: 390, height: 844 } },
  ];

  try {
    for (const profile of profiles) await testViewport(browser, base, profile);
    await testSearch(browser, base);
    await testReducedMotion(browser, base);
    console.log(`quarto-browser: ok (profiles=${profiles.length} pages=${profiles.length * 4} search=1)`);
  } finally {
    await browser.close();
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  }
}

await main();
