// Tail analysis: which requests settle after T+{threshold}, grouped by first-load vs dynamic-import.
// Usage: node perf-tail.mjs --url http://localhost:5180/ --wait 12000 [--after 1000]

import { parseArgs } from "./perf-args.mjs";

const args = parseArgs(process.argv.slice(2));
if (!args.url) { console.error("--url required"); process.exit(2); }
const afterMs = args.after ?? 1000;

const { chromium } = await import("playwright");
const browser = await chromium.launch();
const page = await browser.newPage();

const reqs = [];
const byReq = new Map();
page.on("request", (r) => {
  const e = { url: r.url(), start: Date.now(), end: null, status: null, initiator: null };
  reqs.push(e); byReq.set(r, e);
});
page.on("response", (r) => { const e = byReq.get(r.request()); if (e) { e.end = Date.now(); e.status = r.status(); } });

const t0 = Date.now();
await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: args.timeout }).catch(() => {});
await page.waitForTimeout(args.wait);
await browser.close();

const late = reqs.filter((r) => r.end !== null && (r.end - t0) > afterMs);
const fmt = (r) => ({ startAt: r.start - t0, endAt: r.end - t0, ms: r.end - r.start, status: r.status, url: r.url.replace("http://localhost:5180", "").slice(0, 130) });
console.log(JSON.stringify({
  totalReqs: reqs.length,
  lateThresholdMs: afterMs,
  lateCount: late.length,
  lateReqs: late.sort((a, b) => a.start - b.start).map(fmt),
}, null, 2));
