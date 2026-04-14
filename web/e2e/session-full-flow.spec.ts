/**
 * Session Full Flow E2E Tests
 * 
 * Tests complete session lifecycle including creation, message sending,
 * and metadata updates with backend integration.
 */

import { test, expect } from '@playwright/test'

test.describe('Session Full Flow', () => {
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

  test('创建 Session 并发送消息 - 完整流程', async ({ page }) => {
    // 1. Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    const firstModel = page.locator('.model-selector__content button').first()
    const modelName = await firstModel.textContent()
    await firstModel.click()
    
    // 2. Send a message
    const testMessage = 'Hello, this is a test message for session creation'
    await page.fill('.composer__input', testMessage)
    await page.click('.composer__button')
    
    // 3. Wait for user message to appear
    await page.waitForSelector('.message--user')
    await expect(page.locator('.message--user')).toContainText(testMessage)
    
    // 4. Wait for AI response (from Mock LLM)
    await page.waitForSelector('.message--assistant', { timeout: 30000 })
    
    // 5. Verify Session metadata is created
    await page.waitForSelector('[data-testid="session-card"]', { timeout: 10000 })
    const sessionCard = page.locator('[data-testid="session-card"]').first()
    
    // Verify title (should be first 50 chars of message)
    const title = await sessionCard.locator('[data-testid="session-card__title"]').textContent()
    expect(title).toBeTruthy()
    expect(title!.length).toBeLessThanOrEqual(50)
    expect(title).toContain(testMessage.substring(0, 47))
    
    // Verify last message (should be first 200 chars of user message)
    const lastMessage = await sessionCard.locator('[data-testid="session-card__last-message"]').textContent()
    expect(lastMessage).toBeTruthy()
    expect(lastMessage).toContain(testMessage.substring(0, 197))
    
    // Verify message count
    const messageCount = await sessionCard.locator('[data-testid="session-card__count"]').textContent()
    expect(messageCount).toBe('1') // Only user message at this point
    
    // 6. Verify localStorage Session data
    const sessions = await page.evaluate(() => 
      JSON.parse(localStorage.getItem('loom-sessions') || '[]')
    )
    expect(sessions).toHaveLength(1)
    expect(sessions[0].title).toBe(title)
    expect(sessions[0].messageCount).toBe(1)
    expect(sessions[0].status).toBe('active')
    expect(sessions[0].lastMessage).toContain(testMessage.substring(0, 197))
    
    // 7. Verify active session ID is set
    const activeSessionId = await page.evaluate(() => 
      localStorage.getItem('loom-active-session')
    )
    expect(activeSessionId).toBeTruthy()
    expect(activeSessionId).toBe(sessions[0].id)
  })

  test('长消息标题截断', async ({ page }) => {
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create a very long message (> 50 chars)
    const longMessage = 'This is a very long message that exceeds the fifty character limit for session title generation and should be truncated properly with an ellipsis at the end to indicate that it was cut off'
    await page.fill('.composer__input', longMessage)
    await page.click('.composer__button')
    
    await page.waitForSelector('.message--user')
    await page.waitForSelector('[data-testid="session-card"]')
    
    // Verify title is truncated
    const sessionCard = page.locator('[data-testid="session-card"]').first()
    const title = await sessionCard.locator('[data-testid="session-card__title"]').textContent()
    expect(title).toBeTruthy()
    expect(title!.length).toBeLessThanOrEqual(50)
    expect(title).toContain('...')
  })

  test('最后消息截断（200字符）', async ({ page }) => {
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Create a very long message (> 200 chars)
    const longMessage = 'A'.repeat(300)
    await page.fill('.composer__input', longMessage)
    await page.click('.composer__button')
    
    await page.waitForSelector('.message--user')
    await page.waitForSelector('[data-testid="session-card"]')
    
    // Verify last message is truncated
    const sessionCard = page.locator('[data-testid="session-card"]').first()
    const lastMessage = await sessionCard.locator('[data-testid="session-card__last-message"]').textContent()
    expect(lastMessage).toBeTruthy()
    expect(lastMessage!.length).toBeLessThanOrEqual(200)
  })

  test('发送多条消息更新消息计数', async ({ page }) => {
    // Select a model
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
    
    // Send 3 messages
    for (let i = 1; i <= 3; i++) {
      await page.fill('.composer__input', `Message ${i}`)
      await page.click('.composer__button')
      await page.waitForSelector(`.message--user >> text=Message ${i}`)
      await page.waitForSelector('.message--assistant', { timeout: 15000 })
    }
    
    // Verify message count is 3
    const sessionCard = page.locator('[data-testid="session-card"]').first()
    const messageCount = await sessionCard.locator('[data-testid="session-card__count"]').textContent()
    expect(messageCount).toBe('3')
  })
})
