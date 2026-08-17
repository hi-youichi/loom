// Capture ACP WS frame payloads to identify duplicate traffic.
import { chromium } from "playwright";

const args = { url: "http://localhost:5180/", wait: 60000 };
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--url") args.url = argv[++i];
  else if (argv[i] === "--wait") args.wait = Number(argv[++i]);
}

const t0 = Date.now();
const now = () => ((Date.now() - t0) / 1000).toFixed(1);
const browser = await chromium.launch();
const ctx = await browser.newContext();
const page = await ctx.newPage();

const frames = [];
page.on("websocket", (ws) => {
  if (!ws.url().includes("/acp")) return;
  console.log(`[${now()}s] ACP WS opened`);
  ws.on("framereceived", (d) => {
    const s = typeof d === "string" ? d : (d.payload ?? d.data ?? JSON.stringify(d));
    frames.push({ t: now(), dir: "IN", s: String(s) });
  });
  ws.on("framesent", (d) => {
    const s = typeof d === "string" ? d : (d.payload ?? d.data ?? JSON.stringify(d));
    frames.push({ t: now(), dir: "OUT", s: String(s) });
  });
  ws.on("close", () => console.log(`[${now()}s] ACP WS closed`));
});

await page.goto(args.url, { waitUntil: "domcontentloaded" });
await page.evaluate(() => {
  const dir = "C:/Users/heycj/dev/loom";
  localStorage.setItem("lastDirectory", dir);
  localStorage.setItem("directory-store", JSON.stringify({
    state: { currentDirectory: dir, directoryHistory: [dir], historyIndex: 0, homeDirectory: dir, hasPersistedDirectory: true, isHomeReady: true, isSwitchingDirectory: false },
    version: 0,
  }));
});
await page.reload({ waitUntil: "domcontentloaded" });
console.log(`waiting ${args.wait / 1000}s ...`);
await page.waitForTimeout(args.wait);

console.log(`\n=== ACP FRAMES (${frames.length}) ===`);
const counts = {};
for (const f of frames) {
  let method = "?";
  try {
    const j = JSON.parse(f.s);
    if (j.method) method = j.method + (j.id ? `#${j.id}` : " (notif)");
    else if (j.result !== undefined) method = `result#${j.id}`;
    else if (j.error) method = `error#${j.id}`;
  } catch { method = "RAW"; }
  const key = `${f.dir} ${method.replace(/#\d+/g, "#")}`;
  counts[key] = (counts[key] ?? 0) + 1;
}
for (const [k, v] of Object.entries(counts).sort((a, b) => b[1] - a[1])) {
  console.log(`${String(v).padStart(4)}x ${k}`);
}
const fs = await import("node:fs");
fs.writeFileSync("acp-frames.json", JSON.stringify(frames, null, 2));
console.log("full frames -> acp-frames.json");
await browser.close();
