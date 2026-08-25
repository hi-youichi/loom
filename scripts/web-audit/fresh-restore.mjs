// Simulate a fresh browser: clear ALL storage, reload, verify projects restored from server.
import { chromium } from "playwright";

const browser = await chromium.launch();
const page = await (await browser.newContext()).newPage();
page.on("console", (m) => {
  const t = m.text();
  if (/ACP project/i.test(t)) console.log("[c] " + t.slice(0, 150));
});

await page.goto("http://localhost:5180/", { waitUntil: "domcontentloaded" });
await page.evaluate(() => localStorage.clear());
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(12000);

const state = await page.evaluate(() => {
  const projects = JSON.parse(localStorage.getItem("projects") || "[]");
  const body = document.body.textContent || "";
  return {
    projects: projects.map((p) => ({ id: p.id, path: p.path, label: p.label })),
    activeProjectId: localStorage.getItem("activeProjectId"),
    sidebarHasanureo: body.includes("anureo"),
  };
});
console.log(JSON.stringify(state, null, 1));
await page.screenshot({ path: "C:/Users/heycj/dev/anureo/scripts/web-audit/fresh-restore.png" });
await browser.close();
