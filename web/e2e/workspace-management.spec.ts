import { test, expect } from '@playwright/test'

test.describe('Workspace Management', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
  })

  test('should display workspace selector', async ({ page }) => {
    const workspaceSelector = page.locator('[data-testid="workspace-selector"]')
    await expect(workspaceSelector).toBeVisible()
  })

  test('should create new workspace', async ({ page }) => {
    // Open workspace dropdown
    await page.click('[data-testid="workspace-selector"]')
    
    // Click create workspace button
    await page.click('[data-testid="create-workspace-button"]')
    
    // Enter workspace name
    const workspaceName = `Test Workspace ${Date.now()}`
    await page.fill('[data-testid="workspace-name-input"]', workspaceName)
    
    // Submit creation
    await page.click('[data-testid="submit-workspace-button"]')
    
    // Verify new workspace is selected
    const selectedWorkspace = page.locator('[data-testid="selected-workspace-name"]')
    await expect(selectedWorkspace).toHaveText(workspaceName)
  })

  test('should switch between workspaces', async ({ page }) => {
    // Create first workspace
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="create-workspace-button"]')
    const workspace1 = `Workspace 1 ${Date.now()}`
    await page.fill('[data-testid="workspace-name-input"]', workspace1)
    await page.click('[data-testid="submit-workspace-button"]')
    await expect(page.locator('[data-testid="selected-workspace-name"]')).toHaveText(workspace1)

    // Create second workspace
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="create-workspace-button"]')
    const workspace2 = `Workspace 2 ${Date.now()}`
    await page.fill('[data-testid="workspace-name-input"]', workspace2)
    await page.click('[data-testid="submit-workspace-button"]')
    await expect(page.locator('[data-testid="selected-workspace-name"]')).toHaveText(workspace2)

    // Switch back to first workspace
    await page.click('[data-testid="workspace-selector"]')
    await page.click(`[data-testid="workspace-item-${workspace1}"]`)
    await expect(page.locator('[data-testid="selected-workspace-name"]')).toHaveText(workspace1)
  })

  test('should isolate threads between workspaces', async ({ page }) => {
    // Create workspace A
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="create-workspace-button"]')
    const workspaceA = `Workspace A ${Date.now()}`
    await page.fill('[data-testid="workspace-name-input"]', workspaceA)
    await page.click('[data-testid="submit-workspace-button"]')

    // Send a message in workspace A
    await page.fill('.composer textarea', 'Hello from Workspace A')
    await page.click('.composer button[type="submit"]')
    await page.waitForSelector('.message-item', { timeout: 10000 })
    const messageA = page.locator('.message-item').first()
    await expect(messageA).toContainText('Hello from Workspace A')

    // Create workspace B
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="create-workspace-button"]')
    const workspaceB = `Workspace B ${Date.now()}`
    await page.fill('[data-testid="workspace-name-input"]', workspaceB)
    await page.click('[data-testid="submit-workspace-button"]')

    // Verify no messages in workspace B
    const messagesB = page.locator('.message-item')
    await expect(messagesB).toHaveCount(0)

    // Switch back to workspace A
    await page.click('[data-testid="workspace-selector"]')
    await page.click(`[data-testid="workspace-item-${workspaceA}"]`)
    
    // Verify message from A is still there
    const messageAAgain = page.locator('.message-item').first()
    await expect(messageAAgain).toContainText('Hello from Workspace A')
  })

  test('should delete workspace', async ({ page }) => {
    // Create test workspace
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="create-workspace-button"]')
    const workspaceToDelete = `Delete Me ${Date.now()}`
    await page.fill('[data-testid="workspace-name-input"]', workspaceToDelete)
    await page.click('[data-testid="submit-workspace-button"]')
    
    // Open dropdown again
    await page.click('[data-testid="workspace-selector"]')
    
    // Hover over workspace item to show delete button
    const workspaceItem = page.locator(`[data-testid="workspace-item-${workspaceToDelete}"]`)
    await workspaceItem.hover()
    
    // Click delete button
    await page.click(`[data-testid="delete-workspace-${workspaceToDelete}"]`)
    
    // Confirm deletion
    await page.click('[data-testid="confirm-delete-button"]')
    
    // Verify workspace is removed
    await page.click('[data-testid="workspace-selector"]')
    const deletedWorkspace = page.locator(`[data-testid="workspace-item-${workspaceToDelete}"]`)
    await expect(deletedWorkspace).toHaveCount(0)
  })
})
