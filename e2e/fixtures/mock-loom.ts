/**
 * e2e/fixtures/mock-loom.ts
 *
 * Mock Loom API fixture.
 *
 * 使用 page.route() 拦截前端对 Loom server 的 HTTP 请求。
 * Playwright 自动在每个新 page 创建时应用，无需显式调用。
 *
 * 拦截的端点：
 * - GET /api/sessions → 返回空数组
 * - GET /api/models → 返回 models.json 内容
 * - POST /api/sessions → 返回 mock 会话对象
 *
 * @module fixtures/mock-loom
 */

import { test as base } from "@playwright/test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// ─── Mock Data ────────────────────────────────────────────────────────────────

/**
 * 加载 mock models 数据。
 * 从 e2e/mocks/models.json 读取。
 */
function loadMockModels(): unknown[] {
  try {
    const modelsPath = path.resolve(__dirname, "../mocks/models.json");
    const content = readFileSync(modelsPath, "utf-8");
    const data = JSON.parse(content);
    return data.models ?? [];
  } catch {
    return [];
  }
}

const MOCK_MODELS = loadMockModels();

// ─── Fixture Implementation ─────────────────────────────────────────────────

/**
 * mockLoom fixture — Playwright fixture dict (NOT a TestType).
 *
 * 用法：
 * ```
 * import { test as base, expect } from "@playwright/test";
 * import { mockLoom } from "../../fixtures/mock-loom";
 * import { auth } from "../../fixtures/auth";
 *
 * const test = base.extend({
 *   ...mockLoom,
 *   ...auth,
 * });
 * ```
 */
export const mockLoom: Record<string, unknown> = {
  /**
   * 自动挂载 mock 到每个新 page。
   * 使用 page.route() 在浏览器进程中拦截请求（零网络开销）。
   */
  page: async ({ page }, use) => {
    // 拦截 GET /api/sessions（会话列表）
    await page.route("**/api/sessions**", async (route) => {
      if (route.request().method() === "GET") {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify([]),
        });
        return;
      }
      await route.continue();
    });

    // 拦截 GET /api/models（模型列表）
    await page.route("**/api/models**", async (route) => {
      if (route.request().method() === "GET") {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify(MOCK_MODELS),
        });
        return;
      }
      await route.continue();
    });

    // 拦截 POST /api/sessions（创建会话）
    await page.route("**/api/sessions", async (route) => {
      if (route.request().method() === "POST") {
        await route.fulfill({
          status: 201,
          contentType: "application/json",
          body: JSON.stringify({ id: "mock-session-id", title: "New Session" }),
        });
        return;
      }
      await route.continue();
    });

    await use(page);
  },
};

// Re-export expect for convenience
export { expect } from "@playwright/test";
