// Smoke: open command palette -> settings dialog; verify SettingsWindow chunk loads on demand.
import { chromium } from "playwright";

const browser = await chromium.launch();
const page = await browser.newPage();
const loaded = [];
page.on("response", (r) => { if (r.url().includes("SettingsWindow") || r.url().includes("SettingsView")) loaded.push(`${r.status()} ${r.url().split("/").pop()?.slice(0, 60)}`); });

await page.goto("http://localhost:5180/", { waitUntil: "domcontentloaded", timeout: 30000 });
await page.waitForTimeout(6000);

const beforeDialog = await page.locator("[role='dialog']").count();
await page.keyboard.press("Control+K");
await page.waitForTimeout(800);
await page.keyboard.type("settings");
await page.waitForTimeout(500);
await page.keyboard.press("Enter");
await page.waitForTimeout(3000);

const dialogs = await page.locator("[role='dialog']").count();
const dialogText = dialogs > 0 ? (await page.locator("[role='dialog']").first().innerText().catch(() => "")).slice(0, 200) : "";
const hasSettingsContent = /settings|appearance|provider|projects/i.test(dialogText);

console.log(JSON.stringify({ beforeDialog, dialogsAfterOpen: dialogs, hasSettingsContent, settingsChunksLoadedAfterAction: loaded }, null, 2));
await browser.close();
