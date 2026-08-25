// Seed persisted UI state (like the user's browser), enter app, capture WS churn.
import { chromium } from "playwright";

const args = { url: "http://localhost:5180/", wait: 30000, out: null };
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--url") args.url = argv[++i];
  else if (argv[i] === "--wait") args.wait = Number(argv[++i]);
  else if (argv[i] === "--out") args.out = argv[++i];
}

const t0 = Date.now();
const now = () => ((Date.now() - t0) / 1000).toFixed(1) + "s";
const browser = await chromium.launch();
const ctx = await browser.newContext();
const page = await ctx.newPage();

const wsLog = [];
page.on("websocket", (ws) => {
  const id = wsLog.length;
  const entry = { id, url: ws.url(), open: now(), framesIn: 0, framesOut: 0, close: null };
  wsLog.push(entry);
  ws.on("framereceived", () => { entry.framesIn++; });
  ws.on("framesent", () => { entry.framesOut++; });
  ws.on("close", () => { entry.close = now(); });
  console.log(`[ws] #${id} OPEN ${ws.url()}`);
});
page.on("console", (m) => {
  const text = m.text();
  if (/websocket|terminal|reconnect|error|failed|loop|pipe|acp/i.test(text)) {
    console.log(`[console:${m.type()}] ${text.slice(0, 400)}`);
  }
});
page.on("pageerror", (e) => console.log(`[pageerror] ${String(e).slice(0, 300)}`));

console.log("seeding localStorage...");
await page.goto(args.url, { waitUntil: "domcontentloaded" });
await page.evaluate(() => {
  const dir = "C:/Users/heycj/dev/anureo";
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
    state: {
      sessions: [[dir, { activeTabId: "tab-1", tabs: [{ id: "tab-1", label: "Terminal", iconKey: null, terminalSessionId: null, lifecycle: "idle", createdAt: Date.now() }] }]],
      nextTabId: 2,
    },
    version: 0,
  }));
});

console.log("reloading with seeded state...");
await page.reload({ waitUntil: "domcontentloaded" });
console.log(`waiting ${args.wait}ms ...`);
await page.waitForTimeout(args.wait);
await page.screenshot({ path: "shot-seeded.png" });

console.log("\n=== WS SUMMARY ===");
for (const e of wsLog) {
  console.log(`#${e.id} ${e.url} open=${e.open} close=${e.close ?? "-"} in=${e.framesIn} out=${e.framesOut}`);
}
console.log(`total WS connections: ${wsLog.length}`);
const byUrl = {};
for (const e of wsLog) byUrl[e.url] = (byUrl[e.url] ?? 0) + 1;
console.log("by url:", JSON.stringify(byUrl, null, 2));
if (args.out) {
  const fs = await import("node:fs");
  fs.writeFileSync(args.out, JSON.stringify({ wsLog }, null, 2));
}
await browser.close();
