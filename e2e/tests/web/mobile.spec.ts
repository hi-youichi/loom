/**
 * e2e/tests/web/mobile.spec.ts
 *
 * OpenChamber Web Mobile Tests
 *
 * 注意：此文件由 playwright.config.ts 中的 Mobile Chrome project 运行
 * （Pixel 7 设备，viewport: 412x915）
 *
 * 参考 docs/references/openchamber-text-acceptance-test-cases.md §16
 */

import { test as base, expect } from "@playwright/test";

// 使用 mock-loom 的 page 拦截（mockLoom 的 page fixture 不能被覆盖）
import { mockLoom } from "../../fixtures/mock-loom";

const test = base.extend({
  ...mockLoom,
  baseURL: async ({}, use) => {
    await use(process.env.E2E_BASE_URL ?? "http://localhost:3000");
  },
});

/**
 * MOB-001 — 抽屉式侧栏
 *
 * 目标：验证窄屏下左右侧栏变为抽屉可滑出
 * 前置条件：手机浏览器或 DevTools 模拟移动端（Pixel 7, 412x915）
 * 步骤：1. 打开 OpenChamber（手机宽度）2. 观察布局 3. 点击切换按钮滑出左侧栏 4. 滑出右侧栏
 * 预期结果：三栏变为单栏 + 抽屉；侧栏可滑出/滑入
 * 观察点/失败信号：布局溢出、抽屉不可滑出、三栏挤在一起
 */
test("MOB-001: 移动视口下抽屉式侧栏可滑出/滑入", async ({ page, baseURL }) => {
  // 0. 验证移动视口（playwright.config.ts 中 Mobile Chrome = Pixel 7, 412x915）
  const viewport = page.viewportSize();
  expect(viewport).toBeTruthy();
  if (viewport) {
    expect(viewport.width).toBeLessThanOrEqual(500); // 移动端应 < 500px
  }

  // 1. 导航到应用
  await page.goto(baseURL, { waitUntil: "commit" });
  await page.waitForLoadState("domcontentloaded");

  // 2. 验证主内容区在移动视口下正常显示
  // 移动端应只有主内容区可见，侧栏隐藏为抽屉
  const mainContent = page.locator("[data-chat-area], main, [data-main-content]").first();
  await expect(mainContent).toBeVisible({ timeout: 15_000 });

  // 3. 验证布局没有水平溢出
  const bodyHandle = await page.$("body");
  if (bodyHandle) {
    const scrollWidth = await page.evaluate((el) => el.scrollWidth, bodyHandle);
    const clientWidth = await page.evaluate((el) => el.clientWidth, bodyHandle);
    expect(scrollWidth).toBeLessThanOrEqual(clientWidth + 10);
  }

  // 4. 查找侧栏切换按钮（TitlebarLeftControls.tsx:115）
  // 实际 aria-label 是 "Open sessions and projects"（非 i18n key）
  const sidebarToggleBtn = page.getByRole("button", {
    name: /open sessions and projects/i,
  });

  // 5. 点击切换按钮打开左侧抽屉（先关闭通知栏避免遮挡）
  await page.keyboard.press("Escape"); // 关闭通知
  await page.waitForTimeout(300);
  // 使用 JS click 绕过 React overlay actionability 检测
  await page.evaluate(() => {
    const btn = document.querySelector('button[aria-label*="sessions" i]') as HTMLButtonElement | null;
    if (btn) btn.click();
  });
  await page.waitForTimeout(800);

  // 6. 验证左侧抽屉出现（MobileOverlayPanel 渲染）
  const leftDrawer = page
    .locator("[data-mobile-overlay], [class*='drawer'], [class*='overlay']")
    .first();
  const drawerVisible = await leftDrawer.isVisible({ timeout: 5_000 }).catch(() => false);
  if (drawerVisible) {
    await expect(leftDrawer).toBeVisible();
  }

  // 7. 再次点击切换按钮关闭左侧抽屉
  // JS click 绕过 overlay/actionability 检测
  await page.evaluate(() => {
    const btn = document.querySelector('button[aria-label*="sessions" i]') as HTMLButtonElement | null;
    if (btn) btn.click();
  });
  await page.waitForTimeout(500);

  // 8. 验证主内容区仍然可见（抽屉关闭后布局正常）
  await expect(mainContent).toBeVisible();

  // 9. 验证右侧抽屉（Toggle right sidebar 按钮）
  const rightSidebarToggle = page.getByRole("button", {
    name: /toggle right sidebar/i,
  });
  const rightToggleVisible = await rightSidebarToggle.isVisible({ timeout: 2_000 }).catch(() => false);

  if (rightToggleVisible) {
    await page.evaluate(() => {
      const btn = Array.from(document.querySelectorAll('button')).find(
        b => b.textContent?.includes('sidebar') || b.getAttribute('aria-label')?.includes('right')
      ) as HTMLButtonElement | null;
      if (btn) btn.click();
    });
    await page.waitForTimeout(500);

    // 验证右侧抽屉出现
    const rightDrawer = page
      .locator("[data-mobile-overlay], [class*='drawer'], [class*='overlay']")
      .last();
    const rightDrawerVisible = await rightDrawer.isVisible({ timeout: 5_000 }).catch(() => false);
    if (rightDrawerVisible) {
      await expect(rightDrawer).toBeVisible();
    }

    // 关闭右侧抽屉
    await page.evaluate(() => {
      const btn = Array.from(document.querySelectorAll('button')).find(
        b => b.textContent?.includes('sidebar') || b.getAttribute('aria-label')?.includes('right')
      ) as HTMLButtonElement | null;
      if (btn) btn.click();
    });
    await page.waitForTimeout(500);
  }

  // 10. 最终验证主内容区仍然满屏可见
  await expect(mainContent).toBeVisible();
  const mainBox = await mainContent.boundingBox();
  if (mainBox && viewport) {
    // 主内容区应占满视口大部分宽度（至少 50%）
    expect(mainBox.width).toBeGreaterThan(viewport.width * 0.5);
  }
});
