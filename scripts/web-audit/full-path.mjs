// Full user-path repro: enter app, open a real session, open terminal, watch ALL traffic 120s.
import { chromium } from "playwright";

const t0 = Date.now();
const now = () => ((Date.now() - t0) / 1000).toFixed(1);
const browser = await chromium.launch();
const ctx = await browser.newContext();
const page = await ctx.newPage();

const wsLog = [];
page.on("websocket", (ws) => {
  const e = { url: ws.url(), open: now(), close: null, framesIn: 0, framesOut: 0, methods: {} };
  wsLog.push(e);
  ws.on("framereceived", (d) => { e.framesIn++; const s = String(typeof d === "string" ? d : (d.payload ?? "")); try { const j = JSON.parse(s); const m = j.method ?? `res#${j.id}`; e.methods[m] = (e.methods[m] ?? 0) + 1; } catch {} });
  ws.on("framesent", (d) => { e.framesOut++; const s = String(typeof d === "string" ? d : (d.payload ?? "")); try { const j = JSON.parse(s); const m = j.method ?? `res#${j.id}`; e.methods[m] = (e.methods[m] ?? 0) + 1; } catch {} });
  ws.on("close", () => { e.close = now(); });
  console.log(`[${now()}s] WS OPEN ${ws.url()}`);
});
page.on("response", (r) => {
  if (r.url().includes("/api/") && r.status() >= 400) console.log(`[${now()}s] HTTP ${r.status()} ${r.request().method()} ${r.url().slice(0, 100)}`);
});
page.on("request", (r) => {
  const u = r.url();
  if (u.includes("terminal")) console.log(`[${now()}s] REQ ${r.method()} ${u.slice(0, 110)}`);
});
page.on("console", (m) => {
  const t = m.text();
  if (/error|failed|terminal|reconnect|closed|loop/i.test(t)) console.log(`[${now()}s][${m.type()}] ${t.slice(0, 200)}`);
});

await page.goto("http://localhost:5180/", { waitUntil: "domcontentloaded" });
await page.evaluate(() => {
  const dir = "C:/Users/heycj/dev/loom";
  localStorage.setItem("lastDirectory", dir);
  localStorage.setItem("directory-store", JSON.stringify({
    state: { currentDirectory: dir, directoryHistory: [dir], historyIndex: 0, homeDirectory: dir, hasPersistedDirectory: true, isHomeReady: true, isSwitchingDirectory: false },
    version: 0,
  }));
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(6000);

// Find a session in the sidebar and click it
const clicked = await page.evaluate(() => {
  const items = [...document.querySelectorAll('[data-session-id], [class*="session"] button, a[href*="session"]')];
  return items.length;
});
console.log(`[${now()}s] session-ish elements: ${clicked}`);
try {
  await page.getByText(/loom|main|dev|test|fix/i).first().click({ timeout: 4000 });
  console.log(`[${now()}s] clicked session-ish text`);
} catch (e) { console.log(`[${now()}s] click failed: ${String(e).slice(0, 120)}`); }
await page.waitForTimeout(4000);
await page.screenshot({ path: "shot-session.png" });

// Try opening terminal dock via keyboard shortcut or button
try {
  const btn = page.locator('button:has-text("Terminal"), [aria-label*="erminal"], [title*="erminal"]').first();
  if (await btn.count() > 0) { await btn.click({ timeout: 3000 }); console.log(`[${now()}s] clicked terminal button`); }
  else { await page.keyboard.press("Control+`"); console.log(`[${now()}s] pressed Ctrl+\``); }
} catch (e) { console.log(`[${now()}s] terminal open failed: ${String(e).slice(0, 100)}`); }

console.log(`[${now()}s] observing 120s ...`);
await page.waitForTimeout(120000);

console.log(`\n=== WS SUMMARY (${wsLog.length} connections) ===`);
for (const e of wsLog) {
  console.log(`${e.url}\n  open=${e.open} close=${e.close ?? "-"} in=${e.framesIn} out=${e.framesOut}`);
  const tops = Object.entries(e.methods).sort((a, b) => b[1] - a[1]).slice(0, 12);
  for (const [m, n] of tops) console.log(`    ${n}x ${m}`);
}
await page.screenshot({ path: "shot-final2.png" });
await browser.close();
