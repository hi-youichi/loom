// Enter OpenChamber with seeded state; log ALL network traffic for N seconds.
import { chromium } from "playwright";

const args = { url: "http://localhost:5180/", wait: 45000 };
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--url") args.url = argv[++i];
  else if (argv[i] === "--wait") args.wait = Number(argv[++i]);
}

const t0 = Date.now();
const now = () => ((Date.now() - t0) / 1000).toFixed(1) + "s";
const browser = await chromium.launch();
const ctx = await browser.newContext();
const page = await ctx.newPage();

const reqs = [];
const wsLog = [];
page.on("request", (r) => {
  reqs.push({ t: now(), method: r.method(), url: r.url() });
});
page.on("response", (r) => {
  if (r.status() >= 400) {
    const e = reqs.findLast((x) => x.url === r.url() && x.status === undefined);
    if (e) e.status = r.status();
  }
});
page.on("requestfailed", (r) => {
  const e = reqs.findLast((x) => x.url === r.url() && x.status === undefined);
  if (e) e.status = "FAIL:" + (r.failure()?.errorText ?? "?");
});
page.on("websocket", (ws) => {
  const entry = { url: ws.url(), open: now(), framesIn: 0, framesOut: 0, close: null };
  wsLog.push(entry);
  ws.on("framereceived", () => entry.framesIn++);
  ws.on("framesent", () => entry.framesOut++);
  ws.on("close", () => entry.close = now());
});
page.on("console", (m) => {
  const text = m.text();
  if (/terminal|reconnect|error|failed|acp|websocket/i.test(text)) {
    console.log(`[${now()}][console:${m.type()}] ${text.slice(0, 300)}`);
  }
});

await page.goto(args.url, { waitUntil: "domcontentloaded" });
await page.evaluate(() => {
  const dir = "C:/Users/heycj/dev/loom";
  localStorage.setItem("lastDirectory", dir);
  localStorage.setItem("directory-store", JSON.stringify({
    state: { currentDirectory: dir, directoryHistory: [dir], historyIndex: 0, homeDirectory: dir, hasPersistedDirectory: true, isHomeReady: true, isSwitchingDirectory: false },
    version: 0,
  }));
  localStorage.setItem("ui-store", JSON.stringify({
    state: { isBottomTerminalOpen: true, activeMainTab: "terminal", isSidebarOpen: true },
    version: 10,
  }));
  sessionStorage.setItem("terminal-store", JSON.stringify({
    state: { sessions: [[dir, { activeTabId: "tab-1", tabs: [{ id: "tab-1", label: "Terminal", iconKey: null, terminalSessionId: null, lifecycle: "idle", createdAt: Date.now() }] }]], nextTabId: 2 },
    version: 0,
  }));
});
console.log("reload + observe...");
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(args.wait);

// Aggregate
const byUrl = {};
for (const r of reqs) {
  const key = `${r.method} ${new URL(r.url).pathname}`;
  byUrl[key] ??= { n: 0, statuses: new Set() };
  byUrl[key].n++;
  if (r.status !== undefined) byUrl[key].statuses.add(String(r.status));
}
console.log(`\n=== REQUESTS (${reqs.length} total in ${args.wait / 1000}s) ===`);
for (const [k, v] of Object.entries(byUrl).sort((a, b) => b[1].n - a[1].n)) {
  console.log(`${String(v.n).padStart(4)}x ${k}  [${[...v.statuses].join(",")}]`);
}
console.log(`\n=== WS (${wsLog.length}) ===`);
for (const e of wsLog) console.log(`${e.url} open=${e.open} close=${e.close ?? "-"} in=${e.framesIn} out=${e.framesOut}`);
await browser.close();
