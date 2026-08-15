// First-paint web audit: load a URL in headless Chromium and collect
// page errors, console errors/warnings, failed requests, HTTP >=400
// responses and WebSocket lifecycle events.
//
// Usage:
//   node audit.mjs --url http://localhost:5180/ [--wait 10000]
//                  [--timeout 30000] [--max 40] [--out report.json]
//                  [--ignore substring]...
//
// Exit code: 0 = clean (no page errors / failed requests / 4xx-5xx),
//            1 = problems found, 2 = usage error.

import { writeFileSync } from "node:fs";
import { chromium } from "playwright";

function parseArgs(argv) {
  const args = { wait: 10000, timeout: 30000, max: 40, ignore: [] };
  for (let i = 0; i < argv.length; i++) {
    const key = argv[i];
    const next = () => {
      if (i + 1 >= argv.length) usageError(`missing value for ${key}`);
      return argv[++i];
    };
    switch (key) {
      case "--url": args.url = next(); break;
      case "--wait": args.wait = Number(next()); break;
      case "--timeout": args.timeout = Number(next()); break;
      case "--max": args.max = Number(next()); break;
      case "--out": args.out = next(); break;
      case "--ignore": args.ignore.push(next()); break;
      case "--help": case "-h":
        console.log("node audit.mjs --url <url> [--wait ms] [--timeout ms] [--max n] [--out file] [--ignore substr]*");
        process.exit(0);
      default: usageError(`unknown flag ${key}`);
    }
  }
  if (!args.url) usageError("--url is required");
  return args;
}

function usageError(msg) {
  console.error(`error: ${msg}\nusage: node audit.mjs --url <url> [--wait ms] [--timeout ms] [--max n] [--out file]`);
  process.exit(2);
}

const args = parseArgs(process.argv.slice(2));

const browser = await chromium.launch();
const page = await browser.newPage();

const pageErrors = [];
const consoleMsgs = [];
const failedReqs = [];
const badResponses = [];
const websockets = [];

page.on("pageerror", (e) => pageErrors.push(String(e)));
page.on("console", (m) => {
  if (m.type() === "error" || m.type() === "warning") {
    consoleMsgs.push({ type: m.type(), text: m.text().slice(0, 500) });
  }
});
page.on("requestfailed", (r) => failedReqs.push({
  url: r.url(),
  method: r.method(),
  err: r.failure()?.errorText,
}));
page.on("response", (r) => {
  if (r.status() >= 400) {
    badResponses.push({ url: r.url(), method: r.request().method(), status: r.status() });
    // body read is best-effort; keep the promise alive explicitly
    r.body().then(
      (b) => { badResponses[badResponses.length - 1].body = b.toString("utf8").slice(0, 300); },
      () => { badResponses[badResponses.length - 1].body = "<unreadable>"; },
    );
  }
});
page.on("websocket", (ws) => {
  const entry = { url: ws.url(), framesIn: 0, framesOut: 0, closedByServer: null };
  websockets.push(entry);
  ws.on("framereceived", () => entry.framesIn++);
  ws.on("framesent", () => entry.framesOut++);
  ws.on("close", () => { entry.closedByServer = true; });
});

// NOTE: deliberately NOT waitUntil "networkidle" — long-polling and WS
// connections keep the network busy forever on SPAs. Settle explicitly.
const nav = { ok: true };
await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: args.timeout })
  .catch((e) => { nav.ok = false; nav.error = String(e).slice(0, 300); });
await page.waitForTimeout(args.wait);

const report = {
  url: args.url,
  finalUrl: page.url(),
  title: await page.title().catch(() => null),
  navigation: nav,
  pageErrors,
  consoleMsgs,
  failedReqs,
  badResponses,
  websockets,
};

await browser.close();

const dedupe = (arr, key) => {
  const m = new Map();
  for (const it of arr) m.set(key(it), it);
  return [...m.values()];
};
report.consoleMsgs = dedupe(consoleMsgs, (m) => m.text).slice(0, args.max);
report.failedReqs = dedupe(failedReqs, (r) => r.url + r.method).slice(0, args.max);
report.badResponses = dedupe(badResponses, (r) => r.url + r.method + r.status).slice(0, args.max);
report.pageErrors = pageErrors.slice(0, args.max);

// Known-gap suppression: drop matching network failures from the verdict,
// but keep a transparent count in the report.
if (args.ignore.length > 0) {
  const isIgnored = (entry) => args.ignore.some((substr) => entry.url.includes(substr));
  report.ignored = {
    badResponses: report.badResponses.filter(isIgnored).length,
    failedReqs: report.failedReqs.filter(isIgnored).length,
    patterns: args.ignore,
  };
  report.badResponses = report.badResponses.filter((e) => !isIgnored(e));
  report.failedReqs = report.failedReqs.filter((e) => !isIgnored(e));
}

if (args.out) writeFileSync(args.out, JSON.stringify(report, null, 2) + "\n");

const failed = report.pageErrors.length + report.failedReqs.length + report.badResponses.length;
const verdict = failed === 0 && nav.ok ? "PASS" : "FAIL";
console.error(
  `${verdict}: pageErrors=${report.pageErrors.length} failedReqs=${report.failedReqs.length} ` +
  `badResponses=${report.badResponses.length} consoleIssues=${report.consoleMsgs.length} ` +
  `websockets=${report.websockets.length}`,
);
console.log(JSON.stringify(report, null, 2));
process.exit(verdict === "PASS" ? 0 : 1);
