/**
 * e2e/page-objects/chat-input.ts
 *
 * 聊天输入区 Page Object：新建会话、输入、发送、消息可见性、错误状态。
 * 只提供状态与交互，不做业务断言。
 *
 * @module page-objects/chat-input
 */

import { expect, Locator, Page } from "@playwright/test";

// 与存量 ERR-003 spec 对齐的多语言友好错误文案
const FRIENDLY_ERROR_PATTERNS = [
  /noProvidersOrModelsFound|model.*not.*available|provider.*not.*found|no.*provider.*available/i,
  /请先连接.*provider|请先添加.*提供商|无可用.*模型|模型.*不可用|未连接.*提供商/i,
  /connect.*provider|add.*provider|set.*api.*key|api.*key.*not.*configured/i,
];

export class ChatInput {
  readonly newSessionButton: Locator;
  readonly input: Locator;
  readonly sendButton: Locator;
  readonly userMessages: Locator;
  readonly assistantMessages: Locator;
  readonly stopGeneratingButton: Locator;

  constructor(private readonly page: Page) {
    this.newSessionButton = page
      .getByRole("button", { name: /^New session$/i })
      .first();
    // 通过 placeholder 区分聊天输入框与路径输入框
    this.input = page
      .getByPlaceholder(/@.*for.*files|files.*agents/i)
      .first();
    this.sendButton = page
      .getByRole("button", { name: /send message/i })
      .first();
    this.userMessages = page.locator(
      '[data-testid="chat-message"][data-message-role="user"]',
    );
    this.assistantMessages = page.locator(
      '[data-testid="chat-message"][data-message-role="assistant"]',
    );
    this.stopGeneratingButton = page.getByRole("button", {
      name: /stop generating/i,
    });
  }

  /** 点击侧栏顶部 "New session" 按钮 */
  async clickNewSession(): Promise<void> {
    // The web shell may reopen its command palette while ACP bootstrap calls
    // settle; ensure it cannot intercept the session button click.
    await this.page.keyboard.press("Escape");
    await this.newSessionButton.waitFor({ state: "visible", timeout: 15_000 });
    await this.newSessionButton.click();
  }

  /** 等待聊天输入框可用 */
  async waitForInput(timeout = 10_000): Promise<void> {
    await expect(this.input).toBeEnabled({ timeout });
  }

  /** 输入消息 */
  async fill(text: string): Promise<void> {
    await this.input.fill(text);
  }

  /** 发送消息（优先点击按钮，回退 Enter 键） */
  async send(): Promise<void> {
    const sendVisible = await this.sendButton
      .isVisible({ timeout: 3_000 })
      .catch(() => false);
    if (sendVisible) {
      await this.sendButton.click();
    } else {
      await this.input.press("Enter");
    }
  }

  /** 选择当前会话第一个可用模型，避免测试依赖本地持久化选择。 */
  async selectFirstAvailableModel(): Promise<void> {
    const selector = this.page.getByRole("button", { name: /select model/i }).first();
    if (!(await selector.isVisible({ timeout: 2_000 }).catch(() => false))) return;
    await selector.click();
    const option = this.page.locator('[role="option"]').first();
    await expect(option).toBeVisible({ timeout: 15_000 });
    await option.click();
  }

  /** 输入框是否已清空（消息已发出的信号） */
  async isInputCleared(): Promise<boolean> {
    return (await this.input.inputValue()) === "";
  }

  /** 消息文本是否出现在页面（聊天区或侧栏会话卡片） */
  async isMessageVisible(text: string, timeout = 5_000): Promise<boolean> {
    return this.page
      .getByText(text)
      .first()
      .isVisible({ timeout })
      .catch(() => false);
  }

  /** 等待指定发送操作产生新的助手消息。 */
  async waitForNewAssistantMessage(
    previousCount: number,
    timeout = 90_000,
  ): Promise<Locator> {
    await expect
      .poll(() => this.assistantMessages.count(), { timeout })
      .toBeGreaterThan(previousCount);
    return this.assistantMessages.last();
  }

  /** 等待助手完成生成；优先使用消息状态，兼容旧 UI 则观察停止按钮。 */
  async waitForReplyComplete(timeout = 90_000): Promise<void> {
    const latest = this.assistantMessages.last();
    const hasMessage = await latest.count();
    if (hasMessage > 0) {
      const status = latest.getAttribute("data-message-status");
      if ((await status) !== null) {
        await expect(latest).toHaveAttribute("data-message-status", "complete", {
          timeout,
        });
        return;
      }
    }

    await this.stopGeneratingButton
      .waitFor({ state: "visible", timeout: 15_000 })
      .catch(() => undefined);
    await this.stopGeneratingButton.waitFor({ state: "hidden", timeout });
  }

  async lastAssistantText(): Promise<string> {
    return (await this.assistantMessages.last().innerText()).trim();
  }

  /** 是否出现友好错误提示（多语言文案匹配） */
  async hasFriendlyError(): Promise<boolean> {
    for (const pattern of FRIENDLY_ERROR_PATTERNS) {
      const msg = this.page.getByText(pattern, { exact: false });
      if (await msg.isVisible({ timeout: 2_000 }).catch(() => false)) {
        return true;
      }
    }
    return this.page
      .locator('[role="alert"]')
      .isVisible({ timeout: 2_000 })
      .catch(() => false);
  }

  /** 是否泄露原始错误（JSON body 或 stack trace） */
  async hasRawError(): Promise<boolean> {
    const rawJson = this.page.getByText(/\{[\s\S]*"error"[\s\S]*\}/);
    const stackTrace = this.page.getByText(/at .+\(.+:\d+:\d+\)/);
    return (
      (await rawJson.isVisible({ timeout: 500 }).catch(() => false)) ||
      (await stackTrace.isVisible({ timeout: 500 }).catch(() => false))
    );
  }
}
