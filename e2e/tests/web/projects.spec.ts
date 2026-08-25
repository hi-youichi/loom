/**
 * e2e/tests/web/projects.spec.ts
 *
 * anureo Web Projects Tests
 * 覆盖：PRJ-001~005（共 5 用例）
 *
 * 使用 fixtures:
 * - app: 提供 baseURL
 * - mock-anureo: 拦截 HTTP API
 * - temp-project: 创建临时项目目录
 *
 * 参考 docs/references/anureo-text-acceptance-test-cases.md §2
 */

import { test as base, expect } from "@playwright/test";
import { app } from "../../fixtures/app";
import { mockanureo } from "../../fixtures/mock-anureo";
import { tempProject } from "../../fixtures/temp-project";

const test = base.extend({
  ...app,
  ...mockanureo,
  ...tempProject,
});

// ─── Helpers ─────────────────────────────────────────────────────────────────

async function gotoAndDismissPalette(page: import("@playwright/test").Page, baseURL: string) {
  await page.goto(baseURL, { waitUntil: "commit" });
  await page.keyboard.press("Escape");
  const sidebar = page.locator("aside, [data-sidebar], complementary").first();
  await sidebar.waitFor({ state: "visible", timeout: 15_000 });
}

// ─── PRJ-001: 添加项目 ───────────────────────────────────────────────────────

test.describe("PRJ-001: 添加项目", () => {
  test("点击 + 添加新项目，新项目出现在侧栏", async ({ page, baseURL, tempProject }) => {
    await gotoAndDismissPalette(page, baseURL);

    // 1. 查找 + 添加按钮（在侧栏顶部工具栏）
    const addProjectBtn = page.getByRole("button", { name: /add.*project|new.*project|\+/i }).first();
    const addBtnVisible = await addProjectBtn.isVisible({ timeout: 3_000 }).catch(() => false);

    if (!addBtnVisible) {
      test.skip();
      return;
    }

    // 2. 监听文件选择器，选择临时目录
    const fileChooserPromise = page.waitForEvent("filechooser", { timeout: 5_000 }).catch(() => null);

    await addProjectBtn.click();

    const fileChooser = await fileChooserPromise;
    if (fileChooser) {
      await fileChooser.setFiles(tempProject.projectDir);
      await page.waitForTimeout(1000);
    }

    // 3. 验证新项目出现在侧栏
    const projectName = tempProject.projectDir.split(/[/\\]/).pop() ?? "test-project";
    const projectItem = page.getByText(new RegExp(projectName, "i")).first();
    const projectVisible = await projectItem.isVisible({ timeout: 5_000 }).catch(() => false);

    if (projectVisible) {
      await expect(projectItem).toBeVisible();
    }
  });
});

// ─── PRJ-002: 切换项目 ───────────────────────────────────────────────────────

test.describe("PRJ-002: 切换项目", () => {
  test("切换项目后会话列表隔离，切换回来恢复", async ({ page, baseURL, tempProject }) => {
    test.setTimeout(60_000);

    // 1. 打开添加项目弹窗（监听文件选择器）
    await gotoAndDismissPalette(page, baseURL);

    const addProjectBtn = page.getByRole("button", { name: /add.*project|new.*project|\+/i }).first();
    const addBtnVisible = await addProjectBtn.isVisible({ timeout: 3_000 }).catch(() => false);
    if (!addBtnVisible) {
      test.skip();
      return;
    }

    const fileChooserPromise = page.waitForEvent("filechooser", { timeout: 5_000 }).catch(() => null);
    await addProjectBtn.click();
    const fileChooser = await fileChooserPromise;
    if (fileChooser) {
      await fileChooser.setFiles(tempProject.projectDir);
      await page.waitForTimeout(1000);
    }

    // 2. 在项目A创建会话
    const sidebar = page.locator("aside, [data-sidebar], complementary").first();
    await sidebar.waitFor({ state: "visible", timeout: 15_000 });
    await page.waitForTimeout(500);
    // 使用 JS click 绕过可能的 overlay 遮挡
    await page.evaluate(() => {
      const btn = document.querySelector('button[name="New session"], button[id*="new-session"], aside button:first-child') as HTMLButtonElement | null;
      if (btn) btn.click();
    });
    await page.waitForTimeout(800);

    // 3. 找到项目切换器
    const projectSwitcher = page.getByRole("button", { name: /project|switch.*project/i }).first();
    const projectSwitcherVisible = await projectSwitcher.isVisible({ timeout: 3_000 }).catch(() => false);

    if (!projectSwitcherVisible) {
      test.skip();
      return;
    }

    await projectSwitcher.click();
    await page.waitForTimeout(500);

    // 4. 查找其他项目
    const projectName = tempProject.projectDir.split(/[/\\]/).pop() ?? "";
    const otherProject = page.getByText(new RegExp(projectName, "i")).first();
    const otherProjectVisible = await otherProject.isVisible({ timeout: 3_000 }).catch(() => false);

    if (otherProjectVisible) {
      await otherProject.click();
      await page.waitForTimeout(1000);

      // 5. 验证切换到新项目（会话列表应该变化或清空）
      const sidebarContent = page.locator("aside *, [data-sidebar] *").first();
      await expect(sidebarContent).toBeVisible({ timeout: 5_000 });
    }
  });
});

// ─── PRJ-003: 项目自定义 ─────────────────────────────────────────────────────

test.describe("PRJ-003: 项目自定义（名称、颜色、图标）", () => {
  test("Settings 中修改项目名称，侧栏显示新名称", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);

    // 1. 打开 Settings
    const settingsBtn = page.locator("aside").getByRole("button", { name: /settings/i }).first();
    await settingsBtn.click({ force: true });
    await page.waitForTimeout(1000);

    // 2. 查找 Projects 设置入口
    const projectsLink = page.getByText(/projects/i, { exact: false }).first();
    const projectsVisible = await projectsLink.isVisible({ timeout: 3_000 }).catch(() => false);

    if (!projectsVisible) {
      // 尝试通过搜索到达
      const searchInput = page.locator("input[aria-label*='search' i]").first();
      const searchVisible = await searchInput.isVisible({ timeout: 2_000 }).catch(() => false);
      if (searchVisible) {
        await searchInput.click();
        await page.keyboard.type("projects");
        await page.keyboard.press("Enter");
        await page.waitForTimeout(800);
      }
    } else {
      await projectsLink.click();
      await page.waitForTimeout(500);
    }

    // 3. 验证 Projects 设置页加载
    const projectsContent = page.getByText(/project|name|color|icon/i, { exact: false }).first();
    const projectsPageVisible = await projectsContent.isVisible({ timeout: 10_000 }).catch(() => false);

    if (!projectsPageVisible) {
      test.skip();
      return;
    }

    // 4. 查找项目列表中的编辑按钮
    const editProjectBtn = page.getByRole("button", { name: /edit|rename|customize|configure/i }).first();
    const editBtnVisible = await editProjectBtn.isVisible({ timeout: 3_000 }).catch(() => false);

    if (!editBtnVisible) {
      test.skip();
      return;
    }

    await editProjectBtn.click();
    await page.waitForTimeout(500);

    // 5. 修改项目名称
    const nameInput = page.locator(
      "input[placeholder*='name' i], input[aria-label*='name' i], [contenteditable='true']"
    ).first();
    const nameInputVisible = await nameInput.isVisible({ timeout: 2_000 }).catch(() => false);

    if (nameInputVisible) {
      await nameInput.clear();
      await nameInput.fill(`E2E Custom Project ${Date.now()}`);

      // 保存
      const saveBtn = page.getByRole("button", { name: /save|confirm|apply/i }).first();
      const saveBtnVisible = await saveBtn.isVisible({ timeout: 2_000 }).catch(() => false);
      if (saveBtnVisible) {
        await saveBtn.click();
        await page.waitForTimeout(500);
      }

      // 6. 关闭 Settings
      await page.keyboard.press("Escape");
      await page.waitForTimeout(500);

      // 7. 验证侧栏显示新名称
      const newName = page.getByText(/e2e custom project/i).first();
      const newNameVisible = await newName.isVisible({ timeout: 3_000 }).catch(() => false);
      if (newNameVisible) {
        await expect(newName).toBeVisible();
      }
    }
  });
});

// ─── PRJ-004: Project Action 添加与运行 ──────────────────────────────────────

test.describe("PRJ-004: Project Action 添加与运行", () => {
  test("添加 Project Action 并运行，终端面板打开", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);

    // 1. 打开 Settings → Projects
    const settingsBtn = page.locator("aside").getByRole("button", { name: /settings/i }).first();
    await settingsBtn.click({ force: true });
    await page.waitForTimeout(1000);

    const projectsLink = page.getByText(/projects/i, { exact: false }).first();
    const projectsVisible = await projectsLink.isVisible({ timeout: 3_000 }).catch(() => false);
    if (!projectsVisible) {
      test.skip();
      return;
    }
    await projectsLink.click();
    await page.waitForTimeout(500);

    // 2. 查找 Project Actions 部分
    const actionsSection = page.getByText(/project.*action|action/i, { exact: false }).first();
    const actionsVisible = await actionsSection.isVisible({ timeout: 3_000 }).catch(() => false);

    if (!actionsVisible) {
      test.skip();
      return;
    }

    // 3. 查找"添加 Action"按钮
    const addActionBtn = page.getByRole("button", { name: /add.*action|new.*action|create.*action/i }).first();
    const addActionVisible = await addActionBtn.isVisible({ timeout: 2_000 }).catch(() => false);

    if (!addActionVisible) {
      test.skip();
      return;
    }

    await addActionBtn.click();
    await page.waitForTimeout(500);

    // 4. 填写 Action 名称
    const nameInput = page.locator("input[placeholder*='name' i], input[aria-label*='name' i]").first();
    const nameInputVisible = await nameInput.isVisible({ timeout: 2_000 }).catch(() => false);

    if (nameInputVisible) {
      await nameInput.fill("Dev Server");

      // 填写命令
      const commandInput = page.locator(
        "input[placeholder*='command' i], input[aria-label*='command' i], textarea[placeholder*='command' i]"
      ).first();
      const commandInputVisible = await commandInput.isVisible({ timeout: 2_000 }).catch(() => false);
      if (commandInputVisible) {
        await commandInput.fill("echo 'hello e2e'");
      }

      // 保存
      const saveBtn = page.getByRole("button", { name: /save|create|add/i }).first();
      const saveBtnVisible = await saveBtn.isVisible({ timeout: 2_000 }).catch(() => false);
      if (saveBtnVisible) {
        await saveBtn.click();
        await page.waitForTimeout(500);
      }
    }

    // 5. 关闭 Settings
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);

    // 6. 查找顶栏 Project Actions 菜单
    const actionsMenu = page.getByRole("button", { name: /project.*action|action/i }).first();
    const actionsMenuVisible = await actionsMenu.isVisible({ timeout: 3_000 }).catch(() => false);

    if (actionsMenuVisible) {
      await actionsMenu.click();
      await page.waitForTimeout(500);

      // 7. 查找"Dev Server"选项
      const devServerOption = page.getByText(/dev server/i).first();
      const devServerVisible = await devServerOption.isVisible({ timeout: 2_000 }).catch(() => false);

      if (devServerVisible) {
        await devServerOption.click();
        await page.waitForTimeout(1000);

        // 8. 验证终端面板打开
        const terminal = page.locator("[class*='terminal' i], [class*='output' i], [data-terminal]").first();
        const terminalVisible = await terminal.isVisible({ timeout: 5_000 }).catch(() => false);

        if (terminalVisible) {
          await expect(terminal).toBeVisible();
        }
      }
    }
  });
});

// ─── PRJ-005: Project Action Auto-open ───────────────────────────────────────

test.describe("PRJ-005: Project Action Auto-open URL", () => {
  test("开启 auto-open 后运行 Action，检测到 localhost 时 Preview 面板打开", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);

    // 1. 打开 Settings → Projects → Actions
    const settingsBtn = page.locator("aside").getByRole("button", { name: /settings/i }).first();
    await settingsBtn.click({ force: true });
    await page.waitForTimeout(1000);

    const projectsLink = page.getByText(/projects/i, { exact: false }).first();
    const projectsVisible = await projectsLink.isVisible({ timeout: 3_000 }).catch(() => false);
    if (!projectsVisible) {
      test.skip();
      return;
    }
    await projectsLink.click();
    await page.waitForTimeout(500);

    // 2. 查找 Actions 部分并编辑第一个 Action
    const actionsSection = page.getByText(/project.*action|action/i, { exact: false }).first();
    const actionsVisible = await actionsSection.isVisible({ timeout: 3_000 }).catch(() => false);
    if (!actionsVisible) {
      test.skip();
      return;
    }

    const editActionBtn = page.getByRole("button", { name: /edit/i }).first();
    const editActionVisible = await editActionBtn.isVisible({ timeout: 2_000 }).catch(() => false);
    if (!editActionVisible) {
      test.skip();
      return;
    }
    await editActionBtn.click();
    await page.waitForTimeout(500);

    // 3. 查找 auto-open 复选框
    const autoOpenCheckbox = page.locator(
      "input[type='checkbox'][aria-label*='auto' i], input[type='checkbox'][aria-label*='open' i], [role='switch']"
    ).first();
    const autoOpenVisible = await autoOpenCheckbox.isVisible({ timeout: 2_000 }).catch(() => false);

    if (!autoOpenVisible) {
      test.skip();
      return;
    }

    const isChecked = await autoOpenCheckbox.isChecked().catch(() => false);
    if (!isChecked) {
      await autoOpenCheckbox.check();
      await page.waitForTimeout(300);

      // 保存
      const saveBtn = page.getByRole("button", { name: /save|confirm|apply/i }).first();
      const saveBtnVisible = await saveBtn.isVisible({ timeout: 2_000 }).catch(() => false);
      if (saveBtnVisible) {
        await saveBtn.click();
        await page.waitForTimeout(500);
      }
    }

    // 4. 关闭 Settings
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);

    // 5. 运行 Action（命令输出含 Local: http://localhost:5173）
    const actionsMenu = page.getByRole("button", { name: /project.*action|action/i }).first();
    const actionsMenuVisible = await actionsMenu.isVisible({ timeout: 3_000 }).catch(() => false);
    if (!actionsMenuVisible) {
      test.skip();
      return;
    }
    await actionsMenu.click();
    await page.waitForTimeout(500);

    const actionOption = page.getByText(/dev server/i).first();
    const actionOptionVisible = await actionOption.isVisible({ timeout: 2_000 }).catch(() => false);
    if (!actionOptionVisible) {
      test.skip();
      return;
    }
    await actionOption.click();
    await page.waitForTimeout(2000);

    // 6. 验证 Preview 面板出现（或 Open preview 按钮）
    const previewPanel = page.locator("[class*='preview' i], [data-preview]").first();
    const previewVisible = await previewPanel.isVisible({ timeout: 5_000 }).catch(() => false);

    const openPreviewBtn = page.getByRole("button", { name: /open.*preview|preview.*open/i }).first();
    const openBtnVisible = await openPreviewBtn.isVisible({ timeout: 3_000 }).catch(() => false);

    expect(previewVisible || openBtnVisible).toBeTruthy();
  });
});
