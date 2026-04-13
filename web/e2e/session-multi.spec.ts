/**
 * Multi-Session Management E2E Tests
 * 
 * Tests creating and managing multiple sessions.
 */

import { test, expect } from '@playwright/test'

test.describe('Multi-Session Management', () => {
  test.beforeEach(async ({ page }) => {
    // Clean localStorage before each test
    await page.goto('/')
    await page.evaluate(() => {
      localStorage.removeItem('loom-sessions')
      localStorage.removeItem('loom-active-session')
    })
    await page.reload()
    
    // Wait for page to load
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
  })

  test('创建多个 Session', async ({ page }) => {
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create first session
    await page.fill('.composer__input', 'Session 1: First message')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user >> text=Session 1')
    await page.waitForSelector('[data-testid="session-card"]')
    
    // Clear active session to create new one
    await page.evaluate(() => {
      localStorage.removeItem('loom-active-session')
    })
    await page.reload()
    await page.waitForSelector('.composer')
    
    // Select model again
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create second session
    await page.fill('.composer__input', 'Session 2: Second message')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user >> text=Session 2')
    
    // Verify two sessions exist
    await page.waitForSelector('[data-testid="session-card"]')
    const sessions = await page.locator('[data-testid="session-card"]')
    const count = await sessions.count()
    expect(count).toBe(2)
    
    // Verify session titles
    const firstTitle = await sessions.nth(0).locator('[data-testid="session-card__title"]').textContent()
    const secondTitle = await sessions.nth(1).locator('[data-testid="session-card__title"]').textContent()
    
    expect(firstTitle).toContain('Session 1')
    expect(secondTitle).toContain('Session 2')
  })

  test('切换 Session', async ({ page }) => {
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create first session
    await page.fill('.composer__input', 'Session 1: First message')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user >> text=Session 1')
    
    const firstSessionId = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    // Clear and create second session
    await page.evaluate(() => {
      localStorage.removeItem('loom-active-session')
    })
    await page.reload()
    await page.waitForSelector('.composer')
    
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    await page.fill('.composer__input', 'Session 2: Second message')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user >> text=Session 2')
    
    // Get second session ID
    const secondSessionId = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    expect(secondSessionId).not.toBe(firstSessionId)
    
    // Click on first session to switch back
    await page.locator(`[data-testid="session-card-${firstSessionId}"]`).click()
    
    // Verify active session is now the first one
    const currentActiveId = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    expect(currentActiveId).toBe(firstSessionId)
    
    // Verify first session card is selected
    await expect(page.locator(`[data-testid="session-card-${firstSessionId}"]`))
      .toHaveClass(/session-card--selected/)
  })

  test('不同 Session 的消息隔离', async ({ page }) => {
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create first session with message
    await page.fill('.composer__input', 'Session 1 unique message')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user >> text=Session 1')
    
    const firstSessionId = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    // Clear and create second session
    await page.evaluate(() => {
      localStorage.removeItem('loom-active-session')
    })
    await page.reload()
    await page.waitForSelector('.composer')
    
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    await page.fill('.composer__input', 'Session 2 unique message')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user >> text=Session 2')
    
    // Click back to first session
    await page.locator(`[data-testid="session-card-${firstSessionId}"]`).click()
    
    // Verify only Session 1's message is visible
    const messages = await page.locator('.message--user')
    const session1Messages = await messages.filter({ hasText: 'Session 1' }).count()
    const session2Messages = await messages.filter({ hasText: 'Session 2' }).count()
    
    expect(session1Messages).toBeGreaterThan(0)
    expect(session2Messages).toBe(0)
  })

  test('Session 元数据独立', async ({ page }) => {
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create first session
    await page.fill('.composer__input', 'First session')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user')
    await page.waitForSelector('[data-testid="session-card"]')
    
    // Get first session metadata
    const firstCard = page.locator('[data-testid="session-card"]').first()
    const firstTitle = await firstCard.locator('[data-testid="session-card__title"]').textContent()
    const firstLastMessage = await firstCard.locator('[data-testid="session-card__last-message"]').textContent()
    const firstCount = await firstCard.locator('[data-testid="session-card__count"]').textContent()
    
    // Clear and create second session
    await page.evaluate(() => {
      localStorage.removeItem('loom-active-session')
    })
    await page.reload()
    await page.waitForSelector('.composer')
    
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    await page.fill('.composer__input', 'Second session')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user')
    
    // Verify both sessions have independent metadata
    const allCards = page.locator('[data-testid="session-card"]')
    const cardCount = await allCards.count()
    expect(cardCount).toBe(2)
    
    // Find the first session card
    const firstSessionCard = page.locator(`[data-testid="session-card"]:has-text("${firstTitle}")`)
    await expect(firstSessionCard).toBeVisible()
    
    const currentFirstTitle = await firstSessionCard.locator('[data-testid="session-card__title"]').textContent()
    const currentFirstLastMessage = await firstSessionCard.locator('[data-testid="session-card__last-message"]').textContent()
    const currentFirstCount = await firstSessionCard.locator('[data-testid="session-card__count"]').textContent()
    
    expect(currentFirstTitle).toBe(firstTitle)
    expect(currentFirstLastMessage).toBe(firstLastMessage)
    expect(currentFirstCount).toBe(firstCount)
  })
})
