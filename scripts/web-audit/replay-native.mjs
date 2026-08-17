// Validate ACP-native rendering via history replay of a real session.
import { chromium } from "playwright";

const SESSION = process.argv[2] ?? "session-d1e074c4-3676-425f-b3fd-ee5160d8e0a6";
const t0 = Date.now();
const now = () => ((Date.now() - t0) / 1000).toFixed(1);
const browser = await chromium.launch();
const page = await (await browser.newContext()).newPage();

page.on("console", (m) => {
  const t = m.text();
  if (/error|failed|\[acp/i.test(t)) console.log(`[${now()}][${m.type()}] ${t.slice(0, 220)}`);
});
page.on("pageerror", (e) => console.log(`[${now()}][pageerror] ${String(e).slice(0, 220)}`));

await page.goto("http://localhost:5180/", { waitUntil: "domcontentloaded" });
await page.evaluate((sid) => {
  const dir = "C:/Users/heycj/dev/loom";
  localStorage.setItem("lastDirectory", dir);
  localStorage.setItem("directory-store", JSON.stringify({
    state: { currentDirectory: dir, directoryHistory: [dir], historyIndex: 0, homeDirectory: dir, hasPersistedDirectory: true, isHomeReady: true, isSwitchingDirectory: false },
    version: 0,
  }));
  sessionStorage.setItem("pending-session-open", sid);
}, SESSION);
await page.goto(`http://localhost:5180/?session=${SESSION}`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(12000);
for (let i = 0; i < 2; i++) { await page.keyboard.press("Escape"); await page.waitForTimeout(400); }
await page.waitForTimeout(8000);

const stats = await page.evaluate(() => {
  const native = document.querySelector('[data-acp-native]');
  const msgs = [...document.querySelectorAll('[data-acp-message]')].map((el) => ({
    id: (el.getAttribute("data-acp-message") || "").slice(0, 30),
    text: (el.textContent || "").slice(0, 90),
  }));
  const tools = [...document.querySelectorAll('[data-acp-tool]')].map((el) => (el.textContent || "").slice(0, 70));
  const plan = !!document.querySelector("[data-acp-native] .text-\\[11px\\]");
  return { native: !!native, msgCount: msgs.length, msgs: msgs.slice(0, 8), toolCount: tools.length, tools: tools.slice(0, 8) };
});
console.log(`[${now()}s]`, JSON.stringify(stats, null, 1).slice(0, 1800));
await page.screenshot({ path: "replay-native.png", fullPage: false });
await browser.close();
