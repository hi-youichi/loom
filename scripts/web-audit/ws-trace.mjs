// Navigate into OpenChamber like a user, capture WS churn with timings.
// Usage: node ws-trace.mjs [--url http://localhost:5180/] [--wait 20000] [--click <text>]
import { chromium } from "playwright";

const args = { url: "http://localhost:5180/", wait: 20000, click: null, out: null };
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--url") args.url = argv[++i];
  else if (argv[i] === "--wait") args.wait = Number(argv[++i]);
  else if (argv[i] === "--click") args.click = argv[++i];
  else if (argv[i] === "--out") args.out = argv[++i];
}

const t0 = Date.now();
const now = () => ((Date.now() - t0) / 1000).toFixed(1) + "s";
const browser = await chromium.launch();
const page = await browser.newPage();

const wsLog = [];
page.on("websocket", (ws) => {
  const id = wsLog.length;
  const entry = { id, url: ws.url(), open: now(), framesIn: 0, framesOut: 0, close: null, closedByServer: null };
  wsLog.push(entry);
  ws.on("framereceived", (d) => { entry.framesIn++; });
  ws.on("framesent", (d) => { entry.framesOut++; });
  ws.on("close", () => { entry.close = now(); entry.closedByServer = true; });
  console.log(`[ws] #${id} OPEN ${ws.url()}`);
});
page.on("console", (m) => {
  const text = m.text();
  if (/websocket|terminal|reconnect|error|failed|loop|pipe/i.test(text)) {
    console.log(`[console:${m.type()}] ${text.slice(0, 300)}`);
  }
});
page.on("pageerror", (e) => console.log(`[pageerror] ${String(e).slice(0, 300)}`));

console.log(`goto ${args.url}`);
await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: 30000 });
await page.waitForTimeout(5000);
console.log(`title: ${await page.title()}`);
await page.screenshot({ path: "shot-landing.png" });

console.log("\n=== LINKS/BUTTONS ===");
const els = await page.evaluate(() => {
  const items = [];
  for (const a of document.querySelectorAll("a[href]")) {
    const t = (a.textContent || "").trim().slice(0, 60);
    if (t) items.push({ tag: "a", text: t, href: a.getAttribute("href") });
  }
  for (const b of document.querySelectorAll("button")) {
    const t = (b.textContent || "").trim().slice(0, 60);
    if (t) items.push({ tag: "button", text: t, href: null });
  }
  return items.slice(0, 60);
});
for (const e of els) console.log(`${e.tag}: "${e.text}" ${e.href ?? ""}`);
console.log(`location: ${page.url()}`);

if (args.click) {
  console.log(`clicking text=${args.click}`);
  try {
    await page.getByText(args.click).first().click({ timeout: 5000 });
  } catch (e) {
    console.log(`click failed: ${String(e).slice(0, 200)}`);
  }
  await page.screenshot({ path: "shot-after-click.png" });
}

console.log(`waiting ${args.wait}ms ...`);
await page.waitForTimeout(args.wait);
await page.screenshot({ path: "shot-final.png" });

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
