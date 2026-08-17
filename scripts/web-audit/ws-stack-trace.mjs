// Capture JS call stacks for outgoing WS frames (method-filtered).
import { chromium } from "playwright";

const url = process.argv[2] ?? "http://localhost:5180/?session=session-03522b8e-361c-4702-8ee8-c68ceb9c0493";
const wait = Number(process.argv[3] ?? 12000);
const filter = process.argv[4] ?? "session/load,session/list,global/subscribe";

const browser = await chromium.launch();
const page = await browser.newPage();
await page.addInitScript(`
  const FILTER = ${JSON.stringify(filter.split(","))};
  const origSend = WebSocket.prototype.send;
  WebSocket.prototype.send = function (data) {
    try {
      const s = typeof data === "string" ? data : "";
      if (s) {
        const j = JSON.parse(s);
        if (j.method && FILTER.some((f) => j.method.includes(f))) {
          const stack = new Error().stack.split("\\n").slice(1, 8).join(" | ");
          console.log("[WSSEND] " + j.method + "#" + j.id + " " + JSON.stringify(j.params).slice(0, 140) + "\\nSTACK " + stack);
        }
      }
    } catch {}
    return origSend.apply(this, arguments);
  };
`);
page.on("console", (m) => {
  const t = m.text();
  if (t.startsWith("[WSSEND]")) console.log(t);
});
await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30000 });
await page.waitForTimeout(wait);
await browser.close();
