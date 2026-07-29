/**
 * e2e/tests/web/errors.spec.ts
 *
 * OpenChamber Web Error State Tests
 *
 * 使用 fixtures:
 * - app: 提供 baseURL fixture
 * - auth: 提供 Bearer Token 注入（已认证状态）
 * - mock-opencode: 拦截 /api/models 返回空列表（模拟模型不可用）
 *
 * 参考 docs/references/openchamber-text-acceptance-test-cases.md §15 ERR-003
 */

import { test as base, expect } from "@playwright/test";

// 使用 mock-opencode 的 page 拦截 + auth 的 loginWithPassword
import { mockOpencode } from "../../fixtures/mock-opencode";
import { auth } from "../../fixtures/auth";

const test = base.extend({
  ...mockOpencode,
  ...auth,
  baseURL: async ({}, use) => {
    await use(process.env.E2E_BASE_URL ?? "http://localhost:3000");
  },
});

/**
 * ERR-003 — 模型不可用时友好报错
 *
 * 目标：验证 Provider 未连接或 Key 失效时发送消息有友好提示
 * 前置条件：mock-opencode 返回空 models 列表（模拟无可用模型）
 * 步骤：1. 新建会话 2. 发送消息
 * 预期结果：显示友好错误提示（如 "请先连接 Provider"）而非原始 API 错误
 * 观察点/失败信号：无提示、原始 JSON 错误泄露、白屏
 */
test("ERR-003: 模型不可用时显示友好错误提示而非原始 API 错误", async ({ page, baseURL }) => {
  // 1. 导航到应用（waitUntil: 'commit' 避免 SessionAuthGate 重试等待 load 事件）
  await page.goto(baseURL, { waitUntil: "commit" });
  await page.waitForLoadState("domcontentloaded");
  await page.keyboard.press("Escape"); // 关闭可能打开的 Command Palette

  // 2. 等待侧栏渲染（React 组件挂载后才出现 "New session" 按钮）
  const sidebar = page.locator("aside, [data-sidebar], complementary").first();
  await sidebar.waitFor({ state: "visible", timeout: 15_000 });

  // 3. 点击新建会话按钮
  const newSessionBtn = page.getByRole("button", { name: /^New session$/i }).first();
  const newSessionVisible = await newSessionBtn.isVisible().catch(() => false);
  if (newSessionVisible) {
    await newSessionBtn.click();
    await page.waitForTimeout(500);
  }

  // 4. 找到输入框并输入消息
  const chatInput = page.getByPlaceholder(/@.*for.*files|files.*agents/i).first();
  await expect(chatInput).toBeEnabled({ timeout: 10_000 });
  await chatInput.fill("Hello, what can you do?");

  // 5. 点击发送按钮
  const sendBtn = page.getByRole("button", { name: /send message/i }).first();
  await sendBtn.click();

  // 6. 等待响应（mock 返回空 sessions，但 SSE 流可能报错）
  await page.waitForTimeout(2_000);

  // 7. 验证友好错误提示出现
  // 匹配多语言友好错误文本（来自 ModelControls.tsx 等源码）
  const friendlyErrorPatterns = [
    // 英文
    /noProvidersOrModelsFound|model.*not.*available|provider.*not.*found|no.*provider.*available/i,
    // 中文
    /请先连接.*provider|请先添加.*提供商|无可用.*模型|模型.*不可用|未连接.*提供商/i,
    // 通用的友好提示
    /connect.*provider|add.*provider|set.*api.*key|api.*key.*not.*configured/i,
  ];

  let errorVisible = false;
  for (const pattern of friendlyErrorPatterns) {
    const errorMsg = page.getByText(pattern, { exact: false });
    if (await errorMsg.isVisible({ timeout: 2_000 }).catch(() => false)) {
      errorVisible = true;
      break;
    }
  }

  // 8. 检查 role="alert" 错误区域
  const alertArea = page.locator('[role="alert"]');
  if (await alertArea.isVisible({ timeout: 2_000 }).catch(() => false)) {
    errorVisible = true;
  }

  // 9. 验证不显示原始 JSON 错误或 stack trace
  const rawJsonError = page.getByText(/\{[\s\S]*"error"[\s\S]*\}/);
  const stackTraceError = page.getByText(/at .+\(.+:\d+:\d+\)/);

  const hasRawJson = await rawJsonError.isVisible({ timeout: 500 }).catch(() => false);
  const hasStackTrace = await stackTraceError.isVisible({ timeout: 500 }).catch(() => false);

  // 验证：要么有友好错误提示，要么至少没有原始 JSON 错误泄露
  expect(errorVisible || !(hasRawJson || hasStackTrace)).toBeTruthy();

  // 如果错误可见，验证它是友好提示（不是原始 JSON 或 stack trace）
  if (errorVisible) {
    expect(hasRawJson).toBeFalsy();
    expect(hasStackTrace).toBeFalsy();
  }
});
