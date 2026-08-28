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

When("我选择第一个可用模型", async ({ page }) => {
  await new ChatInput(page).selectFirstAvailableModel();
});

When("我在输入框输入消息 {string}", async ({ page }, text: string) => {
  const chat = new ChatInput(page);
  await chat.waitForInput();
  await chat.fill(text);
});

When("我发送消息", async ({ page }) => {
  await new ChatInput(page).send();
});

When("我发送消息 {string}", async ({ page, chatState }, text: string) => {
  const chat = new ChatInput(page);
  await chat.waitForInput();
  // A new-session draft can briefly retain the previous session's rendered
  // assistant messages while the session switch settles. The new session is
  // logically empty, so use zero as the baseline instead of counting stale
  // DOM nodes from the previous session.
  chatState.assistantCountBeforeSend = 0;
  chatState.sentText = text;
  await chat.fill(text);
  await chat.selectFirstAvailableModel();
  await chat.send();
});

Then("用户消息 {string} 出现在聊天区", async ({ page }, text: string) => {
  const chat = new ChatInput(page);
  await expect(chat.userMessages.last()).toContainText(text, {
    timeout: 10_000,
  });
});

Then("我等待助手回复完成", async ({ page, chatState }) => {
  const chat = new ChatInput(page);
  await chat.waitForNewAssistantMessage(chatState.assistantCountBeforeSend);
  await chat.waitForReplyComplete();
});

Then("助手回复出现在聊天区", async ({ page, chatState }) => {
  const chat = new ChatInput(page);
  await expect
    .poll(() => chat.assistantMessages.count(), { timeout: 90_000 })
    .toBeGreaterThan(chatState.assistantCountBeforeSend);
  await expect(chat.assistantMessages.last()).toBeVisible();
});

Then("助手回复内容不为空", async ({ page }) => {
  const chat = new ChatInput(page);
  await expect.poll(() => chat.lastAssistantText(), {
    timeout: 90_000,
    message: "等待助手生成非空回复",
}).toBeTruthy();
});

Then("聊天区包含助手消息", async ({ page }) => {
  const chat = new ChatInput(page);
  await expect(chat.assistantMessages.first()).toBeVisible({ timeout: 90_000 });
});

Then("消息 {string} 出现在聊天区或输入框已清空", async ({ page }, text: string) => {
  const chat = new ChatInput(page);
  await page.waitForTimeout(500);
  const messageVisible = await chat.isMessageVisible(text);
  const inputCleared = await chat.isInputCleared();
  expect(messageVisible || inputCleared).toBeTruthy();
});
