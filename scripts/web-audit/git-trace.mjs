// Trace WHO calls git.getGitBranches repeatedly: patch + log stack traces.
import { chromium } from "playwright";

const t0 = Date.now();
const now = () => ((Date.now() - t0) / 1000).toFixed(1);
const browser = await chromium.launch();
const ctx = await browser.newContext();
const page = await ctx.newPage();

page.on("console", (m) => {
  const text = m.text();
  if (text.startsWith("[TRACE]") || text.startsWith("[SETVIS]") || text.startsWith("[SUB]")) {
    console.log(text.slice(0, 1500));
  }
});

await page.addInitScript(() => {
  window.__gbCount = 0;
  const origSend = WebSocket.prototype.send;
  WebSocket.prototype.send = function (data) {
    try {
      if (typeof data === "string" && data.includes("git/branches")) {
        window.__gbCount++;
        if (window.__gbCount <= 8) {
          const stack = new Error().stack?.split("\n").slice(1, 12).join("\n    ");
          console.log(`[TRACE] git/branches send #${window.__gbCount}\n    ${stack ?? "?"}`);
        }
      } else if (typeof data === "string" && (data.includes("notification/set_visibility") || data.includes("global/subscribe"))) {
        const stack = new Error().stack?.split("\n").slice(1, 8).join("\n    ");
        console.log(`[SUB] ${data.slice(0, 120)}\n    ${stack ?? "?"}`);
      }
    } catch {}
    return origSend.apply(this, arguments);
  };
  console.log("[TRACE] WS.send patched");
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
console.log("observing 40s ...");
await page.waitForTimeout(40000);
await browser.close();
