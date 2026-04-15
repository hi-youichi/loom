import { test, expect } from '@playwright/test'

test.describe('Workspace Management', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
  })

  test('should display workspace selector', async ({ page }) => {
    const workspaceSelector = page.locator('[data-testid="workspace-selector"]')
    await expect(workspaceSelector).toBeVisible()
  })

  test('should create new workspace', async ({ page }) => {
    await page.click('[data-testid="workspace-selector"]')

    await page.click('[data-testid="workspace-create-btn"]')

    const workspaceName = `Test Workspace ${Date.now()}`
    await page.fill('[data-testid="workspace-create-input"]', workspaceName)

    await page.locator('[data-testid="workspace-create-input"]').press('Enter')

    await page.waitForTimeout(1000)

    const selectedName = page.locator('[data-testid="selected-workspace-name"]')
    await expect(selectedName).toHaveText(workspaceName)
  })

  test('should switch between workspaces', async ({ page }) => {
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="workspace-create-btn"]')
    await page.fill('[data-testid="workspace-create-input"]', 'Workspace A')
    await page.locator('[data-testid="workspace-create-input"]').press('Enter')
    await page.waitForTimeout(1000)

    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="workspace-create-btn"]')
    await page.fill('[data-testid="workspace-create-input"]', 'Workspace B')
    await page.locator('[data-testid="workspace-create-input"]').press('Enter')
    await page.waitForTimeout(1000)

    await page.click('[data-testid="workspace-selector"]')
    const items = page.locator('[data-testid^="workspace-item-"]')
    const count = await items.count()
    expect(count).toBeGreaterThanOrEqual(2)

    await items.first().click()
    await page.waitForTimeout(500)

    const selectedName = page.locator('[data-testid="selected-workspace-name"]')
    await expect(selectedName).toBeVisible()
  })

  test('should cancel workspace creation', async ({ page }) => {
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="workspace-create-btn"]')

    await expect(page.locator('[data-testid="workspace-create-input"]')).toBeVisible()

    await page.fill('[data-testid="workspace-create-input"]', 'Cancelled Workspace')

    await page.locator('[data-testid="workspace-selector"]').click({ force: true })

    const input = page.locator('[data-testid="workspace-create-input"]')
    await expect(input).toHaveCount(0)
  })
})
