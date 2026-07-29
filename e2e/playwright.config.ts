import { fileURLToPath } from "node:url";
import path from "node:path";
import { defineConfig, devices } from "@playwright/test";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// 仓库根目录（本配置位于 e2e/ 下，`npm run dev` 脚本定义在仓库根 package.json）
const repoRoot = path.resolve(__dirname, "..");

/**
 * OpenChamber Web Server 地址。
 * 默认 http://localhost:3000，可通过 E2E_BASE_URL 覆盖（CI 场景常用）。
 */
function getBaseUrl(): string {
  return process.env.E2E_BASE_URL ?? "http://localhost:3000";
}

export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  // CI 中禁止 .only；本地放开
  forbidOnly: !!process.env.CI,
  // CI 重试 2 次，本地 0 次
  retries: process.env.CI ? 2 : 0,
  workers: 1, // 单 worker 避免测试间共享状态导致的不稳定（localStorage/React state）
  reporter: process.env.CI
    ? [["html", { open: "never" }], ["github"]]
    : [["html", { open: "on-failure" }], ["list"]],

  use: {
    baseURL: getBaseUrl(),
    // 首次重试时采集 trace 与截图
    trace: "on-first-retry",
    screenshot: "on-first-retry",
    video: "retain-on-failure",
    actionTimeout: 10_000,
    navigationTimeout: 15_000,
  },

  projects: [
    // P0 — Web (Chromium)
    {
      name: "Web Chromium",
      use: {
        ...devices["Desktop Chrome"],
        headless: process.env.E2E_HEADLESS !== "false",
      },
      testIgnore: /mobile\.spec\.ts/,
    },

    // P1 — Mobile viewport（复用 Web Server，仅切换视口 / UA）
    {
      name: "Mobile Chrome",
      use: {
        ...devices["Pixel 7"],
      },
      testMatch: /mobile\.spec\.ts/,
    },
  ],

  // Web Server 自动启动（开发模式下）
  // `npm run dev` 定义在仓库根 package.json，因此 cwd 指向仓库根目录。
  // 设置 E2E_NO_AUTOSTART=1 可禁用自动启动（连接外部已运行的 server）。
  //
  // E2E_NODE_FALLBACK=1 时：直接用 node 运行 openchamber serve --foreground，
  // 不走 npm run dev → scripts/dev-web-hmr.mjs → spawn bun 的链路。
  // playwright 的 env 不支持直接在 command 字符串里设变量，
  // 所以用单独的 env 对象注入 E2E_NO_AUTOSTART=1 来绕过 npm dev 脚本，
  // 改用 node 直接启动（--foreground 使其在前台运行，适合 Playwright 生命周期管理）。
  webServer: process.env.E2E_NO_AUTOSTART
    ? undefined
    : {
        command: "node packages/web/bin/cli.js serve --foreground",
        url: getBaseUrl(),
        cwd: repoRoot,
        reuseExistingServer: !process.env.CI,
        timeout: 120_000,
      },
});
