/**
 * e2e/steps/web/navigation.steps.ts
 *
 * 导航类步骤：进入应用、刷新、侧栏可见性。
 *
 * @module steps/web/navigation.steps
 */

import { Given, When, Then } from "./fixtures";
import { expect } from "@playwright/test";
import { AppPage } from "../../page-objects/app-page";
import { Sidebar } from "../../page-objects/sidebar";

Given("我已进入应用", async ({ page }) => {
  await new AppPage(page).open();
});

When("我刷新页面", async ({ page }) => {
  await new AppPage(page).reload();
});

Then("侧栏可见", async ({ page }) => {
  await new Sidebar(page).waitForVisible();
});

Then("侧栏包含会话操作按钮", async ({ page }) => {
  expect(await new Sidebar(page).hasInteractiveButtons()).toBeTruthy();
});
