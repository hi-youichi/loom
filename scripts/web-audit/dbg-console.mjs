// Capture console.trace from page (setCurrentSession instrumentation).
import { chromium } from "playwright";
const url = process.argv[2] ?? "http://localhost:5180/?session=session-03522b8e-361c-4702-8ee8-c68ceb9c0493";
const wait = Number(process.argv[3] ?? 14000);
const browser = await chromium.launch();
const page = await browser.newPage();
page.on("console", (m) => {
  const t = m.text();
  if (t.includes("dbg setCurrentSession")) console.log(t);
});
await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30000 });
await page.waitForTimeout(wait);
await browser.close();
