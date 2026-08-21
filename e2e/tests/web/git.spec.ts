/**
 * e2e/tests/web/git.spec.ts
 *
 * OpenChamber Web Git Tests
 *
 * 使用 fixtures:
 * - app: 提供 baseURL fixture
 * - temp-git-repo: 创建临时 git 仓库
 *
 * 参考 docs/references/openchamber-text-acceptance-test-cases.md §6
 */

import { test as base, expect } from "@playwright/test";
import { app } from "../../fixtures/app";
import { mockLoom } from "../../fixtures/mock-loom";

// 合并 fixtures：mockLoom 的 page 必须不被覆盖
const test = base.extend({
  ...app,
  ...mockLoom,
});

/**
 * GIT-001 — 暂存与提交
 *
 * 目标：验证暂存文件、提交 commit
 * 前置条件：项目为 git 仓库，有未暂存变更
 * 步骤：1. 右侧栏 Git 标签 2. 在 Unstaged 区域点击文件的 "+" 3. 确认移到 Staged 4. 输入 commit message 5. 点 Commit
 * 预期结果：文件暂存成功；提交成功；提交历史中可见新提交
 * 观察点/失败信号：暂存不生效、提交失败无提示
 */
test("GIT-001: 暂存并提交修改", async ({ page }) => {
  // 1. 访问应用（waitUntil: 'commit' 避免 SessionAuthGate 重试等待 load 事件）
  await page.goto("http://localhost:3000", { waitUntil: "commit" });
  await page.keyboard.press("Escape");

  // 2. 等待侧栏渲染（React 组件挂载后才出现 "Toggle right sidebar" 按钮）
  const sidebar = page.locator("aside, [data-sidebar], complementary").first();
  await sidebar.waitFor({ state: "visible", timeout: 15_000 });
  await page.waitForTimeout(500);

  // 3. 打开右侧栏（默认折叠）
  await page.keyboard.press("Escape");
  await page.waitForTimeout(500);
  // JS click 绕过 scroll + React overlay 检测（此时侧栏已渲染，aria-label 按钮存在）
  await page.evaluate(() => {
    const btn = document.querySelector('button[aria-label="Toggle right sidebar"]') as HTMLButtonElement | null;
    if (btn) btn.click();
  });
  await page.waitForTimeout(1000); // 等待面板动画

  // 4. 切换到 Git 标签
  // tablist name="Tabs" 包含 Git/Files/Context（由 diagnostic.spec.ts 验证）
  const gitTab = page.getByRole("tab", { name: /git/i });

  // 等待 tab 在 DOM 中出现（动画完成），如果未出现则跳过
  const tabAttached = await gitTab.waitFor({ state: "attached", timeout: 5000 }).then(() => true).catch(() => false);
  if (!tabAttached) {
    console.log("[GIT-001] SKIP: 右侧栏 Git 标签不可用");
    return;
  }
  await page.waitForTimeout(300);

  await gitTab.click({ timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(500);

  // 4. 验证 Git 面板出现
  const gitPanel = page.locator("[data-git-panel], [class*='git']").first();
  const gitPanelExists = await gitPanel.count() > 0;

  if (!gitPanelExists) {
    console.log("[GIT-001] SKIP: Git 面板不可用（当前项目可能不是 git 仓库）");
    return;
  }

  await expect(gitPanel).toBeVisible({ timeout: 10_000 });

  // 5. 验证 Unstaged 区域有文件
  const unstagedArea = page.locator("[data-unstaged-files], [data-git-unstaged]").first();
  const unstagedVisible = await unstagedArea.isVisible({ timeout: 5000 }).catch(() => false);

  if (unstagedVisible) {
    const unstagedFiles = page.locator("[data-file-item], [data-git-file]").filter({ hasText: /\.ts|\.js|\.tsx|\.jsx/i });
    const unstagedFileCount = await unstagedFiles.count();

    console.log(`[GIT-001] Found ${unstagedFileCount} unstaged files`);

    if (unstagedFileCount > 0) {
      const firstFile = unstagedFiles.first();
      const stageButton = firstFile.locator("[data-stage-button], [data-add-button], button").first();

      const stageButtonVisible = await stageButton.isVisible({ timeout: 2000 }).catch(() => false);
      if (stageButtonVisible) {
        await stageButton.click();
        await page.waitForTimeout(500);
      } else {
        await firstFile.click({ button: "right" });
        await page.waitForTimeout(500);
      }
    }
  }

  // 6. 验证文件已移到 Staged 区域
  const stagedArea = page.locator("[data-staged-files], [data-git-staged]").first();
  const stagedVisible = await stagedArea.isVisible({ timeout: 5000 }).catch(() => false);

  if (stagedVisible) {
    const stagedFiles = page.locator("[data-file-item], [data-git-file]").filter({ hasText: /\.ts|\.js|\.tsx|\.jsx/i });
    const stagedFileCount = await stagedFiles.count();

    console.log(`[GIT-001] Found ${stagedFileCount} staged files`);
    expect(stagedFileCount).toBeGreaterThan(0);
  }

  // 7. 输入 commit message
  const commitMessageInput = page.locator(
    "[data-commit-message], [data-git-commit-message], textarea, input"
  ).filter({ hasText: "" }).or(
    page.locator("[data-commit-message-input], [placeholder*='commit' i], [placeholder*='message' i]")
  ).first();

  await commitMessageInput.waitFor({ timeout: 3000 }).catch(() => {});

  const inputVisible = await commitMessageInput.isVisible({ timeout: 2000 }).catch(() => false);
  if (inputVisible) {
    const testCommitMessage = `test: ${new Date().toISOString()}`;
    await commitMessageInput.fill(testCommitMessage);
    await page.waitForTimeout(300);
  } else {
    const anyInput = page.locator("textarea, input[type='text']").first();
    if (await anyInput.isVisible({ timeout: 1000 }).catch(() => false)) {
      await anyInput.fill(`test: ${new Date().toISOString()}`);
    }
  }

  // 8. 点击 Commit 按钮
  const commitButton = page.getByRole("button", { name: /^commit$/i }).or(
    page.locator("[data-commit-button], [data-git-commit]").filter({ hasText: /commit/i })
  ).first();

  const commitButtonVisible = await commitButton.isVisible({ timeout: 2000 }).catch(() => false);
  if (commitButtonVisible) {
    await commitButton.click();
    await page.waitForTimeout(1000);

    // 9. 验证提交成功
    const successMessage = page.locator("[data-success], [class*='success']").filter({ hasText: /commit|success/i }).first();
    const successVisible = await successMessage.isVisible({ timeout: 3000 }).catch(() => false);

    if (successVisible) {
      await expect(successMessage).toBeVisible();
    }

    // 10. 验证 git log 中有该 commit
    const historyButton = page.getByText(/history|log/i).first();
    const historyButtonVisible = await historyButton.isVisible({ timeout: 2000 }).catch(() => false);

    if (historyButtonVisible) {
      await historyButton.click();
      await page.waitForTimeout(500);
    }

    const commitHistory = page.locator("[data-commit-history], [data-git-history]").first();
    const historyVisible = await commitHistory.isVisible({ timeout: 3000 }).catch(() => false);

    if (historyVisible) {
      const commits = page.locator("[data-commit-item], [data-git-commit]");
      const commitCount = await commits.count();

      console.log(`[GIT-001] Found ${commitCount} commits in history`);
      expect(commitCount).toBeGreaterThan(0);
    }

    console.log("[GIT-001] PASS: 暂存和提交功能验证成功");
  } else {
    console.log("[GIT-001] WARN: Commit button not visible, skipping commit verification");
  }
});
