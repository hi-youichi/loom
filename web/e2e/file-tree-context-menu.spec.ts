import { test, expect } from '@playwright/test'
import fs from 'fs'
import path from 'path'
import os from 'os'

declare global {
  var __workspaceRootDir: string
}

async function setupWorkspaceWithFiles(page: import('@playwright/test').Page) {
  await page.goto('/')
  await page.waitForSelector('.composer')
  await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })

  await page.click('[data-testid="workspace-selector"]')
  await page.click('[data-testid="workspace-create-btn"]')
  await page.fill('[data-testid="workspace-create-input"]', 'ContextMenu WS')
  await page.locator('[data-testid="workspace-create-input"]').press('Enter')

  const selectedName = page.locator('[data-testid="selected-workspace-name"]')
  await expect(selectedName).toContainText('ContextMenu WS', { timeout: 5000 })

  await page.click('[data-testid="workspace-selector"]')
  const workspaceItems = page.locator('[data-testid^="workspace-item-"]')
  const testid = await workspaceItems.first().getAttribute('data-testid')
  const workspaceId = testid!.replace('workspace-item-', '')
  await page.click('body', { position: { x: 300, y: 300 } })

  const rootDir = global.__workspaceRootDir || path.join(os.tmpdir(), 'loom-test-workspace-root')
  const wsDir = path.join(rootDir, workspaceId)

  fs.mkdirSync(path.join(wsDir, 'src'), { recursive: true })
  fs.writeFileSync(path.join(wsDir, 'README.md'), '# Test')
  fs.writeFileSync(path.join(wsDir, 'package.json'), '{"name":"test"}')
  fs.writeFileSync(path.join(wsDir, 'src', 'main.ts'), 'console.log("hello")')
  fs.writeFileSync(path.join(wsDir, 'src', 'utils.ts'), 'export const add = (a: number, b: number) => a + b')

  await page.click('[data-testid="view-files"]')
  await page.waitForTimeout(500)

  return { workspaceId, wsDir }
}

test.describe('File Tree Context Menu', () => {
  test('should open context menu on right-click file', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const fileItem = page.locator('[data-file-type="file"]').first()
    await expect(fileItem).toBeVisible({ timeout: 5000 })
    await fileItem.click({ button: 'right' })

    const contextMenu = page.locator('[data-slot="context-menu-content"]').first()
    await expect(contextMenu).toBeVisible()

    await expect(contextMenu.locator('text=重命名')).toBeVisible()
    await expect(contextMenu.locator('text=复制路径')).toBeVisible()
    await expect(contextMenu.locator('text=删除')).toBeVisible()
    await expect(contextMenu.locator('text=刷新')).toBeVisible()

    await expect(contextMenu.locator('text=新建文件')).not.toBeVisible()
    await expect(contextMenu.locator('text=新建文件夹')).not.toBeVisible()
  })

  test('should show new file/folder options for folder', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const folderItem = page.locator('[data-file-type="folder"]').first()
    await expect(folderItem).toBeVisible({ timeout: 5000 })
    await folderItem.click({ button: 'right' })

    const contextMenu = page.locator('[data-slot="context-menu-content"]').first()
    await expect(contextMenu).toBeVisible()

    await expect(contextMenu.locator('text=新建文件')).toBeVisible()
    await expect(contextMenu.locator('text=新建文件夹')).toBeVisible()
    await expect(contextMenu.locator('text=重命名')).toBeVisible()
    await expect(contextMenu.locator('text=删除')).toBeVisible()
  })

  test('should start inline rename from context menu', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const fileItem = page.locator('[data-file-type="file"]').first()
    await expect(fileItem).toBeVisible({ timeout: 5000 })
    await fileItem.click({ button: 'right' })

    const renameItem = page.locator('[data-slot="context-menu-content"]').first().locator('text=重命名')
    await renameItem.click()

    const renameInput = page.locator('[data-testid="inline-rename-input"]')
    await expect(renameInput).toBeVisible()
    await expect(renameInput).toBeFocused()
  })

  test('should cancel rename on Escape', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const fileItem = page.locator('[data-file-type="file"]').first()
    await expect(fileItem).toBeVisible({ timeout: 5000 })
    await fileItem.click({ button: 'right' })

    await page.locator('[data-slot="context-menu-content"]').first().locator('text=重命名').click()

    const renameInput = page.locator('[data-testid="inline-rename-input"]')
    await expect(renameInput).toBeVisible()
    await renameInput.press('Escape')

    await expect(renameInput).not.toBeVisible()
  })

  test('should start rename with F2 keyboard shortcut', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const fileItem = page.locator('[data-file-type="file"]').first()
    await expect(fileItem).toBeVisible({ timeout: 5000 })
    await fileItem.click()
    await fileItem.press('F2')

    const renameInput = page.locator('[data-testid="inline-rename-input"]')
    await expect(renameInput).toBeVisible()
  })

  test('should show delete confirmation toast', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const fileItem = page.locator('[data-file-type="file"]').first()
    await expect(fileItem).toBeVisible({ timeout: 5000 })
    await fileItem.click({ button: 'right' })

    await page.locator('[data-slot="context-menu-content"]').first().locator('text=删除').click()

    const toast = page.locator('[data-testid^="toast-"]').first()
    await expect(toast).toBeVisible({ timeout: 3000 })
    await expect(toast).toContainText('确认删除')
  })

  test('should trigger delete with Delete key', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const fileItem = page.locator('[data-file-type="file"]').first()
    await expect(fileItem).toBeVisible({ timeout: 5000 })
    await fileItem.click()
    await fileItem.press('Delete')

    const toast = page.locator('[data-testid^="toast-"]').first()
    await expect(toast).toBeVisible({ timeout: 3000 })
    await expect(toast).toContainText('确认删除')
  })

  test('should start new file creation from folder context menu', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const folderItem = page.locator('[data-file-type="folder"]').first()
    await expect(folderItem).toBeVisible({ timeout: 5000 })
    await folderItem.click()
    await folderItem.click({ button: 'right' })

    await page.locator('[data-slot="context-menu-content"]').first().locator('text=新建文件').click()

    const createInput = page.locator('[data-testid="inline-create-input"]')
    await expect(createInput).toBeVisible()
    await expect(createInput).toBeFocused()
  })

  test('should start new folder creation from folder context menu', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const folderItem = page.locator('[data-file-type="folder"]').first()
    await expect(folderItem).toBeVisible({ timeout: 5000 })
    await folderItem.click()
    await folderItem.click({ button: 'right' })

    await page.locator('[data-slot="context-menu-content"]').first().locator('text=新建文件夹').click()

    const createInput = page.locator('[data-testid="inline-create-input"]')
    await expect(createInput).toBeVisible()
  })

  test('should cancel new file creation on Escape', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const folderItem = page.locator('[data-file-type="folder"]').first()
    await expect(folderItem).toBeVisible({ timeout: 5000 })
    await folderItem.click()
    await folderItem.click({ button: 'right' })

    await page.locator('[data-slot="context-menu-content"]').first().locator('text=新建文件').click()

    const createInput = page.locator('[data-testid="inline-create-input"]')
    await expect(createInput).toBeVisible()
    await createInput.press('Escape')

    await expect(createInput).not.toBeVisible()
  })

  test('should show toast on copy path', async ({ page, context }) => {
    const { wsDir } = await setupWorkspaceWithFiles(page)

    await context.grantPermissions(['clipboard-read', 'clipboard-write'])

    const fileItem = page.locator('[data-file-type="file"]').first()
    await expect(fileItem).toBeVisible({ timeout: 5000 })
    await fileItem.click({ button: 'right' })

    await page.locator('[data-slot="context-menu-content"]').first().locator('text=复制路径').click()

    const toast = page.locator('[data-testid^="toast-"]').first()
    await expect(toast).toBeVisible({ timeout: 3000 })
    await expect(toast).toContainText('已复制')
  })

  test('should close context menu on clicking outside', async ({ page }) => {
    await setupWorkspaceWithFiles(page)

    const fileItem = page.locator('[data-file-type="file"]').first()
    await expect(fileItem).toBeVisible({ timeout: 5000 })
    await fileItem.click({ button: 'right' })

    const contextMenu = page.locator('[data-slot="context-menu-content"]').first()
    await expect(contextMenu).toBeVisible()

    await page.click('body', { position: { x: 300, y: 300 } })
    await page.waitForTimeout(300)

    await expect(contextMenu).not.toBeVisible()
  })
})
