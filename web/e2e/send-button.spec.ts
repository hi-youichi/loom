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
    await textarea.pressSequentially('Line 2')

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

    // streaming 中按钮可点击（用于取消）
    await expect(button).toBeEnabled()

    // 等待 AI 响应完成（等待 assistant 消息出现）
    await page.waitForSelector('.message--assistant', { timeout: 30000 })

    // streaming 结束，输入框已清空，按钮应禁用
    await expect(button).toBeDisabled()

    // 输入新内容后按钮恢复可用
    await textarea.fill('Next message')
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

  // 验证按钮变为停止状态（可点击取消）
  await expect(button).toBeEnabled()

  // 点击停止按钮
  await button.click()

  // 验证发送被取消，消息列表中没有用户消息
  await page.waitForTimeout(1000)
  const userMessages = await messageList.locator('.message--user').count()
  expect(userMessages).toBe(0)

  // streaming 结束，输入框已清空，按钮应禁用
  await expect(button).toBeDisabled()
})

test('取消发送时发送 cancel_run 协议并正确处理响应', async ({ page }) => {
  const button = page.locator('.composer__button')
  const textarea = page.locator('.composer__input')

  // 拦截 WebSocket 消息
  const wsMessages: Array<{ direction: 'send' | 'receive'; data: unknown }> = []
  await page.evaluate(() => {
    const originalSend = WebSocket.prototype.send
    WebSocket.prototype.send = function (this: WebSocket, data: string) {
      try {
        const parsed = JSON.parse(data)
        ;(window as any).__wsMessages = (window as any).__wsMessages || []
        ;(window as any).__wsMessages.push({ direction: 'send', data: parsed })
      } catch {}
      return originalSend.call(this, data)
    }

    const origAddEventListener = WebSocket.prototype.addEventListener
    WebSocket.prototype.addEventListener = function (this: WebSocket, type: string, listener: EventListenerOrEventListenerObject, ...rest: any[]) {
      if (type === 'message') {
        const wrappedListener = (event: Event) => {
          try {
            const parsed = JSON.parse((event as MessageEvent).data)
            ;(window as any).__wsMessages = (window as any).__wsMessages || []
            ;(window as any).__wsMessages.push({ direction: 'receive', data: parsed })
          } catch {}
          if (typeof listener === 'function') {
            listener(event)
          } else if (listener && typeof listener.handleEvent === 'function') {
            listener.handleEvent(event)
          }
        }
        return origAddEventListener.call(this, type, wrappedListener, ...rest)
      }
      return origAddEventListener.call(this, type as any, listener as any, ...rest as any[])
    }
  })

  // 输入并发送消息
  await textarea.fill('Test cancel protocol')
  await button.click()

  // 等待 streaming 开始
  await expect(button).toBeEnabled()

  // 点击停止按钮
  await button.click()

  // 等待 cancel 协议交互完成
  await page.waitForTimeout(2000)

  // 获取 WebSocket 消息记录
  const messages = await page.evaluate(() => (window as any).__wsMessages || []) as Array<{ direction: string; data: any }>

  // 验证发送了 cancel_run 请求
  const cancelRequest = messages.find(
    (m: any) => m.direction === 'send' && m.data?.type === 'cancel_run'
  )
  expect(cancelRequest).toBeDefined()
  expect(cancelRequest!.data).toHaveProperty('run_id')
  expect(cancelRequest!.data).toHaveProperty('id')

  // 验证收到 cancel_run 响应（不是 cancel_run_ack）
  const cancelResponse = messages.find(
    (m: any) => m.direction === 'receive' && m.data?.type === 'cancel_run'
  )
  expect(cancelResponse).toBeDefined()
  expect(cancelResponse!.data).toHaveProperty('run_id')
  expect(cancelResponse!.data).toHaveProperty('id')

  // 验证没有收到 cancel_run_ack（旧协议）
  const oldAckResponse = messages.find(
    (m: any) => m.direction === 'receive' && m.data?.type === 'cancel_run_ack'
  )
  expect(oldAckResponse).toBeUndefined()
})
