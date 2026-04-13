/**
 * Session Persistence E2E Tests
 * 
 * Tests session persistence across page reloads and browser restarts.
 */

import { test, expect } from '@playwright/test'

test.describe('Session Persistence', () => {
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

  test('页面刷新后 Session 持久化', async ({ page }) => {
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create a session by sending a message
    const testMessage = 'Test message for persistence'
    await page.fill('.composer__input', testMessage)
    await page.click('.composer__button')
    
    await page.waitForSelector('.message--user')
    await page.waitForSelector('[data-testid="session-card"]')
    
    // Get session data before reload
    const sessionsBefore = await page.evaluate(() => 
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )
    const activeIdBefore = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    expect(sessionsBefore).toHaveLength(1)
    expect(activeIdBefore).toBeTruthy()
    
    // Reload the page
    await page.reload()
    
    // Wait for page to load
    await page.waitForSelector('.composer')
    
    // Verify session data persists
    const sessionsAfter = await page.evaluate(() => 
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )
    const activeIdAfter = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    expect(sessionsAfter).toHaveLength(1)
    expect(activeIdAfter).toBe(activeIdBefore)
    expect(sessionsAfter[0].id).toBe(sessionsBefore[0].id)
    
    // Verify session card is still visible
    await page.waitForSelector('[data-testid="session-card"]')
    const sessionCard = page.locator('[data-testid="session-card"]').first()
    const title = await sessionCard.locator('[data-testid="session-card__title"]').textContent()
    expect(title).toBe(sessionsAfter[0].title)
  })

  test('关闭浏览器后 Session 持久化', async ({ context }) => {
    // Create a new page
    const page = await context.newPage()
    await page.goto('/')
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
    
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create a session
    const testMessage = 'Persistent message across browser restart'
    await page.fill('.composer__input', testMessage)
    await page.click('.composer__button')
    
    await page.waitForSelector('.message--user')
    await page.waitForSelector('[data-testid="session-card"]')
    
    // Get session data
    const sessionId = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    // Close the page
    await page.close()
    
    // Create a new page (simulating browser restart)
    const newPage = await context.newPage()
    await newPage.goto('/')
    await newPage.waitForSelector('.composer')
    
    // Verify session is restored
    const restoredSessionId = await newPage.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    expect(restoredSessionId).toBe(sessionId)
    
    // Verify session card is visible
    await newPage.waitForSelector('[data-testid="session-card"]')
    const sessions = await newPage.locator('[data-testid="session-card"]')
    const count = await sessions.count()
    expect(count).toBe(1)
  })

  test('活动会话恢复', async ({ page }) => {
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create first session
    await page.fill('.composer__input', 'First session message')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user')
    await page.waitForSelector('[data-testid="session-card"]')
    
    const firstSessionId = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    // Verify first session is active
    const firstSessionCard = page.locator(`[data-testid="session-card-${firstSessionId}"]`)
    await expect(firstSessionCard).toHaveClass(/session-card--selected/)
    
    // Reload page
    await page.reload()
    await page.waitForSelector('.composer')
    
    // Verify active session is restored
    const restoredSessionId = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    expect(restoredSessionId).toBe(firstSessionId)
    
    // Verify session card is still selected
    await page.waitForSelector('[data-testid="session-card"]')
    const restoredSessionCard = page.locator(`[data-testid="session-card-${restoredSessionId}"]`)
    await expect(restoredSessionCard).toHaveClass(/session-card--selected/)
  })

  test('多个 Session 持久化', async ({ page }) => {
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create first session
    await page.fill('.composer__input', 'Session 1')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user')
    await page.waitForSelector('[data-testid="session-card"]')
    
    // Get first session ID
    const firstSessionId = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    
    // Create second session (simulate by clearing and creating new)
    await page.evaluate(() => {
      localStorage.removeItem('loom-active-session')
    })
    
    // Refresh to start new session
    await page.reload()
    await page.waitForSelector('.composer')
    
    // Select model again
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create second session
    await page.fill('.composer__input', 'Session 2')
    await page.click('.composer__button')
    await page.waitForSelector('.message--user')
    
    // Get all sessions
    const sessions = await page.evaluate(() => 
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )
    
    expect(sessions).toHaveLength(2)
    
    // Reload to verify persistence
    await page.reload()
    await page.waitForSelector('.composer')
    
    // Verify both sessions persist
    const sessionsAfter = await page.evaluate(() => 
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )
    
    expect(sessionsAfter).toHaveLength(2)
    expect(sessionsAfter.map((s: any) => s.id)).toContain(firstSessionId)
  })
})
