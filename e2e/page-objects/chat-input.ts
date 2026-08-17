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
  }

  /** 点击侧栏顶部 "New session" 按钮 */
  async clickNewSession(): Promise<void> {
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
