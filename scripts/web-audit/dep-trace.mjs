// Inject dep-logging into ChatInput draft-branch effect via vite module patching.
import { chromium } from "playwright";

const t0 = Date.now();
const now = () => ((Date.now() - t0) / 1000).toFixed(1);
const browser = await chromium.launch();
const ctx = await browser.newContext();
const page = await ctx.newPage();

page.on("console", (m) => {
  const text = m.text();
  if (text.startsWith("[DEP]")) console.log(text.slice(0, 800));
});

await page.route("**/*", async (route) => {
  const url = route.request().url();
  if (url.includes("ChatInput")) {
    const resp = await route.fetch();
    let body = await resp.text();
    const marker = "void fetchBranches(selectedDraftProjectPath, runtimeGit)";
    if (body.includes(marker)) {
      body = body.replace(marker, `
console.log("[DEP] t=${now()} showSel=" + showDraftTargetSelectors + " path=" + selectedDraftProjectPath + " proj=" + (selectedDraftProject ? (selectedDraftProject.path||'') + '#' + (selectedDraftProject.id||'') : 'null') + " isGitRepo=" + selectedDraftProjectIsGitRepo + " fetchedAt=" + selectedDraftProjectBranchesFetchedAt + " hasList=" + hasDraftBranchList + " stale=" + isStale + " runtimeGit=" + (runtimeGit ? 'ok' : 'null'));
${marker}`);
      await route.fulfill({ response: resp, body, headers: { ...resp.headers(), "content-type": "application/javascript; charset=utf-8" } });
      console.log("[DEP] ChatInput patched");
      return;
    } else {
      console.log("[DEP] ChatInput fetched but marker missing!");
    }
  }
  await route.continue();
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
console.log("observing 30s ...");
await page.waitForTimeout(30000);
await browser.close();
