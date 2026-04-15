import { test, expect } from '@playwright/test'

test.describe('SendButton', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    // 等待页面完全加载
    await page.waitForSelector('.composer')
    await page.waitForSelector('.composer__button')
  })

  test('发送按钮可见', async ({ page }) => {
    const button = page.locator('.composer__button')
    await expect(button).toBeVisible()
  })

  test('空内容时按钮禁用，输入内容后启用', async ({ page }) => {
    const button = page.locator('.composer__button')
    const textarea = page.locator('.composer__input')

    // 初始状态：空内容，按钮应该禁用
    await expect(button).toBeDisabled()

    // 输入内容
    await textarea.fill('Hello, test message!')
    await page.waitForTimeout(100)

    // 有内容后，按钮应该启用
    await expect(button).not.toBeDisabled()

    // 清空内容
    await textarea.fill('')
    await page.waitForTimeout(100)

    // 空内容后，按钮应该再次禁用
    await expect(button).toBeDisabled()
  })

  test('点击发送按钮发送消息', async ({ page }) => {
    const button = page.locator('.composer__button')
    const textarea = page.locator('.composer__input')
    const messageList = page.locator('.message-list')

    // 获取当前消息数量
    const messageCountBefore = await messageList.locator('.message').count()

    // 输入消息
    const testMessage = 'Test message from Playwright'
    await textarea.fill(testMessage)

    // 等待按钮启用
    await expect(button).not.toBeDisabled()

    // 点击发送按钮
    await button.click()

    // 验证按钮在发送中禁用
    await expect(button).toBeDisabled()

    // 等待用户消息出现在列表中
    await page.waitForSelector('.message--user', { timeout: 10000 })

    // 验证消息数量增加
    const messageCountAfter = await messageList.locator('.message').count()
    expect(messageCountAfter).toBe(messageCountBefore + 1)

    // 验证消息内容
    const lastUserMessage = messageList.locator('.message--user').last()
    await expect(lastUserMessage).toContainText(testMessage)
  })

  test('按 Enter 键发送消息', async ({ page }) => {
    const button = page.locator('.composer__button')
    const textarea = page.locator('.composer__input')
    const messageList = page.locator('.message-list')

    // 获取当前消息数量
    const messageCountBefore = await messageList.locator('.message').count()

    // 输入消息
    const testMessage = 'Test message via Enter key'
    await textarea.fill(testMessage)

    // 按 Enter 键（不是 Shift+Enter）
    await textarea.press('Enter')

    // 验证消息发送
    await page.waitForSelector('.message--user', { timeout: 10000 })

    // 验证消息数量增加
    const messageCountAfter = await messageList.locator('.message').count()
    expect(messageCountAfter).toBe(messageCountBefore + 1)

    // 验证消息内容
    const lastUserMessage = messageList.locator('.message--user').last()
    await expect(lastUserMessage).toContainText(testMessage)
  })

  test('Shift+Enter 不发送消息，只换行', async ({ page }) => {
    const button = page.locator('.composer__button')
    const textarea = page.locator('.composer__input')

    // 输入第一行
    await textarea.fill('Line 1')
    await textarea.press('Shift+Enter')
    await textarea.fill('Line 2')

    // 获取 textarea 的值
    const value = await textarea.inputValue()
    expect(value).toContain('Line 1')
    expect(value).toContain('Line 2')

    // 消息不应该发送，按钮应该仍然可用
    await expect(button).not.toBeDisabled()
  })

  test('发送完成后按钮恢复可用', async ({ page }) => {
    const button = page.locator('.composer__button')
    const textarea = page.locator('.composer__input')

    // 输入并发送消息
    await textarea.fill('Test message for button state')
    await button.click()

    // 等待发送中状态
    await expect(button).toBeDisabled()

    // 等待 AI 响应完成（等待 assistant 消息出现）
    await page.waitForSelector('.message--assistant', { timeout: 15000 })

    // 等待发送状态恢复
    await page.waitForTimeout(500)

    // 验证按钮恢复可用
    await expect(button).not.toBeDisabled()
  })
})

test('点击停止按钮可以取消发送', async ({ page }) => {
  const button = page.locator('.composer__button')
  const textarea = page.locator('.composer__input')
  const messageList = page.locator('.message-list')

  // 输入消息
  const testMessage = 'Test cancel message'
  await textarea.fill(testMessage)

  // 点击发送按钮
  await button.click()

  // 验证按钮变为停止状态（禁用）
  await expect(button).toBeDisabled()

  // 点击停止按钮
  await button.click()

  // 验证发送被取消，消息列表中没有用户消息
  await page.waitForTimeout(1000)
  const userMessages = await messageList.locator('.message--user').count()
  expect(userMessages).toBe(0)

  // 验证按钮恢复为发送状态（启用）
  await expect(button).not.toBeDisabled()
})
