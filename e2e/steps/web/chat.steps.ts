/**
 * e2e/steps/web/chat.steps.ts
 *
 * 聊天类步骤：新建会话、输入、发送、消息可见性。
 *
 * @module steps/web/chat.steps
 */

import { When, Then } from "./fixtures";
import { expect } from "@playwright/test";
import { ChatInput } from "../../page-objects/chat-input";

When("我点击新建会话按钮", async ({ page }) => {
  await new ChatInput(page).clickNewSession();
});

When("我在输入框输入消息 {string}", async ({ page }, text: string) => {
  const chat = new ChatInput(page);
  await chat.waitForInput();
  await chat.fill(text);
});

When("我发送消息", async ({ page }) => {
  await new ChatInput(page).send();
});

Then("消息 {string} 出现在聊天区或输入框已清空", async ({ page }, text: string) => {
  const chat = new ChatInput(page);
  await page.waitForTimeout(500);
  const messageVisible = await chat.isMessageVisible(text);
  const inputCleared = await chat.isInputCleared();
  expect(messageVisible || inputCleared).toBeTruthy();
});
