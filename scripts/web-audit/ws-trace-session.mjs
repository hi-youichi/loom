// Deep WS trace for a specific session URL: lifecycle, frames, errors, reconnect loops.
// Usage: node ws-trace-session.mjs --url "http://localhost:5180/?session=..." [--wait 15000] [--max-frame 600]
import { chromium } from "playwright";

const args = { url: "http://localhost:5180/", wait: 15000, maxFrame: 600, out: "ws-trace-session.json" };
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--url") args.url = argv[++i];
  else if (argv[i] === "--wait") args.wait = Number(argv[++i]);
  else if (argv[i] === "--max-frame") args.maxFrame = Number(argv[++i]);
  else if (argv[i] === "--out") args.out = argv[++i];
}

const t0 = Date.now();
const now = () => ((Date.now() - t0) / 1000).toFixed(2);
const browser = await chromium.launch();
const page = await browser.newPage();
await page.addInitScript(() => {
  for (const method of ["pushState", "replaceState"]) {
    const original = history[method].bind(history);
    history[method] = (...args) => {
      console.debug(`[ws-audit] history.${method}`, String(args[2] ?? ""));
      return original(...args);
    };
  }
});

const conns = [];
const frames = [];
page.on("websocket", (ws) => {
  const id = conns.length;
  const entry = { id, url: ws.url(), openT: now(), closeT: null, framesIn: 0, framesOut: 0, closeCode: null };
  conns.push(entry);
  console.log(`[${now()}] WS#${id} OPEN ${ws.url()}`);
  ws.on("framereceived", (d) => {
    entry.framesIn++;
    const s = typeof d === "string" ? d : (d.payload ?? d.data ?? "");
    frames.push({ ws: id, t: now(), dir: "IN", len: String(s).length, s: String(s).slice(0, args.maxFrame) });
    let method = "?";
    try {
      const j = JSON.parse(s);
      if (j.method) method = j.method + (j.id !== undefined ? `#${j.id}` : " (notif)");
      else if (j.result !== undefined) method = `result#${j.id}`;
      else if (j.error) method = `error#${j.id} ${JSON.stringify(j.error).slice(0, 200)}`;
    } catch { method = "RAW"; }
    console.log(`[${now()}] WS#${id} <<IN  ${method} ${String(s).length}b`);
  });
  ws.on("framesent", (d) => {
    entry.framesOut++;
    const s = typeof d === "string" ? d : (d.payload ?? d.data ?? "");
    frames.push({ ws: id, t: now(), dir: "OUT", len: String(s).length, s: String(s).slice(0, args.maxFrame) });
    let method = "?";
    try {
      const j = JSON.parse(s);
      if (j.method) method = j.method + (j.id !== undefined ? `#${j.id}` : " (notif)");
      else if (j.result !== undefined) method = `result#${j.id}`;
      else if (j.error) method = `error#${j.id} ${JSON.stringify(j.error).slice(0, 200)}`;
    } catch { method = "RAW"; }
    console.log(`[${now()}] WS#${id}  OUT>> ${method} ${String(s).length}b`);
  });
  ws.on("close", (e) => {
    entry.closeT = now();
    entry.closeCode = e?.code ?? "?";
    console.log(`[${now()}] WS#${id} CLOSED code=${entry.closeCode}`);
  });
  ws.on("socketerror", (e) => console.log(`[${now()}] WS#${id} SOCKERR ${String(e).slice(0, 200)}`));
});
page.on("console", (m) => {
  const text = m.text();
  if (/websocket|acp|session|reconnect|error|failed|loop|timeout|abort/i.test(text)) {
    console.log(`[${now()}] [console:${m.type()}] ${text.slice(0, 400)}`);
  }
});
page.on("pageerror", (e) => console.log(`[${now()}] [pageerror] ${String(e).slice(0, 400)}`));
page.on("framenavigated", (frame) => {
  if (frame === page.mainFrame()) console.log(`[${now()}] NAV ${frame.url()}`);
});
page.on("requestfailed", (r) => {
  if (/api|acp|ws/i.test(r.url())) console.log(`[${now()}] [reqfailed] ${r.method()} ${r.url()} ${r.failure()?.errorText}`);
});
page.on("request", (r) => {
  if (r.isNavigationRequest() && r.frame() === page.mainFrame()) {
    console.log(`[${now()}] DOC ${r.method()} ${r.url()}`);
  }
});
page.on("response", (r) => {
  if (r.status() >= 400 && /api|acp/i.test(r.url())) console.log(`[${now()}] [http${r.status()}] ${r.url()}`);
});

console.log(`goto ${args.url}`);
await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: 30000 });
await page.waitForTimeout(3000);
console.log(`title: ${await page.title()}`);
console.log(`location: ${page.url()}`);
await page.screenshot({ path: "ws-trace-session-shot.png" });

console.log(`waiting ${args.wait}ms ...`);
await page.waitForTimeout(args.wait);
const startupTrace = await page.evaluate(() => window.__ANUREO_STARTUP_TRACE__ ?? []);
if (startupTrace.length > 0) {
  console.log("\n=== STARTUP TRACE ===");
  console.log(JSON.stringify(startupTrace, null, 2));
}

console.log(`\n=== SUMMARY ===`);
for (const e of conns) {
  console.log(`WS#${e.id} ${e.url}\n  open=${e.openT}s close=${e.closeT ?? "-"} code=${e.closeCode ?? "-"} in=${e.framesIn} out=${e.framesOut}`);
}
const byUrl = {};
for (const e of conns) byUrl[e.url.replace(/[?&]session=[^&]*/, "")] = (byUrl[e.url] ?? 0) + 1;
console.log("connections by url:", JSON.stringify(byUrl, null, 2));

// anomalies
const anomalies = [];
for (const e of conns) {
  if (e.framesIn === 0) anomalies.push(`WS#${e.id} zero frames IN`);
  if (e.closeT !== null && (now() - e.closeT) < 1) anomalies.push(`WS#${e.id} closed at ${e.closeT}s`);
}
const urlCounts = {};
for (const e of conns) { const k = e.url.split("?")[0]; urlCounts[k] = (urlCounts[k] ?? 0) + 1; }
for (const [u, c] of Object.entries(urlCounts)) if (c > 2) anomalies.push(`${c}x connections to ${u} (possible reconnect loop)`);
const errFrames = frames.filter((f) => /"error"/.test(f.s));
for (const f of errFrames.slice(0, 10)) anomalies.push(`WS#${f.ws} ${f.dir} error frame: ${f.s.slice(0, 300)}`);
console.log("\n=== ANOMALIES ===");
console.log(anomalies.length ? anomalies.join("\n") : "none");

const fs = await import("node:fs");
fs.writeFileSync(args.out, JSON.stringify({ conns, frames, startupTrace }, null, 2));
console.log(`\nfull frames (${frames.length}) -> ${args.out}`);
await browser.close();
