// Inject stack-logger into useGitStore module via vite dev server response patching.
import { chromium } from "playwright";

const t0 = Date.now();
const now = () => ((Date.now() - t0) / 1000).toFixed(1);
const browser = await chromium.launch();
const ctx = await browser.newContext();
const page = await ctx.newPage();

page.on("console", (m) => {
  const text = m.text();
  if (text.startsWith("[FB]") || text.startsWith("[GS]")) console.log(text.slice(0, 1200));
});

await page.route("**/*", async (route) => {
  const req = route.request();
  const url = req.url();
  if (url.includes("useGitStore")) {
    const resp = await route.fetch();
    let body = await resp.text();
    if (body.includes("fetchBranches")) {
      body += `
;(function(){
  const st = useGitStore.getState();
  const origFB = st.fetchBranches.bind(st);
  useGitStore.setState({ fetchBranches: (...a) => {
    const stack = new Error().stack.split("\\n").slice(1, 9).join("\\n    ");
    console.log("[FB] t=${now()}\\n    " + stack);
    return origFB(...a);
  }});
  const origFS = st.fetchStatus?.bind(st);
  if (origFS) useGitStore.setState({ fetchStatus: (...a) => {
    console.log("[GS] t=${now()} dir=" + a[0]);
    return origFS(...a);
  }});
  console.log("[FB] instrumented useGitStore");
})();
`;
      await route.fulfill({ response: resp, body, headers: { ...resp.headers(), "content-type": "application/javascript; charset=utf-8" } });
      return;
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
console.log("observing 35s ...");
await page.waitForTimeout(35000);
await browser.close();
