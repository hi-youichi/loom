/**
 * e2e/fixtures/temp-project.ts
 *
 * Playwright test fixture for creating temporary project directories.
 *
 * 功能：
 * - 在系统临时目录创建项目文件夹
 * - 创建基本项目结构（README.md、src/index.ts）
 * - 测试结束后自动清理
 *
 * @module fixtures/temp-project
 */

import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { TestFixture } from "@playwright/test";

// ─── Types ──────────────────────────────────────────────────────────────────

export interface TempProjectFixture {
  /** 临时项目目录路径 */
  tempProject: { projectDir: string };
}

// ─── Fixture Implementation ─────────────────────────────────────────────────

/**
 * tempProject fixture。
 *
 * 在系统临时目录创建一个临时项目文件夹，用于 E2E 测试。
 *
 * 创建的项目结构：
 * ```
 * <temp-dir>/
 * ├── README.md
 * └── src/
 *     └── index.ts
 * ```
 *
 * 测试结束后自动清理。
 */
export const tempProject: Record<string, TestFixture> = {
  tempProject: async ({}, use) => {
    // 创建临时目录
    const projectDir = mkdtempSync(join(tmpdir(), "oc-e2e-"));

    // 创建基本项目结构
    writeFileSync(join(projectDir, "README.md"), "# Test Project\n");
    mkdirSync(join(projectDir, "src"), { recursive: true });
    writeFileSync(join(projectDir, "src", "index.ts"), "// placeholder\n");

    console.log("[temp-project] Created:", projectDir);

    await use({ projectDir });

    // 清理临时目录
    try {
      rmSync(projectDir, { recursive: true, force: true });
      console.log("[temp-project] Cleaned up:", projectDir);
    } catch (err) {
      console.warn("[temp-project] Cleanup failed:", err);
    }
  },
};
