import { test, expect } from '@playwright/test'

async function goToSessionsTab(page: import('@playwright/test').Page) {
  const sessionsTab = page.locator('button.tab-item:has-text("会话")')
  await sessionsTab.click()
  await page.waitForTimeout(300)
}

test.describe('Session Full Flow', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.evaluate(() => {
      localStorage.removeItem('loom-sessions')
      localStorage.removeItem('loom-active-session')
    })
    await page.reload()

    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
  })

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

  test('创建 Session 并发送消息 - 完整流程', async ({ page }) => {
    await selectModel(page)

    const testMessage = 'Hello, this is a test message for session creation'
    await sendMessage(page, testMessage)

    await expect(page.locator('.message--user')).toContainText(testMessage)
    await expect(page.locator('.message--assistant')).toBeVisible()
  })

  test('发送多条消息更新 Session 元数据', async ({ page }) => {
    await selectModel(page)

    for (let i = 1; i <= 3; i++) {
      await page.fill('.composer__input', `Message ${i}`)
      await page.click('.composer__button')
      await page.waitForSelector(`.message--user`, { timeout: 10000 })
      await page.waitForSelector('.message--assistant', { timeout: 30000 })
      await page.waitForTimeout(500)
    }

    const userMessages = page.locator('.message--user')
    const assistantMessages = page.locator('.message--assistant')
    await expect(userMessages).toHaveCount(3)
    await expect(assistantMessages).toHaveCount(3)
  })

  test('Session 元数据在 Dashboard 中可见', async ({ page }) => {
    await selectModel(page)

    const testMessage = 'Check session metadata'
    await sendMessage(page, testMessage)

    await goToSessionsTab(page)

    const sessionCards = page.locator('[data-testid="session-list"] [data-testid^="session-card-"]')
    await expect(sessionCards).toHaveCount(1)

    const title = await sessionCards.first().locator('[data-testid="session-card__title"]').textContent()
    expect(title).toBeTruthy()
  })
})
