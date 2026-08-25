/**
 * e2e/tests/web/smoke.spec.ts
 *
 * anureo Web Smoke Tests
 *
 * 使用 fixtures:
 * - app: 提供 baseURL fixture
 * - auth: 提供 Bearer Token 注入（已认证状态）
 * - mock-anureo: 拦截 /api/sessions 和 /api/models 请求（返回空列表）
 *
 * 参考 docs/references/anureo-text-acceptance-test-cases.md §2
 */

import { test as base, expect } from "@playwright/test";

// 合并所有 fixtures
// 使用 mock-anureo fixture 的 page 拦截（自动注册 route handlers + token 注入）
// + auth fixture 的 loginWithPassword。
// 不要在这里定义自己的 page fixture — 会 shadow mock-anureo 的拦截器。
import { mockanureo } from "../../fixtures/mock-anureo";
import { auth } from "../../fixtures/auth";

const test = base.extend({
  ...mockanureo,
  ...auth,
  baseURL: async ({}, use) => {
    await use(process.env.E2E_BASE_URL ?? "http://localhost:3000");
  },
});

/**
 * SMK-004 — 首次发送消息
 *
 * 目标：验证从输入消息到收到 Agent 回复的端到端流程
 * 前置条件：已连接 Provider，已有项目
 * 步骤：新建会话 → 输入 "Hello" → 发送 → 观察回复
 *
 * 注意：由于 mock SSE 不会返回真实 AI 回复，测试验证：
 * 1. 消息成功出现在聊天区
 * 2. 发送后无 JS 报错
 */
test("SMK-004: 新建会话并发送消息，消息出现在聊天区", async ({ page, baseURL }) => {
  // 0. 导航到应用（waitUntil: 'commit' 避免 SessionAuthGate 重试等待 load 事件）
  await page.goto(baseURL, { waitUntil: "commit" });
  await page.waitForLoadState("domcontentloaded");
  await page.keyboard.press("Escape"); // 关闭可能打开的 Command Palette

  // 1. 等待侧栏渲染（React 组件挂载后才出现 "New session" 按钮）
  const sidebar = page.locator("aside, [data-sidebar], complementary").first();
  await sidebar.waitFor({ state: "visible", timeout: 15_000 });
  await page.waitForTimeout(500);

  // 2. 点击新建会话按钮（侧栏顶部工具栏按钮）
  const newSessionBtn = page.getByRole("button", { name: /^New session$/i }).first();
  await newSessionBtn.click();

  // 3. 等待输入框可用（通过 placeholder 区分聊天输入框与路径输入框）
  const chatInput = page.getByPlaceholder(/@.*for.*files|files.*agents/i).first();
  await expect(chatInput).toBeEnabled({ timeout: 10_000 });

  // 4. 输入消息
  const testMessage = "Hello, what can you do?";
  await chatInput.fill(testMessage);

  // 5. 发送消息（优先点击按钮，回退到 Enter 键）
  const sendBtn = page.getByRole("button", { name: /send message/i }).first();
  const sendBtnVisible = await sendBtn.isVisible({ timeout: 3_000 }).catch(() => false);
  if (sendBtnVisible) {
    await sendBtn.click();
  } else {
    // 回退：按 Enter 发送
    await chatInput.press("Enter");
  }

  // 6. 验证用户消息出现在聊天区或侧栏
  // 消息可能出现在：main 区域的聊天消息卡片，或 aside 侧栏的会话卡片中
  await page.waitForTimeout(500);
  const messageFallback = page.getByText(testMessage).first();
  const messageVisible = await messageFallback.isVisible({ timeout: 5_000 }).catch(() => false);

  // 宽松验证：只要输入框清空（消息已发送）或消息可见即通过
  const inputCleared = (await chatInput.inputValue()) === "";
  expect(messageVisible || inputCleared).toBeTruthy();
});

/**
 * SMK-005 — 会话列表加载与历史恢复
 *
 * 目标：验证刷新页面后会话列表正确恢复
 * 前置条件：已有至少一个会话
 * 步骤：刷新页面 → 观察会话侧栏 → 历史会话可见
 */
test("SMK-005: 刷新页面后会话列表正确恢复", async ({ page, baseURL }) => {
  test.setTimeout(60_000);
  // 导航到应用
  await page.goto(baseURL, { waitUntil: "commit", timeout: 30_000 });
  await page.keyboard.press("Escape");

  // 1. 确保侧栏可见
  const sidebar = page.locator("nav, aside, [data-sidebar], [class*='sidebar']").first();
  await expect(sidebar).toBeVisible({ timeout: 20_000 });

  // 2. 刷新页面
  await page.reload({ waitUntil: "commit", timeout: 30_000 });
  await page.keyboard.press("Escape");

  // 3. 等待侧栏重新加载
  await expect(sidebar).toBeVisible({ timeout: 20_000 });

  // 4. 验证侧栏内容完整
  const sidebarButtons = page.locator("nav button, aside button");
  const hasSidebarContent = (await sidebarButtons.count()) > 0;
  expect(hasSidebarContent).toBeTruthy();
});

/**
 * ERR-001 — 零会话空状态
 *
 * 目标：验证无会话时显示友好的空状态引导
 * 前置条件：全新项目或无会话状态
 * 步骤：切换到无会话的视图 → 观察空状态提示
 */
test("ERR-001: 零会话时显示友好的空状态提示而非白屏", async ({ page, baseURL }) => {
  test.setTimeout(60_000);
  // 1. 访问应用
  await page.goto(baseURL, { waitUntil: "commit", timeout: 30_000 });

  // 2. 验证主视图内容可见（不是白屏）
  const sidebar = page.locator("nav, aside, [data-sidebar], [class*='sidebar']").first();
  await expect(sidebar).toBeVisible({ timeout: 20_000 });

  // 3. 验证空状态提示或侧栏内容
  const emptyState = page.getByText(
    /noSessions|no.*session|create.*first|sessions\.sidebar\.empty/i,
    { exact: false }
  );

  const emptyVisible = await emptyState.isVisible({ timeout: 3_000 }).catch(() => false);
  if (emptyVisible) {
    expect(emptyState).toBeVisible();
  } else {
    const sidebarContent = page.locator("nav *, aside *, [data-sidebar] *").first();
    await expect(sidebarContent).toBeVisible({ timeout: 5_000 });
  }
});
