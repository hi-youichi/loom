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

test.describe('Session Persistence', () => {
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

  test('页面刷新后 Session 持久化', async ({ page }) => {
    await selectModel(page)

    const testMessage = 'Test message for persistence'
    await sendMessage(page, testMessage)

    const sessionsBefore = await page.evaluate(() =>
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )
    const activeIdBefore = await page.evaluate(() =>
      localStorage.getItem('loom-active-session')
    )

    expect(sessionsBefore.length).toBeGreaterThanOrEqual(1)
    expect(activeIdBefore).toBeTruthy()

    await page.reload()
    await page.waitForSelector('.composer')

    const sessionsAfter = await page.evaluate(() =>
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )

    expect(sessionsAfter.length).toBeGreaterThanOrEqual(1)
    expect(sessionsAfter[0].title).toBeTruthy()
  })

  test('刷新后 Session 列表保留', async ({ page }) => {
    await selectModel(page)

    await sendMessage(page, 'Persistence test message')

    await goToSessionsTab(page)
    const cards = page.locator('[data-testid="session-list"] [data-testid^="session-card-"]')
    await expect(cards).toHaveCount(1)

    await page.reload()
    await page.waitForSelector('.composer')

    await goToSessionsTab(page)
    await expect(cards).toHaveCount(1)

    const title = await cards.first().locator('[data-testid="session-card__title"]').textContent()
    expect(title).toBeTruthy()
  })

  test('多个 Session 跨刷新持久化', async ({ page }) => {
    await selectModel(page)

    await sendMessage(page, 'First session message')

    const firstSessionId = await page.evaluate(() =>
      localStorage.getItem('loom-active-session')
    )

    await page.evaluate(() => {
      localStorage.removeItem('loom-active-session')
    })
    await page.reload()
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
    await selectModel(page)

    await sendMessage(page, 'Second session message')

    const sessions = await page.evaluate(() =>
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )
    expect(sessions.length).toBeGreaterThanOrEqual(2)

    await page.reload()
    await page.waitForSelector('.composer')

    const sessionsAfter = await page.evaluate(() =>
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )
    expect(sessionsAfter.length).toBeGreaterThanOrEqual(2)
    expect(sessionsAfter.map((s: any) => s.id)).toContain(firstSessionId)
  })
})
