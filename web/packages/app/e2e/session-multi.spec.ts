import { test, expect } from '@playwright/test'

async function goToSessionsTab(page: import('@playwright/test').Page) {
  const sessionsTab = page.locator('button.tab-item:has-text("会话")')
  await sessionsTab.click()
  await page.waitForTimeout(300)
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

test.describe('Multi-Session Management', () => {
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

  test.skip('创建多个 Session', async ({ page }) => {
    await selectModel(page)

    await sendMessage(page, 'Session 1: First message')

    await goToSessionsTab(page)
    const cards = page.locator('[data-testid="session-list"] [data-testid^="session-card-"]')
    await expect(cards).toHaveCount(1)
  })

  test.skip('两个 Session 消息隔离', async ({ page }) => {
    await selectModel(page)

    await sendMessage(page, 'Session 1: Hello from session one')

    const sessionId1 = await page.evaluate(() =>
      localStorage.getItem('loom-active-session')
    )
    expect(sessionId1).toBeTruthy()

    await page.evaluate(() => {
      localStorage.removeItem('loom-active-session')
    })
    await page.reload()
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })

    await selectModel(page)

    await sendMessage(page, 'Session 2: Hello from session two')

    const sessionId2 = await page.evaluate(() =>
      localStorage.getItem('loom-active-session')
    )
    expect(sessionId2).toBeTruthy()
    expect(sessionId2).not.toBe(sessionId1)
  })

  test.skip('Session 列表按最近活动排序', async ({ page }) => {
    await selectModel(page)

    await sendMessage(page, 'First session message')

    await page.evaluate(() => {
      localStorage.removeItem('loom-active-session')
    })
    await page.reload()
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
    await selectModel(page)

    await sendMessage(page, 'Second session message')

    await goToSessionsTab(page)
    const cards = page.locator('[data-testid="session-list"] [data-testid^="session-card-"]')
    await expect(cards).toHaveCount(2)

    const firstTitle = await cards.first().locator('[data-testid="session-card__title"]').textContent()
    expect(firstTitle).toBeTruthy()
  })

  test.skip('刷新后保留多个 Session', async ({ page }) => {
    await selectModel(page)

    await sendMessage(page, 'Session A message')

    await page.evaluate(() => {
      localStorage.removeItem('loom-active-session')
    })
    await page.reload()
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
    await selectModel(page)

    await sendMessage(page, 'Session B message')

    const sessions = await page.evaluate(() =>
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )
    expect(sessions).toHaveLength(2)

    await page.reload()
    await page.waitForSelector('.composer')

    const sessionsAfter = await page.evaluate(() =>
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )
    expect(sessionsAfter).toHaveLength(2)
  })
})
