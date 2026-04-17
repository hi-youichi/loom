import { test, expect } from '@playwright/test'

async function goToSessionsTab(page: import('@playwright/test').Page) {
  const sessionsTab = page.locator('button.tab-item:has-text("会话")')
  await sessionsTab.click()
  await page.waitForTimeout(500)
}

async function selectModel(page: import('@playwright/test').Page) {
  await page.click('.model-selector__trigger')
  await page.waitForSelector('.model-selector__content button')
  await page.locator('.model-selector__content button').first().click()
}

async function sendMessage(page: import('@playwright/test').Page, message: string) {
  await page.fill('.composer__input', message)
  await page.click('.composer__button')
  await page.waitForSelector('.message--user', { timeout: 10000 })
  await page.waitForSelector('.message--assistant', { timeout: 30000 })
}

async function createWorkspace(page: import('@playwright/test').Page, name: string) {
  await page.click('[data-testid="workspace-selector"]')
  await page.waitForTimeout(300)
  await page.click('[data-testid="workspace-create-btn"]')
  await page.fill('[data-testid="workspace-create-input"]', name)
  await page.locator('[data-testid="workspace-create-input"]').press('Enter')
  await page.waitForTimeout(1000)
}

test.describe('Workspace Thread List', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
  })

  test('新建工作空间后发送消息', async ({ page }) => {
    await selectModel(page)
    await createWorkspace(page, 'test-ws-1')

    await sendMessage(page, 'Hello from workspace')

    await expect(page.locator('.message--user')).toContainText('Hello from workspace')
    await expect(page.locator('.message--assistant')).toBeVisible()
  })

  test('工作空间 Session 在列表中显示', async ({ page }) => {
    await selectModel(page)
    await createWorkspace(page, 'test-ws-2')

    await sendMessage(page, 'Thread in workspace')

    await goToSessionsTab(page)

    const cards = page.locator('[data-testid="session-list"] [data-testid^="session-card-"]')
    await expect(cards).toHaveCount(1)
  })

  test.skip('thread 在刷新后保留', async ({ page }) => {
    await selectModel(page)
    await createWorkspace(page, 'test-persist')

    await sendMessage(page, 'Persistent thread')

    await goToSessionsTab(page)
    const cards = page.locator('[data-testid="session-list"] [data-testid^="session-card-"]')
    await expect(cards).toHaveCount(1)

    await page.reload()
    await page.waitForSelector('.composer')
    await goToSessionsTab(page)
    await expect(cards).toHaveCount(1)
  })
})
