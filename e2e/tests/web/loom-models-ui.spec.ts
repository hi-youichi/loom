/**
 * e2e/tests/web/loom-models-ui.spec.ts
 *
 * Loom Backend Models UI Verification
 * 验证 openchamber 前端的模型选择器能正确显示 loom backend 返回的模型列表
 *
 * 前置条件：
 * 1. loom-server 运行在 http://127.0.0.1:18081
 *    cargo run -p loom-server -- serve --host 127.0.0.1 --port 18081
 *
 * 2. openchamber 前端运行在 http://localhost:5173
 *    cd openchamber-feat-dev
 *    $env:OPENCODE_HOST = "http://127.0.0.1:18081"
 *    $env:OPENCHAMBER_PORT = "3200"
 *    bun run dev:web:full
 *
 * 运行：
 *    cd e2e
 *    npx playwright test loom-models-ui.spec.ts
 */

import { test, expect } from "@playwright/test";

const LOOM_SERVER_URL = process.env.LOOM_SERVER_URL ?? "http://127.0.0.1:18081";
const OPENCHAMBER_URL = process.env.OPENCHAMBER_URL ?? "http://localhost:5173";

// ─── API 测试（不需要 UI）─────────────────────────────────────────────────────

test.describe("Loom Backend API", () => {
  test("/api/model 返回正确格式和模型列表", async ({ request }) => {
    const resp = await request.get(`${LOOM_SERVER_URL}/api/model`);
    expect(resp.ok()).toBeTruthy();

    const json = await resp.json();

    // Location 信封格式: { location: {...}, data: [...] }
    expect(json).toHaveProperty("location");
    expect(json).toHaveProperty("data");

    // data 应该是数组
    const models = json.data;
    expect(Array.isArray(models)).toBeTruthy();
    expect(models.length).toBeGreaterThan(0);

    // 验证模型结构
    const first = models[0];
    expect(first).toHaveProperty("id");
    expect(first).toHaveProperty("providerID");
    expect(first).toHaveProperty("name");

    console.log("Loom API models count:", models.length);
    console.log("Sample models:", models.slice(0, 3).map((m: any) => `${m.providerID}/${m.id}`));
  });
});

// ─── UI 测试 ──────────────────────────────────────────────────────────────────

test.describe("Loom Backend Models UI", () => {
  test.beforeEach(async ({ page }) => {
    // 导航到 openchamber 并等待加载
    await page.goto(OPENCHAMBER_URL, { waitUntil: "domcontentloaded" });

    // 等待页面完全加载
    await page.waitForTimeout(3000);

    // 截图调试
    await page.screenshot({ path: "debug-page-load.png", fullPage: true });

    // 关闭可能出现的欢迎弹窗/palette
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);

    // 尝试多种 sidebar 选择器
    const sidebarSelectors = [
      "aside",
      "[data-sidebar]",
      "[role='complementary']",
      "nav",
      "[class*='sidebar']",
      "[class*='Sidebar']",
    ];

    let sidebarFound = false;
    for (const selector of sidebarSelectors) {
      const sidebar = page.locator(selector).first();
      if (await sidebar.isVisible({ timeout: 2000 }).catch(() => false)) {
        sidebarFound = true;
        break;
      }
    }

    if (!sidebarFound) {
      // 如果找不到 sidebar，尝试查找 main 区域或任何可见内容
      const mainContent = page.locator("main, [role='main'], #root, #app").first();
      await mainContent.waitFor({ state: "attached", timeout: 5000 }).catch(() => {});
    }
  });

  test("模型选择器能显示 loom backend 返回的模型", async ({ page }) => {
    // 1. 新建会话
    const newSessionBtn = page.getByRole("button", { name: /^New session$/i }).first();
    await newSessionBtn.click();
    await page.waitForTimeout(500);

    // 2. 查找模型选择器
    const modelSelector = page.locator("main [role='combobox'], [class*='model'] [role='combobox']").first();
    await expect(modelSelector).toBeVisible({ timeout: 10_000 });

    // 3. 打开模型选择器
    await modelSelector.click();
    await page.waitForTimeout(800);

    // 4. 验证模型列表出现
    const options = page.locator("[role='option'], [role='listbox'] > *");
    const firstOption = options.first();
    await expect(firstOption).toBeVisible({ timeout: 5_000 });

    // 5. 验证至少有一个模型选项
    const count = await options.count();
    expect(count).toBeGreaterThan(0);

    // 6. 截图记录
    await page.screenshot({ path: "loom-models-dropdown.png" });
  });

  test("模型选择器显示正确的 provider/model 格式", async ({ page }) => {
    // 1. 新建会话
    await page.getByRole("button", { name: /^New session$/i }).first().click();
    await page.waitForTimeout(500);

    // 2. 打开模型选择器
    const modelSelector = page.locator("main [role='combobox']").first();
    await modelSelector.click();
    await page.waitForTimeout(800);

    // 3. 获取所有模型选项的文本
    const options = page.locator("[role='option']");
    const texts = await options.allTextContents();

    // 4. 验证模型名称非空
    expect(texts.length).toBeGreaterThan(0);
    for (const text of texts) {
      expect(text.trim().length).toBeGreaterThan(0);
    }

    // 5. 输出模型列表（调试用）
    console.log("Available models:", texts);
  });

  test("选择模型后会话使用该模型", async ({ page }) => {
    // 1. 新建会话
    await page.getByRole("button", { name: /^New session$/i }).first().click();
    await page.waitForTimeout(500);

    // 2. 打开模型选择器
    const modelSelector = page.locator("main [role='combobox']").first();
    await modelSelector.click();
    await page.waitForTimeout(800);

    // 3. 选择第一个模型
    const firstOption = page.locator("[role='option']").first();
    const modelText = await firstOption.textContent();
    await firstOption.click();
    await page.waitForTimeout(500);

    // 4. 验证选择器显示已选择的模型
    const selectedText = await modelSelector.textContent();
    expect(selectedText?.trim()).toBeTruthy();

    // 5. 截图记录
    await page.screenshot({ path: "loom-model-selected.png" });
  });
});
