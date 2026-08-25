/**
 * e2e/fixtures/diagnostics.ts
 *
 * 执行期诊断收集 fixture：控制台错误、未捕获 JS 异常、网络请求失败、异常 HTTP 响应。
 * auto: true — 无需在 step 中显式请求，监听器随每个 page 自动挂载。
 *
 * 使用方式：
 * ```
 * const test = base.extend({ ...mockanureo, ...auth, ...diagnostics });
 * test('...', async ({ page, diagnosticsCollector }) => {
 *   expect(diagnosticsCollector.pageErrors).toEqual([]);
 * });
 * ```
 *
 * @module fixtures/diagnostics
 */

import { TestFixture } from "@playwright/test";

// ─── Types ───────────────────────────────────────────────────────────────────

export interface ConsoleErrorEntry {
  type: string;
  text: string;
  url?: string;
}

export interface NetworkErrorEntry {
  method: string;
  url: string;
  failureText?: string;
  status?: number;
}

export interface DiagnosticsCollector {
  consoleErrors: ConsoleErrorEntry[];
  pageErrors: string[];
  requestFailed: NetworkErrorEntry[];
  badResponses: NetworkErrorEntry[];
}

// ─── Filters ─────────────────────────────────────────────────────────────────

// 已知良性噪音（dev server 常见）：favicon、sourcemap、被主动 abort 的请求
const BENIGN_URL = /\/favicon\.ico|\.map(\?|$)|\/@vite\/|\/@fs\//;
const ABORTED = /ERR_ABORTED/;

function isBenignNetworkEntry(entry: NetworkErrorEntry): boolean {
  if (BENIGN_URL.test(entry.url)) return true;
  if (entry.failureText && ABORTED.test(entry.failureText)) return true;
  return false;
}

// 已知良性控制台噪音（第三方库的 dev 告警等）
const BENIGN_CONSOLE = [
  /Download the React DevTools/i,
  /\[vite\]/i,
  /Fast Refresh.*invalidated/i,
];

// ─── Fixture Implementation ─────────────────────────────────────────────────

export const diagnostics: Record<string, TestFixture> = {
  diagnosticsCollector: [
    async ({ page }, use) => {
      const collector: DiagnosticsCollector = {
        consoleErrors: [],
        pageErrors: [],
        requestFailed: [],
        badResponses: [],
      };

      page.on("console", (msg) => {
        if (msg.type() !== "error") return;
        const text = msg.text();
        if (BENIGN_CONSOLE.some((p) => p.test(text))) return;
        collector.consoleErrors.push({
          type: msg.type(),
          text,
          url: msg.location().url,
        });
      });

      page.on("pageerror", (error) => {
        collector.pageErrors.push(error.message);
      });

      page.on("requestfailed", (request) => {
        const entry: NetworkErrorEntry = {
          method: request.method(),
          url: request.url(),
          failureText: request.failure()?.errorText,
        };
        if (!isBenignNetworkEntry(entry)) collector.requestFailed.push(entry);
      });

      page.on("response", (response) => {
        if (response.status() < 400) return;
        const entry: NetworkErrorEntry = {
          method: response.request().method(),
          url: response.url(),
          status: response.status(),
        };
        if (!isBenignNetworkEntry(entry)) collector.badResponses.push(entry);
      });

      await use(collector);
    },
    { auto: true },
  ],
};
