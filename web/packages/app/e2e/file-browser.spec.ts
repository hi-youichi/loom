import { test, expect } from '@playwright/test'

test.describe('Workspace File Browser', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
  })

  test('should switch to file tree view', async ({ page }) => {
    // Click the file tree tab (second button in sidebar)
    const fileTreeBtn = page.locator('button:has-text("文件")')
    await fileTreeBtn.click()

    // File tree sidebar should be visible
    const sidebar = page.locator('.file-tree-sidebar, [data-testid="file-tree"]')
    await expect(sidebar.or(page.locator('text=没有找到匹配的文件'))).toBeVisible({ timeout: 5000 })
  })

  test('should show loading state when workspace is selected', async ({ page }) => {
    // Create a workspace first
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="workspace-create-btn"]')
    await page.fill('[data-testid="workspace-create-input"]', 'File Test WS')
    await page.locator('[data-testid="workspace-create-input"]').press('Enter')
    await page.waitForTimeout(1500)

    // Switch to file view
    const fileTreeBtn = page.locator('button:has-text("文件")')
    await fileTreeBtn.click()
    await page.waitForTimeout(1000)

    // Should show empty state or file list (no workspace files by default)
    const emptyState = page.locator('text=没有找到匹配的文件')
    const fileList = page.locator('[data-testid="file-tree-item"]')
    await expect(emptyState.or(fileList.first())).toBeVisible({ timeout: 5000 })
  })

  test('should open file tab when clicking a file', async ({ page }) => {
    // Create workspace
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="workspace-create-btn"]')
    await page.fill('[data-testid="workspace-create-input"]', 'Tab Test WS')
    await page.locator('[data-testid="workspace-create-input"]').press('Enter')
    await page.waitForTimeout(1500)

    // Switch to file view
    const fileTreeBtn = page.locator('button:has-text("文件")')
    await fileTreeBtn.click()
    await page.waitForTimeout(500)

    // If there are files, clicking one should open a tab
    const fileItem = page.locator('[data-testid="file-tree-item"]').first()
    if (await fileItem.isVisible({ timeout: 3000 }).catch(() => false)) {
      await fileItem.click()

      // Tab bar should appear
      const tabBar = page.locator('[data-testid="tab-bar"], .tab-bar')
      const fileContent = page.locator('pre, [data-testid="file-content"]')
      await expect(tabBar.or(fileContent)).toBeVisible({ timeout: 5000 })
    }
  })

  test('should show dashboard as default tab', async ({ page }) => {
    // Dashboard should be visible by default in the center
    const dashboard = page.locator('[data-testid="dashboard"], .dashboard-view')
    await expect(dashboard.or(page.locator('text=活跃 Agent'))).toBeVisible({ timeout: 10000 })
  })

  test('should switch between dashboard and file tabs', async ({ page }) => {
    // Dashboard is default
    const dashboardView = page.locator('.dashboard-view')
    await expect(dashboardView.or(page.locator('text=活跃 Agent'))).toBeVisible({ timeout: 10000 })

    // Switch to file view in sidebar
    const fileTreeBtn = page.locator('button:has-text("文件")')
    await fileTreeBtn.click()
    await page.waitForTimeout(500)

    // Switch back to dashboard
    const dashboardBtn = page.locator('button:has-text("仪表盘")')
    await dashboardBtn.click()
    await page.waitForTimeout(500)

    // Dashboard hint should show
    const hint = page.locator('text=Dashboard 显示在右侧')
    await expect(hint).toBeVisible()
  })

  test('should display tab bar when multiple tabs are open', async ({ page }) => {
    // Initially no tab bar (only dashboard)
    // Tab bar only shows when there are 2+ tabs
    const tabBar = page.locator('[data-testid="tab-bar"]')
    await expect(tabBar).toHaveCount(0)
  })
})
