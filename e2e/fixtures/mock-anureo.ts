/**
 * e2e/fixtures/mock-anureo.ts
 *
 * Mock anureo API fixture.
 *
 * 使用 page.route() 拦截前端对 anureo server 的 HTTP 请求。
 * Playwright 自动在每个新 page 创建时应用，无需显式调用。
 *
 * 拦截的端点：
 * - GET /api/sessions → 返回空数组
 * - GET /api/models → 返回 models.json 内容
 * - POST /api/sessions → 返回 mock 会话对象
 *
 * @module fixtures/mock-anureo
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
 * mockanureo fixture — Playwright fixture dict (NOT a TestType).
 *
 * 用法：
 * ```
 * import { test as base, expect } from "@playwright/test";
 * import { mockanureo } from "../../fixtures/mock-anureo";
 * import { auth } from "../../fixtures/auth";
 *
 * const test = base.extend({
 *   ...mockanureo,
 *   ...auth,
 * });
 * ```
 */
/** 安装 REST mock；供普通 spec 和 Mock ACP chat fixture 复用。 */
export async function installMockHttpRoutes(page: import("@playwright/test").Page): Promise<void> {
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

  // Legacy web bootstrap endpoints used while the ACP runtime is probing.
  // Keep them consistent with the ACP model response so startup can select a
  // provider/model before the first prompt.
  const providerConfig = {
    providers: [{
      id: "mock-provider",
      name: "Mock Provider",
      models: { "mock-model": { id: "mock-model", name: "Mock Model" } },
    }],
    default: { "mock-provider": "mock-model" },
  };
  await page.route("**/api/config/providers**", async (route) => {
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(providerConfig) });
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
}

export const mockanureo: Record<string, unknown> = {
  page: async ({ page }, use) => {
    await installMockHttpRoutes(page);
    await use(page);
  },
};

// Re-export expect for convenience
export { expect } from "@playwright/test";
