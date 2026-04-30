import { test, expect } from '@playwright/test'
import fs from 'fs'
import path from 'path'
import os from 'os'
import { WebSocket as Ws } from 'ws'

declare global {
  var __workspaceRootDir: string
}

function getRootDir(): string {
  try {
    return fs.readFileSync(path.join(os.tmpdir(), 'loom-test-workspace-root-dir.txt'), 'utf8').trim()
  } catch {
    return path.join(os.tmpdir(), 'loom-test-workspace-root')
  }
}

async function wsRequest(msg: object): Promise<any> {
  const ws = new Ws('ws://127.0.0.1:8080')
  await new Promise<void>((r) => ws.on('open', r))
  const resp: any = await new Promise((resolve) => {
    ws.on('message', (data) => {
      const m = JSON.parse(data.toString())
      if (m.type === msg.type && m.id === (msg as any).id) resolve(m)
    })
    ws.send(JSON.stringify(msg))
  })
  ws.close()
  return resp
}

async function getLatestWorkspaceId(): Promise<string> {
  const resp = await wsRequest({ type: 'workspace_list', id: 'get-ws-list' })
  const workspaces = resp.workspaces || []
  if (workspaces.length === 0) throw new Error('No workspaces found')
  return workspaces[workspaces.length - 1].id
}

async function waitForFileItems(page: import('@playwright/test').Page, timeout = 10000): Promise<number> {
  const start = Date.now()
  while (Date.now() - start < timeout) {
    const count = await page.locator('[data-testid^="file-item-"]').count()
    if (count > 0) return count
    await page.waitForTimeout(500)
  }
  return 0
}

async function createWorkspaceWithFiles(page: import('@playwright/test').Page, name: string): Promise<string> {
  await page.click('[data-testid="workspace-selector"]')
  await page.click('[data-testid="workspace-create-btn"]')
  await page.fill('[data-testid="workspace-create-input"]', name)
  await page.locator('[data-testid="workspace-create-input"]').press('Enter')

  await expect(page.locator('[data-testid="selected-workspace-name"]')).toContainText(name, { timeout: 5000 })

  const workspaceId = await getLatestWorkspaceId()
  const wsDir = path.join(getRootDir(), workspaceId)

  fs.mkdirSync(path.join(wsDir, 'src'), { recursive: true })
  fs.mkdirSync(path.join(wsDir, 'assets'), { recursive: true })
  fs.writeFileSync(path.join(wsDir, 'README.md'), '# Test Project')
  fs.writeFileSync(path.join(wsDir, 'package.json'), '{"name": "test"}')
  fs.writeFileSync(path.join(wsDir, 'src', 'main.ts'), 'console.log("hello")')
  fs.writeFileSync(path.join(wsDir, 'src', 'utils.ts'), 'export function add(a: number, b: number) { return a + b }')
  fs.writeFileSync(path.join(wsDir, 'assets', 'logo.svg'), '<svg></svg>')

  await page.reload()
  await page.waitForSelector('[data-testid="file-sidebar"]', { timeout: 10000 })
  await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
  await page.locator('[data-testid="view-files"]').click({ force: true })

  const count = await waitForFileItems(page)
  if (count === 0) throw new Error('File items did not appear after reload')

  return workspaceId
}

test.describe('Workspace File List', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForSelector('.composer')
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
  })

  test('should display file sidebar', async ({ page }) => {
    await expect(page.locator('[data-testid="file-sidebar"]')).toBeVisible()
  })

  test('should show dashboard view by default', async ({ page }) => {
    await expect(page.locator('[data-testid="view-dashboard"]')).toBeVisible()
    await expect(page.locator('[data-testid="view-files"]')).toBeVisible()
  })

  test('should switch to files view', async ({ page }) => {
    await createWorkspaceWithFiles(page, 'File Test WS')
    const count = await waitForFileItems(page)
    expect(count).toBeGreaterThan(0)
  })

  test('should list root files and folders after workspace creation', async ({ page }) => {
    await createWorkspaceWithFiles(page, 'File List WS')

    const fileItems = page.locator('[data-testid^="file-item-"]')
    const allItems = await fileItems.all()
    const names = await Promise.all(allItems.map(item => item.locator('span.truncate').textContent()))

    expect(names.some(n => n?.includes('src'))).toBeTruthy()
    expect(names.some(n => n?.includes('assets'))).toBeTruthy()
    expect(names.some(n => n?.includes('README.md'))).toBeTruthy()
    expect(names.some(n => n?.includes('package.json'))).toBeTruthy()

    const folders = page.locator('[data-file-type="folder"]')
    const files = page.locator('[data-file-type="file"]')
    expect(await folders.count() + await files.count()).toBe(4)
  })

  test('should display folders before files', async ({ page }) => {
    await createWorkspaceWithFiles(page, 'Sort Order WS')

    const allItems = await page.locator('[data-testid^="file-item-"]').all()
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
    await srcFolder.click()
    await page.waitForTimeout(500)

    const allNames = await Promise.all(
      (await page.locator('[data-testid^="file-item-"]').all()).map(item => item.locator('span.truncate').textContent())
    )
    expect(allNames.some(n => n?.includes('main.ts'))).toBeTruthy()
    expect(allNames.some(n => n?.includes('utils.ts'))).toBeTruthy()
  })

  test('should show empty state when no files', async ({ page }) => {
    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="workspace-create-btn"]')
    await page.fill('[data-testid="workspace-create-input"]', 'Empty WS')
    await page.locator('[data-testid="workspace-create-input"]').press('Enter')
    await expect(page.locator('[data-testid="selected-workspace-name"]')).toContainText('Empty WS', { timeout: 5000 })

    await page.locator('[data-testid="view-files"]').click({ force: true })
    await page.waitForTimeout(1000)

    const emptyState = page.locator('[data-testid="file-empty"]')
    if (await emptyState.isVisible()) {
      await expect(emptyState).toContainText('没有找到匹配的文件')
    }
  })

  test('should update file list after switching workspace', async ({ page }) => {
    await createWorkspaceWithFiles(page, 'WS Alpha')
    expect(await page.locator('[data-testid^="file-item-"]').count()).toBeGreaterThan(0)

    await page.click('[data-testid="workspace-selector"]')
    await page.click('[data-testid="workspace-create-btn"]')
    await page.fill('[data-testid="workspace-create-input"]', 'WS Beta')
    await page.locator('[data-testid="workspace-create-input"]').press('Enter')
    await expect(page.locator('[data-testid="selected-workspace-name"]')).toContainText('WS Beta', { timeout: 5000 })

    const ws2Id = await getLatestWorkspaceId()
    const ws2Dir = path.join(getRootDir(), ws2Id)
    fs.mkdirSync(ws2Dir, { recursive: true })
    fs.writeFileSync(path.join(ws2Dir, 'hello.txt'), 'world')

    await page.reload()
    await page.waitForSelector('[data-testid="file-sidebar"]', { timeout: 10000 })
    await page.waitForSelector('.model-selector__trigger', { timeout: 15000 })
    await page.locator('[data-testid="view-files"]').click({ force: true })

    const count = await waitForFileItems(page)
    expect(count).toBeGreaterThan(0)

    const names = await Promise.all(
      (await page.locator('[data-testid^="file-item-"]').all()).map(item => item.locator('span.truncate').textContent())
    )
    expect(names.some(n => n?.includes('hello.txt'))).toBeTruthy()
  })
})
