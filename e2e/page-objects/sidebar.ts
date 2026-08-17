/**
 * e2e/page-objects/sidebar.ts
 *
 * 侧栏 Page Object：定位、等待策略、状态查询。
 *
 * @module page-objects/sidebar
 */

import { expect, Locator, Page } from "@playwright/test";

export class Sidebar {
  readonly root: Locator;

  constructor(page: Page) {
    // 与存量 spec 保持一致的多候选定位（nav / aside / data-sidebar / class 启发式）
    this.root = page
      .locator("nav, aside, [data-sidebar], [class*='sidebar']")
      .first();
  }

  /** 等待侧栏渲染完成（React 挂载后出现） */
  async waitForVisible(timeout = 20_000): Promise<void> {
    await expect(this.root).toBeVisible({ timeout });
  }

  /** 侧栏是否包含可交互按钮 */
  async hasInteractiveButtons(): Promise<boolean> {
    return (await this.root.locator("button").count()) > 0;
  }

  /** 侧栏是否渲染了任意子内容（非空壳） */
  async hasContent(timeout = 5_000): Promise<boolean> {
    return this.root
      .locator("*")
      .first()
      .isVisible({ timeout })
      .catch(() => false);
  }
}
