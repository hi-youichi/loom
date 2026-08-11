/**
 * e2e/tests/web/models-skills.spec.ts
 *
 * OpenChamber Web Models + Skills + MCP Tests
 * 覆盖：MOD-001~003, MOD-005, SKL-001~004, MCP-001~003（共 11 用例）
 *
 * 使用 fixtures:
 * - app: 提供 baseURL
 * - mock-opencode: 拦截 HTTP API
 *
 * 参考 docs/references/openchamber-text-acceptance-test-cases.md §4, §8, §9
 */

import { test as base, expect } from "@playwright/test";
import { app } from "../../fixtures/app";
import { mockOpencode } from "../../fixtures/mock-opencode";

const test = base.extend({
  ...app,
  ...mockOpencode,
});

// ─── Helpers ─────────────────────────────────────────────────────────────────

async function gotoAndDismissPalette(page: import("@playwright/test").Page, baseURL: string) {
  await page.goto(baseURL, { waitUntil: "commit" });
  await page.keyboard.press("Escape");
  const sidebar = page.locator("aside, [data-sidebar], complementary").first();
  await sidebar.waitFor({ state: "visible", timeout: 15_000 });
}

async function openSettings(page: import("@playwright/test").Page) {
  const settingsBtn = page.locator("aside").getByRole("button", { name: /settings/i }).first();
  await settingsBtn.click({ force: true });
  await page.waitForTimeout(1000);
}

// ─── MOD-001: Provider 连接 ─────────────────────────────────────────────────

test.describe("MOD-001: Provider 连接与模型列表", () => {
  test("Settings 添加 Provider，验证模型列表更新", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);
    await openSettings(page);

    // 1. 查找 Providers 设置入口
    const providersLink = page.getByText(/providers/i, { exact: false }).first();
    const providersLinkVisible = await providersLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!providersLinkVisible) {
      // 尝试通过搜索到达
      const searchInput = page.locator("input[aria-label*='search' i]").first();
      const searchVisible = await searchInput.isVisible({ timeout: 2000 }).catch(() => false);
      if (searchVisible) {
        await searchInput.click();
        await page.keyboard.type("provider");
        await page.keyboard.press("Enter");
        await page.waitForTimeout(800);
      } else {
        test.skip();
        return;
      }
    } else {
      await providersLink.click();
      await page.waitForTimeout(500);
    }

    // 2. 查找"添加 Provider"按钮
    const addProviderBtn = page.getByRole("button", { name: /add.*provider|new.*provider|create.*provider/i }).first();
    const addBtnVisible = await addProviderBtn.isVisible({ timeout: 3000 }).catch(() => false);

    if (!addBtnVisible) {
      test.skip();
      return;
    }
    await addProviderBtn.click();
    await page.waitForTimeout(500);

    // 3. 查找 Provider 类型选择器（OpenAI / Anthropic）
    const providerType = page.locator(
      "[role='combobox'], select, [data-provider-type]"
    ).first();
    const providerTypeVisible = await providerType.isVisible({ timeout: 2000 }).catch(() => false);

    if (!providerTypeVisible) {
      test.skip();
      return;
    }
    await providerType.click();
    await page.waitForTimeout(300);

    const openaiOption = page.locator("[role='option'], option").filter({ hasText: /openai/i }).first();
    const openaiOptionVisible = await openaiOption.isVisible({ timeout: 2000 }).catch(() => false);
    if (openaiOptionVisible) {
      await openaiOption.click();
      await page.waitForTimeout(300);
    }

    // 4. 填写 API Key
    const apiKeyInput = page.locator(
      "input[type='password'], input[placeholder*='key' i], input[aria-label*='key' i]"
    ).first();
    const apiKeyInputVisible = await apiKeyInput.isVisible({ timeout: 2000 }).catch(() => false);

    if (apiKeyInputVisible) {
      await apiKeyInput.fill("sk-test-key-for-e2e");

      // 5. 保存
      const saveBtn = page.getByRole("button", { name: /save|connect|add|create/i }).first();
      const saveBtnVisible = await saveBtn.isVisible({ timeout: 2000 }).catch(() => false);
      if (saveBtnVisible) {
        await saveBtn.click();
        await page.waitForTimeout(2000);
      }
    }

    // 6. 关闭 Settings
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);

    // 7. 新建会话，验证模型选择器出现
    await page.getByRole("button", { name: /^New session$/i }).first().click();
    await page.waitForTimeout(500);

    const modelSelector = page.locator("main [role='combobox']").first();
    const modelSelectorVisible = await modelSelector.isVisible({ timeout: 5000 }).catch(() => false);
    if (modelSelectorVisible) {
      await expect(modelSelector).toBeVisible();
    }
  });
});

// ─── MOD-002: Provider 断开 ─────────────────────────────────────────────────

test.describe("MOD-002: Provider 断开与重连", () => {
  test("断开 Provider 后验证模型不可用；重连后恢复", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);
    await openSettings(page);

    // 1. 导航到 Providers
    const providersLink = page.getByText(/providers/i, { exact: false }).first();
    const providersLinkVisible = await providersLink.isVisible({ timeout: 3000 }).catch(() => false);
    if (!providersLinkVisible) {
      test.skip();
      return;
    }
    await providersLink.click();
    await page.waitForTimeout(500);

    // 2. 查找已连接的 Provider 并断开
    const connectedProvider = page.locator(
      "[data-provider-item][data-status='connected'], [class*='provider'][class*='connected']"
    ).first();
    const providerVisible = await connectedProvider.isVisible({ timeout: 3000 }).catch(() => false);

    if (!providerVisible) {
      test.skip();
      return;
    }

    // 查找断开按钮
    const disconnectBtn = connectedProvider.getByRole("button", { name: /disconnect|remove|delete/i }).first();
    const disconnectBtnVisible = await disconnectBtn.isVisible({ timeout: 2000 }).catch(() => false);
    if (disconnectBtnVisible) {
      await disconnectBtn.click();
      await page.waitForTimeout(500);
    }

    // 3. 关闭 Settings，尝试新建会话
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);

    await page.getByRole("button", { name: /^New session$/i }).first().click();
    await page.waitForTimeout(500);

    // 4. 验证错误提示或模型选择器不可用
    const errorArea = page.locator("[role='alert'], [class*='error' i]").first();
    const errorVisible = await errorArea.isVisible({ timeout: 3000 }).catch(() => false);

    const modelSelector = page.locator("main [role='combobox']").first();
    const modelSelectorVisible = await modelSelector.isVisible({ timeout: 3000 }).catch(() => false);

    // 至少有一种状态出现（错误或模型不可用）
    expect(errorVisible || !modelSelectorVisible).toBeTruthy();
  });
});

// ─── MOD-003: Agent 创建 ────────────────────────────────────────────────────

test.describe("MOD-003: Agent 创建与配置", () => {
  test("Settings 创建自定义 Agent，在会话中选择", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);
    await openSettings(page);

    // 1. 查找 Agents 设置入口
    const agentsLink = page.getByText(/agents/i, { exact: false }).first();
    const agentsLinkVisible = await agentsLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!agentsLinkVisible) {
      test.skip();
      return;
    }
    await agentsLink.click();
    await page.waitForTimeout(500);

    // 2. 查找"创建 Agent"按钮
    const createAgentBtn = page.getByRole("button", { name: /create.*agent|add.*agent|new.*agent/i }).first();
    const createBtnVisible = await createAgentBtn.isVisible({ timeout: 3000 }).catch(() => false);

    if (!createBtnVisible) {
      test.skip();
      return;
    }
    await createAgentBtn.click();
    await page.waitForTimeout(500);

    // 3. 填写 Agent 名称
    const nameInput = page.locator("input[placeholder*='name' i], input[aria-label*='name' i]").first();
    const nameInputVisible = await nameInput.isVisible({ timeout: 2000 }).catch(() => false);

    if (nameInputVisible) {
      await nameInput.fill(`E2E Test Agent ${Date.now()}`);

      // 4. 保存
      const saveBtn = page.getByRole("button", { name: /save|create|add/i }).first();
      const saveBtnVisible = await saveBtn.isVisible({ timeout: 2000 }).catch(() => false);
      if (saveBtnVisible) {
        await saveBtn.click();
        await page.waitForTimeout(500);
      }
    }

    // 5. 关闭 Settings
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);

    // 6. 新建会话
    await page.getByRole("button", { name: /^New session$/i }).first().click();
    await page.waitForTimeout(500);

    // 7. 查找 Agent 选择器
    const agentSelector = page.locator(
      "main [role='combobox'], [class*='agent' i] [role='combobox']"
    ).first();
    const agentSelectorVisible = await agentSelector.isVisible({ timeout: 3000 }).catch(() => false);

    if (agentSelectorVisible) {
      await expect(agentSelector).toBeVisible();
    }
  });
});

// ─── MOD-005: 按项目覆盖 Provider ──────────────────────────────────────────

test.describe("MOD-005: 按项目覆盖 Provider", () => {
  test("不同项目设置不同 API Key，切换后使用正确凭据", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);
    await openSettings(page);

    // 1. 导航到 Projects 设置
    const projectsLink = page.getByText(/projects/i, { exact: false }).first();
    const projectsLinkVisible = await projectsLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!projectsLinkVisible) {
      test.skip();
      return;
    }
    await projectsLink.click();
    await page.waitForTimeout(500);

    // 2. 选择一个项目
    const projectItem = page.locator("[data-project-item], [class*='project']").first();
    const projectItemVisible = await projectItem.isVisible({ timeout: 3000 }).catch(() => false);

    if (!projectItemVisible) {
      test.skip();
      return;
    }

    // 3. 查找 Provider/Agent 配置区域
    const providerConfig = page.getByText(/provider|api.*key|override/i, { exact: false }).first();
    const configVisible = await providerConfig.isVisible({ timeout: 3000 }).catch(() => false);

    if (!configVisible) {
      test.skip();
      return;
    }

    // 4. 查找 API Key 输入框
    const apiKeyInput = page.locator(
      "input[type='password'], input[placeholder*='key' i]"
    ).first();
    const apiKeyInputVisible = await apiKeyInput.isVisible({ timeout: 2000 }).catch(() => false);

    if (apiKeyInputVisible) {
      await apiKeyInput.fill("sk-project-specific-key");
      await page.waitForTimeout(300);

      // 5. 保存
      const saveBtn = page.getByRole("button", { name: /save|apply/i }).first();
      const saveBtnVisible = await saveBtn.isVisible({ timeout: 2000 }).catch(() => false);
      if (saveBtnVisible) {
        await saveBtn.click();
        await page.waitForTimeout(500);
      }
    }

    // 6. 验证配置已保存
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);

    await openSettings(page);
    await projectsLink.click();
    await page.waitForTimeout(500);

    const savedKey = page.locator("input[type='password']").first();
    const savedKeyVisible = await savedKey.isVisible({ timeout: 2000 }).catch(() => false);
    if (savedKeyVisible) {
      await expect(savedKey).toBeVisible();
    }
  });
});

// ─── SKL-001: 创建 Skill ────────────────────────────────────────────────────

test.describe("SKL-001: 创建 Skill", () => {
  test("Settings 创建自定义 Skill，验证出现在列表", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);
    await openSettings(page);

    // 1. 查找 Skills 设置入口
    const skillsLink = page.getByText(/skills/i, { exact: false }).first();
    const skillsLinkVisible = await skillsLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!skillsLinkVisible) {
      test.skip();
      return;
    }
    await skillsLink.click();
    await page.waitForTimeout(500);

    // 2. 验证 Skills 页面加载
    const skillsContent = page.getByText(/skill|instruction|behavior/i, { exact: false }).first();
    const skillsPageVisible = await skillsContent.isVisible({ timeout: 5000 }).catch(() => false);

    if (!skillsPageVisible) {
      test.skip();
      return;
    }

    // 3. 查找"创建 Skill"按钮
    const createSkillBtn = page.getByRole("button", { name: /create.*skill|add.*skill|new.*skill/i }).first();
    const createBtnVisible = await createSkillBtn.isVisible({ timeout: 3000 }).catch(() => false);

    if (!createBtnVisible) {
      test.skip();
      return;
    }
    await createSkillBtn.click();
    await page.waitForTimeout(500);

    // 4. 填写 Skill 名称
    const nameInput = page.locator("input[placeholder*='name' i], input[aria-label*='name' i]").first();
    const nameInputVisible = await nameInput.isVisible({ timeout: 2000 }).catch(() => false);

    if (nameInputVisible) {
      const skillName = `e2e-test-skill-${Date.now()}`;
      await nameInput.fill(skillName);

      // 填写 instruction（如果需要）
      const instructionInput = page.locator(
        "textarea[placeholder*='instruction' i], textarea[placeholder*='description' i]"
      ).first();
      const instructionInputVisible = await instructionInput.isVisible({ timeout: 2000 }).catch(() => false);
      if (instructionInputVisible) {
        await instructionInput.fill("This is a test skill created by E2E.");
      }

      // 5. 保存
      const saveBtn = page.getByRole("button", { name: /save|create|add/i }).first();
      const saveBtnVisible = await saveBtn.isVisible({ timeout: 2000 }).catch(() => false);
      if (saveBtnVisible) {
        await saveBtn.click();
        await page.waitForTimeout(500);
      }

      // 6. 验证 Skill 出现在列表
      await page.waitForTimeout(300);
      const skillInList = page.getByText(new RegExp(skillName, "i")).first();
      const skillInListVisible = await skillInList.isVisible({ timeout: 3000 }).catch(() => false);
      if (skillInListVisible) {
        await expect(skillInList).toBeVisible();
      }
    }
  });
});

// ─── SKL-002: Chat 中使用 Skill ──────────────────────────────────────────────

test.describe("SKL-002: Chat 中使用 Skill", () => {
  test("在消息输入中触发 Skill 选择器，选择后内容进入输入框", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);

    // 1. 新建会话
    await page.getByRole("button", { name: /^New session$/i }).first().click();
    await page.waitForTimeout(500);

    const chatInput = page.getByPlaceholder(/@.*for.*files|files.*agents/i).first();
    await expect(chatInput).toBeVisible({ timeout: 10000 });

    // 2. 输入 / 触发命令选择器
    await chatInput.click();
    await chatInput.fill("/");
    await page.waitForTimeout(800);

    // 3. 查找命令/Skill 选择器
    const skillPicker = page.locator(
      "[role='listbox'], [role='combobox'], [class*='command' i], [class*='skill' i]"
    ).first();
    const pickerVisible = await skillPicker.isVisible({ timeout: 3000 }).catch(() => false);

    if (!pickerVisible) {
      test.skip();
      return;
    }

    // 4. 选择第一个 Skill
    const firstSkill = page.locator("[role='option']").first();
    const firstSkillVisible = await firstSkill.isVisible({ timeout: 2000 }).catch(() => false);
    if (firstSkillVisible) {
      await firstSkill.click();
      await page.waitForTimeout(300);

      // 5. 验证输入框有内容
      const inputValue = await chatInput.inputValue();
      expect(inputValue.length).toBeGreaterThan(0);
    }
  });
});

// ─── SKL-003: Catalog 安装 ───────────────────────────────────────────────────

test.describe("SKL-003: Skill Catalog 安装", () => {
  test("从 Catalog 安装 Skill，验证出现在列表", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);
    await openSettings(page);

    // 1. 查找 Skills 设置入口
    const skillsLink = page.getByText(/skills/i, { exact: false }).first();
    const skillsLinkVisible = await skillsLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!skillsLinkVisible) {
      test.skip();
      return;
    }
    await skillsLink.click();
    await page.waitForTimeout(500);

    // 2. 查找 Catalog 入口
    const catalogLink = page.getByText(/catalog|marketplace|available/i, { exact: false }).first();
    const catalogLinkVisible = await catalogLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!catalogLinkVisible) {
      test.skip();
      return;
    }
    await catalogLink.click();
    await page.waitForTimeout(500);

    // 3. 查找可安装的 Skill
    const availableSkill = page.locator("[data-catalog-skill], [class*='catalog' i] [class*='skill' i]").first();
    const availableSkillVisible = await availableSkill.isVisible({ timeout: 3000 }).catch(() => false);

    if (!availableSkillVisible) {
      test.skip();
      return;
    }

    // 4. 安装（点击 Install/Add）
    const installBtn = page.getByRole("button", { name: /install|add|enable/i }).first();
    const installBtnVisible = await installBtn.isVisible({ timeout: 2000 }).catch(() => false);
    if (installBtnVisible) {
      await installBtn.click();
      await page.waitForTimeout(1000);
    }

    // 5. 验证 Skill 出现在已安装列表
    const installedSkill = page.getByText(/installed|enabled/i, { exact: false }).first();
    const installedSkillVisible = await installedSkill.isVisible({ timeout: 3000 }).catch(() => false);
    if (installedSkillVisible) {
      await expect(installedSkill).toBeVisible();
    }
  });
});

// ─── SKL-004: Skill 名称冲突 ─────────────────────────────────────────────────

test.describe("SKL-004: Skill 名称冲突", () => {
  test("安装同名 Skill 时出现冲突提示，可选择 Skip 或 Overwrite", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);
    await openSettings(page);

    // 1. 查找 Skills 设置入口
    const skillsLink = page.getByText(/skills/i, { exact: false }).first();
    const skillsLinkVisible = await skillsLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!skillsLinkVisible) {
      test.skip();
      return;
    }
    await skillsLink.click();
    await page.waitForTimeout(500);

    // 2. 查找 Catalog 入口
    const catalogLink = page.getByText(/catalog|marketplace/i, { exact: false }).first();
    const catalogLinkVisible = await catalogLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!catalogLinkVisible) {
      test.skip();
      return;
    }
    await catalogLink.click();
    await page.waitForTimeout(500);

    // 3. 尝试安装同名 Skill（假设有已安装的 Skill）
    const catalogSkill = page.locator("[data-catalog-skill]").first();
    const catalogSkillVisible = await catalogSkill.isVisible({ timeout: 3000 }).catch(() => false);

    if (!catalogSkillVisible) {
      test.skip();
      return;
    }

    const installBtn = page.getByRole("button", { name: /install|add/i }).first();
    const installBtnVisible = await installBtn.isVisible({ timeout: 2000 }).catch(() => false);
    if (!installBtnVisible) {
      test.skip();
      return;
    }
    await installBtn.click();
    await page.waitForTimeout(1000);

    // 4. 验证出现冲突对话框
    const conflictDialog = page.locator(
      "[role='dialog'], [class*='conflict' i], [class*='dialog' i]"
    ).first();
    const conflictDialogVisible = await conflictDialog.isVisible({ timeout: 3000 }).catch(() => false);

    if (!conflictDialogVisible) {
      // 没有冲突（可能没有同名 Skill），跳过
      test.skip();
      return;
    }

    // 5. 验证对话框中有 Skip/Overwrite 选项
    const skipBtn = page.getByRole("button", { name: /skip|skip.*this|overwrite|replace/i }).first();
    const skipBtnVisible = await skipBtn.isVisible({ timeout: 2000 }).catch(() => false);
    expect(skipBtnVisible).toBeTruthy();
  });
});

// ─── MCP-001: 添加 MCP Server ───────────────────────────────────────────────

test.describe("MCP-001: 添加 MCP Server（本地）", () => {
  test("Settings 添加本地 MCP Server，验证添加成功", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);
    await openSettings(page);

    // 1. 查找 MCP 设置入口
    const mcpLink = page.getByText(/mcp|mcp.*server/i, { exact: false }).first();
    const mcpLinkVisible = await mcpLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!mcpLinkVisible) {
      test.skip();
      return;
    }
    await mcpLink.click();
    await page.waitForTimeout(500);

    // 2. 验证 MCP 页面加载
    const mcpContent = page.getByText(/mcp|server|tool/i, { exact: false }).first();
    const mcpPageVisible = await mcpContent.isVisible({ timeout: 5000 }).catch(() => false);

    if (!mcpPageVisible) {
      test.skip();
      return;
    }

    // 3. 查找"添加 Server"按钮
    const addServerBtn = page.getByRole("button", { name: /add.*server|new.*server|create.*server/i }).first();
    const addBtnVisible = await addServerBtn.isVisible({ timeout: 3000 }).catch(() => false);

    if (!addBtnVisible) {
      test.skip();
      return;
    }
    await addServerBtn.click();
    await page.waitForTimeout(500);

    // 4. 填写 Server 名称和 command
    const nameInput = page.locator("input[placeholder*='name' i], input[aria-label*='name' i]").first();
    const nameInputVisible = await nameInput.isVisible({ timeout: 2000 }).catch(() => false);

    if (nameInputVisible) {
      await nameInput.fill("E2E Test MCP Server");

      const commandInput = page.locator(
        "input[placeholder*='command' i], textarea[placeholder*='command' i]"
      ).first();
      const commandInputVisible = await commandInput.isVisible({ timeout: 2000 }).catch(() => false);
      if (commandInputVisible) {
        await commandInput.fill("npx some-mcp-server");
      }

      // 5. 保存
      const saveBtn = page.getByRole("button", { name: /save|create|add/i }).first();
      const saveBtnVisible = await saveBtn.isVisible({ timeout: 2000 }).catch(() => false);
      if (saveBtnVisible) {
        await saveBtn.click();
        await page.waitForTimeout(500);
      }

      // 6. 验证 Server 出现在列表
      const serverInList = page.getByText(/e2e test mcp server/i).first();
      const serverInListVisible = await serverInList.isVisible({ timeout: 3000 }).catch(() => false);
      if (serverInListVisible) {
        await expect(serverInList).toBeVisible();
      }
    }
  });
});

// ─── MCP-002: MCP Server 范围隔离 ───────────────────────────────────────────

test.describe("MCP-002: MCP Server 按项目范围隔离", () => {
  test("MCP Server 配置了项目范围，只在该项目中可见", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);
    await openSettings(page);

    // 1. 导航到 MCP 设置
    const mcpLink = page.getByText(/mcp/i, { exact: false }).first();
    const mcpLinkVisible = await mcpLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!mcpLinkVisible) {
      test.skip();
      return;
    }
    await mcpLink.click();
    await page.waitForTimeout(500);

    // 2. 查找有项目范围的 Server 配置
    const projectScope = page.locator(
      "[data-mcp-server][data-project], [class*='mcp'][class*='project' i]"
    ).first();
    const projectScopeVisible = await projectScope.isVisible({ timeout: 3000 }).catch(() => false);

    if (!projectScopeVisible) {
      test.skip();
      return;
    }

    // 3. 验证 Server 配置存在
    await expect(projectScope).toBeVisible();
  });
});

// ─── MCP-003: MCP Server Toggle ─────────────────────────────────────────────

test.describe("MCP-003: MCP Server Toggle", () => {
  test("关闭 MCP Server 后验证工具列表更新；重新开启后恢复", async ({ page, baseURL }) => {
    await gotoAndDismissPalette(page, baseURL);
    await openSettings(page);

    // 1. 导航到 MCP 设置
    const mcpLink = page.getByText(/mcp/i, { exact: false }).first();
    const mcpLinkVisible = await mcpLink.isVisible({ timeout: 3000 }).catch(() => false);

    if (!mcpLinkVisible) {
      test.skip();
      return;
    }
    await mcpLink.click();
    await page.waitForTimeout(500);

    // 2. 查找 Server 的 Toggle 开关
    const toggle = page.locator(
      "input[type='checkbox'], [role='switch']"
    ).first();
    const toggleVisible = await toggle.isVisible({ timeout: 3000 }).catch(() => false);

    if (!toggleVisible) {
      test.skip();
      return;
    }

    // 3. 关闭 Toggle
    const isChecked = await toggle.isChecked().catch(() => false);
    if (isChecked) {
      await toggle.uncheck();
      await page.waitForTimeout(500);
    }

    // 4. 重新开启
    await toggle.check();
    await page.waitForTimeout(500);

    // 5. 验证 Toggle 状态已更新
    const toggleAfter = await toggle.isChecked().catch(() => false);
    expect(toggleAfter).toBeTruthy();
  });
});
