/**
 * e2e/fixtures/auth.ts
 *
 * Playwright test fixture for anureo UI authentication.
 *
 * 使用方式：
 * ```
 * const test = base.extend({ ...app, ...auth });
 * test('my test', async ({ page, loginWithPassword }) => {
 *   await page.goto('/');
 *   await loginWithPassword(page, 'test-password');
 * });
 * ```
 *
 * @module fixtures/auth
 */

import { TestFixture } from "@playwright/test";

// ─── Constants ────────────────────────────────────────────────────────────────

const DEFAULT_PASSWORD = process.env.E2E_UI_PASSWORD ?? "test-password";

// ─── Fixture Implementation ─────────────────────────────────────────────────

/**
 * auth fixture — 提供认证相关的 fixtures。
 *
 * 在测试中合并：
 * ```
 * import { test as base, expect } from "@playwright/test";
 * import { app } from "./fixtures/app";
 * import { auth } from "./fixtures/auth";
 * const test = base.extend({ ...app, ...auth });
 * ```
 *
 * 使用 loginWithPassword（密码登录）：
 * ```
 * test('login flow', async ({ page, loginWithPassword }) => {
 *   await page.goto('/');
 *   await loginWithPassword(page, 'my-password');
 * });
 * ```
 */
export const auth: Record<string, TestFixture> = {
  /**
   * loginWithPassword fixture。
   * 模拟用户通过密码登录 UI。
   *
   * 登录流程：
   * 1. 等待密码输入框出现
   * 2. 输入密码
   * 3. 点击登录按钮
   * 4. 等待认证完成
   */
  loginWithPassword: async ({}, use) => {
    await use(
      async (
        page: import("@playwright/test").Page,
        password: string = DEFAULT_PASSWORD,
      ) => {
        await page.waitForSelector('input[type="password"]', { timeout: 10_000 });
        const passwordInput = page.locator('input[type="password"]').first();
        await passwordInput.fill(password);
        const loginButton = page
          .locator(
            'button[type="submit"], button:has-text("Sign in"), button:has-text("Login")',
          )
          .first();
        await loginButton.click();
        await page.waitForSelector('input[type="password"]', {
          state: "hidden",
          timeout: 15_000,
        });
        console.log("[auth fixture] Password login completed");
      },
    );
  },
};
