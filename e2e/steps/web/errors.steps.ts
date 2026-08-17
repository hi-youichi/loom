/**
 * e2e/steps/web/errors.steps.ts
 *
 * 错误状态步骤：友好报错与原始错误泄露检查。
 *
 * @module steps/web/errors.steps
 */

import { Then } from "./fixtures";
import { expect } from "@playwright/test";
import { ChatInput } from "../../page-objects/chat-input";

Then("页面显示友好错误提示或无原始错误泄露", async ({ page }) => {
  // SSE/错误响应有延迟，与存量 ERR-003 spec 对齐
  await page.waitForTimeout(2_000);
  const chat = new ChatInput(page);

  const errorVisible = await chat.hasFriendlyError();
  const hasRaw = await chat.hasRawError();

  // 要么有友好提示，要么至少没有原始错误泄露
  expect(errorVisible || !hasRaw).toBeTruthy();

  // 有提示时必须不是原始错误
  if (errorVisible) {
    expect(hasRaw).toBeFalsy();
  }
});
