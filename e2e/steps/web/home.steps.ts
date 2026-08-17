/**
 * e2e/steps/web/home.steps.ts
 *
 * 首页首屏场景步骤：空状态验证。
 *
 * @module steps/web/home.steps
 */

import { Then } from "./fixtures";
import { expect } from "@playwright/test";
import { AppPage } from "../../page-objects/app-page";
import { Sidebar } from "../../page-objects/sidebar";

Then("页面显示友好空状态提示或侧栏内容", async ({ page }) => {
  const app = new AppPage(page);
  const sidebar = new Sidebar(page);

  const hasHint = await app.hasEmptyStateHint();
  const hasSidebarContent = hasHint || (await sidebar.hasContent());

  expect(hasHint || hasSidebarContent).toBeTruthy();
});
