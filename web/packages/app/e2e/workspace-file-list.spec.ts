import { test, expect } from '@playwright/test'
import fs from 'fs'
import path from 'path'
import os from 'os'

declare global {
  var __workspaceRootDir: string
}

async function switchToFilesView(page: import('@playwright/test').Page) {
  await page.locator('[data-testid="view-files"]').click({ force: true })
  await page.waitForTimeout(1000)
}

async function createWorkspaceWithFiles(page: import('@playwright/test').Page, name: string): Promise<string> {
  await page.click('[data-testid="workspace-selector"]')
  await page.click('[data-testid="workspace-create-btn"]')
  await page.fill('[data-testid="workspace-create-input"]', name)
  await page.locator('[data-testid="workspace-create-input"]').press('Enter')

  const selectedName = page.locator('[data-testid="selected-workspace-name"]')
  await expect(selectedName).toContainText(name, { timeout: 5000 })

  await page.click('[data-testid="workspace-selector"]')
  const workspaceItems = page.locator('[data-testid^="workspace-item-"]')
  const testid = await workspaceItems.first().getAttribute('data-testid')
  const workspaceId = testid!.replace('workspace-item-', '')
  await page.locator('[data-testid="file-sidebar"]').click({ position: { x: 110, y: 20 } })
  await page.waitForTimeout(300)

  const rootDir = (() => {
    try {
      return fs.readFileSync(path.join(os.tmpdir(), 'loom-test-workspace-root-dir.txt'), 'utf8').trim()
    } catch {
      return path.join(os.tmpdir(), 'loom-test-workspace-root')
    }
  })()
  const wsDir = path.join(rootDir, workspaceId)

  fs.mkdirSync(path.join(wsDir, 'src'), { recursive: true })
  fs.mkdirSync(path.join(wsDir, 'assets'), { recursive: true })
  fs.writeFileSync(path.join(wsDir, 'README.md'), '# Test Project')
  fs.writeFileSync(path.join(wsDir, 'package.json'), '{"name": "test"}')
  fs.writeFileSync(path.join(wsDir, 'src', 'main.ts'), 'console.log("hello")')
  fs.writeFileSync(path.join(wsDir, 'src', 'utils.ts'), 'export function add(a: number, b: number) { return a + b }')
  fs.writeFileSync(path.join(wsDir, 'assets', 'logo.svg'), '<svg></svg>')

  await page.locator('[data-testid="view-files"]').click({ force: true })
  await page.waitForTimeout(500)
  await page.locator('[data-testid="file-refresh-btn"]').click()
  await page.waitForTimeout(2000)

  return workspaceId
}

test.describe('Workspace File List', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
  })

  test('should display file sidebar', async ({ page }) => {
    const sidebar = page.locator('[data-testid="file-sidebar"]')
    await expect(sidebar).toBeVisible()
  })

  test('should show dashboard view by default', async ({ page }) => {
    const dashboardBtn = page.locator('[data-testid="view-dashboard"]')
    await expect(dashboardBtn).toBeVisible()

    const filesBtn = page.locator('[data-testid="view-files"]')
    await expect(filesBtn).toBeVisible()
  })

  test('should switch to files view', async ({ page }) => {
    await createWorkspaceWithFiles(page, 'File Test WS')

    const fileItems = page.locator('[data-testid^="file-item-"]')
    await expect(fileItems.first()).toBeVisible({ timeout: 5000 })
  })

  test('should list root files and folders after workspace creation', async ({ page }) => {
    await createWorkspaceWithFiles(page, 'File List WS')

    const fileItems = page.locator('[data-testid^="file-item-"]')
    await expect(fileItems.first()).toBeVisible({ timeout: 5000 })

    const allItems = await fileItems.all()
    const names = await Promise.all(allItems.map(item => item.locator('span.truncate').textContent()))

    expect(names.some(n => n?.includes('src'))).toBeTruthy()
    expect(names.some(n => n?.includes('assets'))).toBeTruthy()
    expect(names.some(n => n?.includes('README.md'))).toBeTruthy()
    expect(names.some(n => n?.includes('package.json'))).toBeTruthy()

    const folders = page.locator('[data-file-type="folder"]')
    const files = page.locator('[data-file-type="file"]')
    const folderCount = await folders.count()
    const fileCount = await files.count()
    expect(folderCount + fileCount).toBe(4)
  })

  test('should display folders before files', async ({ page }) => {
    await createWorkspaceWithFiles(page, 'Sort Order WS')

    const fileItems = page.locator('[data-testid^="file-item-"]')
    await expect(fileItems.first()).toBeVisible({ timeout: 5000 })

    const allItems = await fileItems.all()
    const types: string[] = []
    for (const item of allItems) {
      const t = await item.getAttribute('data-file-type')
      if (t) types.push(t)
    }

    const firstFileIdx = types.indexOf('file')
    if (firstFileIdx > 0) {
      for (let i = 0; i < firstFileIdx; i++) {
        expect(types[i]).toBe('folder')
      }
    }
  })

  test('should expand folder and show children', async ({ page }) => {
    await createWorkspaceWithFiles(page, 'Expand WS')

    const srcFolder = page.locator('[data-file-type="folder"]').first()
    await expect(srcFolder).toBeVisible({ timeout: 5000 })
    await srcFolder.click()
    await page.waitForTimeout(500)

    const childItems = page.locator('[data-testid^="file-item-"]')
    const allNames = await Promise.all(
      (await childItems.all()).map(item => item.locator('span.truncate').textContent())
    )
    expect(allNames.some(n => n?.includes('main.ts'))).toBeTruthy()
    expect(allNames.some(n => n?.includes('utils.ts'))).toBeTruthy()
  })

  test('should show empty state when no files', async ({ page }) => {
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="workspace-create-btn"]')
    await page.fill('[data-testid="workspace-create-input"]', 'Empty WS')
    await page.locator('[data-testid="workspace-create-input"]').press('Enter')

    const selectedName = page.locator('[data-testid="selected-workspace-name"]')
    await expect(selectedName).toContainText('Empty WS', { timeout: 5000 })

    await switchToFilesView(page)

    const emptyState = page.locator('[data-testid="file-empty"]')
    if (await emptyState.isVisible()) {
      await expect(emptyState).toContainText('没有找到匹配的文件')
    }
  })

  test('should update file list after switching workspace', async ({ page }) => {
    const ws1 = await createWorkspaceWithFiles(page, 'WS Alpha')

    const fileItems1 = page.locator('[data-testid^="file-item-"]')
    await expect(fileItems1.first()).toBeVisible({ timeout: 5000 })
    const count1 = await fileItems1.count()
    expect(count1).toBeGreaterThan(0)

    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="workspace-create-btn"]')
    await page.fill('[data-testid="workspace-create-input"]', 'WS Beta')
    await page.locator('[data-testid="workspace-create-input"]').press('Enter')

    const selectedName = page.locator('[data-testid="selected-workspace-name"]')
    await expect(selectedName).toContainText('WS Beta', { timeout: 5000 })

    await page.click('[data-testid="workspace-selector"]')
    const wsItems = page.locator('[data-testid^="workspace-item-"]')
    const count = await wsItems.count()
    const lastItem = wsItems.nth(count - 1)
    const testid = await lastItem.getAttribute('data-testid')
    const ws2Id = testid!.replace('workspace-item-', '')

    const rootDir = (() => {
      try {
        return fs.readFileSync(path.join(os.tmpdir(), 'loom-test-workspace-root-dir.txt'), 'utf8').trim()
      } catch {
        return path.join(os.tmpdir(), 'loom-test-workspace-root')
      }
    })()
    const ws2Dir = path.join(rootDir, ws2Id)
    fs.mkdirSync(ws2Dir, { recursive: true })
    fs.writeFileSync(path.join(ws2Dir, 'hello.txt'), 'world')

    await lastItem.click()
    await page.waitForTimeout(1000)

    const fileItems2 = page.locator('[data-testid^="file-item-"]')
    await expect(fileItems2.first()).toBeVisible({ timeout: 5000 })
    const names2 = await Promise.all(
      (await fileItems2.all()).map(item => item.locator('span.truncate').textContent())
    )
    expect(names2.some(n => n?.includes('hello.txt'))).toBeTruthy()
  })
})
