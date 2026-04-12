import { test, expect } from '@playwright/test'

test.describe('ModelSelector', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForSelector('.model-selector__trigger')
  })

  test('opens model list on trigger click', async ({ page }) => {
    await page.click('.model-selector__trigger')

    await expect(page.locator('.model-selector__content input')).toBeVisible()
    const groups = page.locator('.model-selector__group-title')
    await expect(groups.first()).toBeVisible()
    const groupCount = await groups.count()
    expect(groupCount).toBeGreaterThanOrEqual(1)

    const buttons = page.locator('.model-selector__content button')
    const buttonCount = await buttons.count()
    expect(buttonCount).toBeGreaterThanOrEqual(1)
  })

  test('groups models by provider', async ({ page }) => {
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__group-title')

    const groups = page.locator('.model-selector__group-title')
    const groupCount = await groups.count()
    expect(groupCount).toBeGreaterThanOrEqual(1)

    for (let i = 0; i < groupCount; i++) {
      const title = await groups.nth(i).textContent()
      expect(title?.trim().length).toBeGreaterThan(0)
    }
  })

  test('filters models by search query', async ({ page }) => {
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__group-title')

    const searchInput = page.locator('.model-selector__content input')
    await searchInput.fill('qwen')

    await page.waitForTimeout(300)

    const visibleButtons = page.locator('.model-selector__content button')
    const count = await visibleButtons.count()
    expect(count).toBeGreaterThanOrEqual(1)

    for (let i = 0; i < count; i++) {
      const text = await visibleButtons.nth(i).textContent()
      expect(text?.toLowerCase()).toContain('qwen')
    }
  })

  test('shows no results for unmatched search', async ({ page }) => {
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__group-title')

    const searchInput = page.locator('.model-selector__content input')
    await searchInput.fill('xyz123noexist')

    await page.waitForTimeout(300)

    const groups = page.locator('.model-selector__group-title')
    const groupCount = await groups.count()
    expect(groupCount).toBe(0)

    const buttons = page.locator('.model-selector__content button')
    const buttonCount = await buttons.count()
    expect(buttonCount).toBe(0)

    const noResults = page.locator('.model-selector__content .text-muted-foreground')
    await expect(noResults).toHaveText('No models found')
  })

  test('selects a model and closes popover', async ({ page }) => {
    await page.click('.model-selector__trigger')
    await page.waitForSelector('.model-selector__content button')

    const firstModel = page.locator('.model-selector__content button').first()
    const modelName = await firstModel.textContent()
    await firstModel.click()

    const content = page.locator('.model-selector__content')
    await expect(content).not.toBeVisible()

    const trigger = page.locator('.model-selector__trigger')
    await expect(trigger).toContainText(modelName?.trim() || '')
  })
})
