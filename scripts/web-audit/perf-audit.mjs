// Performance audit: capture per-request waterfall + paint metrics for a URL.
// Usage: node perf-audit.mjs --url http://localhost:5180/ [--wait 10000]

import { parseArgs } from "./perf-args.mjs";

const args = parseArgs(process.argv.slice(2));
if (!args.url) {
  console.error("error: --url is required");
  process.exit(2);
}

const { chromium } = await import("playwright");
const browser = await chromium.launch();
const page = await browser.newPage();

const reqs = [];
const byUrl = new Map();

page.on("request", (r) => {
  const e = { url: r.url(), method: r.method(), start: Date.now(), end: null, status: null, type: r.resourceType() };
  reqs.push(e);
  byUrl.set(r, e);
});
page.on("response", (r) => {
  const e = byUrl.get(r.request());
  if (e) { e.end = Date.now(); e.status = r.status(); }
});
page.on("requestfailed", (r) => {
  const e = byUrl.get(r);
  if (e) { e.end = Date.now(); e.status = "FAIL"; }
});

const t0 = Date.now();
await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: args.timeout })
  .catch(() => {});
await page.waitForTimeout(args.wait);

// Paint + navigation timing from the page itself.
const perf = await page.evaluate(() => {
  const nav = performance.getEntriesByType("navigation")[0];
  const paint = performance.getEntriesByType("paint").map((p) => ({ name: p.name, t: Math.round(p.startTime) }));
  let lcp = null;
  try {
    const es = performance.getEntriesByType("largest-contentful-paint");
    if (es.length) lcp = Math.round(es[es.length - 1].startTime);
  } catch {}
  return {
    domContentLoaded: nav ? Math.round(nav.domContentLoadedEventEnd) : null,
    loadEvent: nav ? Math.round(nav.loadEventEnd) : null,
    responseStart: nav ? Math.round(nav.responseStart) : null,
    paint, lcp,
  };
}).catch(() => null);

await browser.close();

const t1 = Date.now();
const total = t1 - t0;
const done = reqs.filter((r) => r.end !== null);
const pending = reqs.length - done.length;

// Aggregate slowest requests (excluding playwright data URLs).
const slow = done
  .filter((r) => !r.url.startsWith("data:"))
  .map((r) => ({ ...r, ms: r.end - r.start }))
  .sort((a, b) => b.ms - a.ms)
  .slice(0, 25);

const lastEnd = done.reduce((m, r) => Math.max(m, r.end - t0), 0);

const byType = {};
for (const r of done) {
  const key = r.type || "other";
  byType[key] = (byType[key] || 0) + (r.end - r.start);
}

console.log(JSON.stringify({
  url: args.url,
  wallMs: total,
  lastRequestSettledAtMs: lastEnd,
  requestCount: reqs.length,
  pendingAtCutoff: pending,
  perf,
  slowestRequests: slow.map((r) => ({ ms: r.ms, status: r.status, method: r.method, type: r.type, url: r.url.slice(0, 160) })),
  totalMsByType: byType,
}, null, 2));
