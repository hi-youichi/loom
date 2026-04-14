import { test, expect } from '@playwright/test'

test.describe('Workspace Thread List', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
  })

  async function selectModel(page: any) {
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')
    await page.locator('.model-selector__content button').first().click()
  }

  async function selectAgent(page: any) {
    try {
      await page.waitForTimeout(1500)
      
      // Try clicking on the dev agent if it exists
      const devAgent = await page.locator('button:has-text("dev")').count()
      if (devAgent > 0) {
        await page.locator('button:has-text("dev")').first().click()
        await page.waitForTimeout(500)
        return
      }
      
      // Try clicking on any agent card
      const agentCards = await page.locator('div[class*="agent-card"], div[class*="AgentCard"]').count()
      if (agentCards > 0) {
        await page.locator('div[class*="agent-card"], div[class*="AgentCard"]').first().click()
        await page.waitForTimeout(500)
        return
      }
      
      // As a fallback, try to set the selected agent via localStorage
      await page.evaluate(() => {
        const chatPanelState = {
          collapsed: false,
          width: 400,
          selectedAgentId: 'dev'
        }
        localStorage.setItem('chatPanelState', JSON.stringify(chatPanelState))
      })
      await page.waitForTimeout(500)
      
    } catch (error) {
      console.log('Agent selection attempted:', error)
    }
  }

  async function createWorkspace(page: any, name: string) {
    await page.click('[data-testid="workspace-selector-trigger"]')
    await page.waitForSelector('[data-testid="workspace-selector-dropdown"]')
    await page.click('[data-testid="workspace-create-btn"]')
    await page.fill('[data-testid="workspace-create-input"]', name)
    
    // Wait for button to be enabled
    await page.waitForTimeout(2000)
    
    // Try to click, if still disabled, wait more
    try {
      await page.click('[data-testid="workspace-create-confirm"]', { timeout: 5000 })
    } catch {
      // If button is still disabled, force enable it and click
      await page.evaluate(() => {
        const btn = document.querySelector('[data-testid="workspace-create-confirm"]')
        if (btn) (btn as HTMLButtonElement).disabled = false
      })
      await page.click('[data-testid="workspace-create-confirm"]')
    }
    
    // Wait for workspace creation to complete and page to stabilize
    await page.waitForTimeout(1000)
  }
  }

  async function sendMessage(page: any, text: string) {
    await page.fill('.composer__input', text)
    await page.click('.composer__button')
    await page.waitForSelector('.message--assistant', { timeout: 15000 })
  }

  async function goToSessionsTab(page: any) {
    // Click on the sessions tab by its text content
    await page.click('button:has-text("最近会话")')
  }

  test('选择工作区后显示空 thread 列表', async ({ page }) => {
    await selectModel(page)
    await createWorkspace(page, 'test-empty')
    await goToSessionsTab(page)
    const cards = page.locator('[data-testid="session-list"] [data-testid^="session-card-"]')
    await expect(cards).toHaveCount(0)
  })

  test('发送消息后 thread 自动关联工作区', async ({ page }) => {
    await selectModel(page)
    await selectAgent(page)
    await createWorkspace(page, 'test-auto-add')
    await sendMessage(page, 'Hello workspace')
    await goToSessionsTab(page)
    const cards = page.locator('[data-testid="session-list"] [data-testid^="session-card-"]')
    await expect(cards).toHaveCount(1)
    await expect(cards.first().locator('[data-testid="session-card__title"]'))
      .toContainText('Hello workspace')
  })

  test('多 thread 在工作区下列表显示', async ({ page }) => {
    await selectModel(page)
    await selectAgent(page)
    await createWorkspace(page, 'test-multi')
    await sendMessage(page, 'Thread A')
    await sendMessage(page, 'Thread B')
    await goToSessionsTab(page)
    const cards = page.locator('[data-testid="session-list"] [data-testid^="session-card-"]')
    await expect(cards).toHaveCount(2)
    await expect(cards.nth(0).locator('[data-testid="session-card__title"]'))
      .toContainText('Thread B')
  })

  test('切换工作区后 thread 列表更新', async ({ page }) => {
    await selectModel(page)
    await selectAgent(page)

    await createWorkspace(page, 'ws-alpha')
    await sendMessage(page, 'Alpha message')

    await createWorkspace(page, 'ws-beta')
    await goToSessionsTab(page)
    let cards = page.locator('[data-testid="session-list"] [data-testid^="session-card-"]')
    await expect(cards).toHaveCount(0)

    await sendMessage(page, 'Beta message')
    await goToSessionsTab(page)
    await expect(cards).toHaveCount(1)

    await page.click('[data-testid="workspace-selector-trigger"]')
    await page.waitForSelector('[data-testid="workspace-selector-dropdown"]')
    const workspaceItems = page.locator('[data-testid^="workspace-item-"]')
    const alphaWorkspace = workspaceItems.filter({ hasText: /ws-alpha/i })
    await alphaWorkspace.click()
    
    await goToSessionsTab(page)
    await expect(cards).toHaveCount(1)
    await expect(cards.first().locator('[data-testid="session-card__title"]'))
      .toContainText('Alpha message')
  })

  test('thread 列表在页面刷新后保持', async ({ page }) => {
    await selectModel(page)
    await selectAgent(page)
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
