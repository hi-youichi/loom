/**
 * e2e/page-objects/app-page.ts
 *
 * 应用级 Page Object：导航、刷新、全局空状态。
 * 只提供状态与交互，不做业务断言。
 *
 * @module page-objects/app-page
 */

import { Page } from "@playwright/test";

export class AppPage {
  constructor(readonly page: Page) {}

  /** 打开应用首页（waitUntil: 'commit' 避免 SessionAuthGate 重试等待 load 事件） */
  async open(path = "/"): Promise<void> {
    await this.page.goto(path, { waitUntil: "commit" });
    await this.page.waitForLoadState("domcontentloaded");
    await this.closeCommandPalette();
  }

  /** 刷新页面并重新进入稳定态 */
  async reload(): Promise<void> {
    await this.page.reload({ waitUntil: "commit" });
    await this.closeCommandPalette();
  }

  /** 关闭可能自动打开的 Command Palette（无面板时为 no-op） */
  async closeCommandPalette(): Promise<void> {
    await this.page.keyboard.press("Escape");
  }

  /** 是否出现友好空状态提示（多语言文案匹配，与存量 ERR-001 spec 对齐） */
  async hasEmptyStateHint(timeout = 3_000): Promise<boolean> {
    const hint = this.page.getByText(
      /noSessions|no.*session|create.*first|sessions\.sidebar\.empty/i,
      { exact: false },
    );
    return hint.isVisible({ timeout }).catch(() => false);
  }
}
