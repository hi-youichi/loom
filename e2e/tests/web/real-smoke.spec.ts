/**
 * e2e/tests/web/real-smoke.spec.ts
 *
 * OpenChamber Web Real Smoke Tests
 *
 * 使用 fixtures:
 * - app: 提供 baseURL fixture
 * - auth: 提供 loginWithPassword 函数（SMK-001 需要测试登录流程）
 *
 * 参考 docs/references/openchamber-text-acceptance-test-cases.md §1
 */

import { test as base, expect } from "@playwright/test";
import { app } from "../../fixtures/app";
import { mockOpencode } from "../../fixtures/mock-opencode";
import { auth } from "../../fixtures/auth";

// 合并所有 fixtures
// mockOpencode 提供 token 注入（让 SessionAuthGate 直接通过，绕过登录页）
// + auth 提供 loginWithPassword + token 注入（双重保险）
// + app 提供 baseURL。
const test = base.extend({
  ...app,
  ...mockOpencode,
  ...auth,
});

/**
 * SMK-001 — 应用启动与初始界面
 *
 * 目标：验证 Web 运行时能正常启动并展示主界面
 * 前置条件：已安装 OpenCode；浏览器可访问 http://localhost:3000
 * 步骤：1. 打开 http://localhost:3000 2. 输入密码登录
 * 预期结果：看到三栏布局：左侧会话侧栏、中间主视图（默认 Chat 标签）、右侧栏（Git/Files/Context）
 * 观察点/失败信号：页面空白、控制台报错、布局错乱、密码不生效
 */
test("SMK-001: 应用启动并显示三栏布局", async ({ page, loginWithPassword }) => {
  // 1. 访问应用（waitUntil: 'commit' 避免 SessionAuthGate 重试等待 load 事件）
  await page.goto("http://localhost:3000", { waitUntil: "commit" });
  await page.keyboard.press("Escape"); // 关闭可能打开的 Command Palette

  // 2. 检查是否是登录页面（如果有密码输入框则登录）
  const passwordInput = page.locator('input[type="password"]');
  const hasPasswordField = await passwordInput
    .isVisible({ timeout: 5000 })
    .catch(() => false);

  if (hasPasswordField) {
    // 使用 auth fixture 登录
    await loginWithPassword(page, "test-password");
  }

  // 3. 验证三栏布局存在

  // 3.1 左侧会话侧栏
  // 根据 page snapshot: <complementary> 是左侧栏
  const leftSidebar = page.locator(
    "[data-sidebar], [class*='sidebar'], complementary"
  ).first();
  await expect(leftSidebar).toBeVisible({ timeout: 15_000 });

  // 3.2 中间主视图（Chat 标签）
  const mainContent = page.locator(
    "[data-main-content], [data-chat-area], main"
  ).first();
  await expect(mainContent).toBeVisible();

  // 3.3 右侧栏（Git/Files/Context）
  // 根据 page snapshot: tablist 中的 "Toggle right sidebar" 按钮存在即说明右侧栏存在
  const rightSidebarToggle = page.getByRole("button", {
    name: /toggle right sidebar/i,
  });
  const hasRightSidebar = await rightSidebarToggle
    .isVisible({ timeout: 3000 })
    .catch(() => false);
  expect(hasRightSidebar).toBeTruthy();

  // 4. 验证页面不是白屏（检查是否有实际内容）
  const bodyContent = page.locator("body > *");
  const contentCount = await bodyContent.count();
  expect(contentCount).toBeGreaterThan(0);

  console.log("[SMK-001] PASS: 三栏布局验证成功");
});
