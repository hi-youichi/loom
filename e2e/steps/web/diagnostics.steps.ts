/**
 * e2e/steps/web/diagnostics.steps.ts
 *
 * 诊断类步骤：控制台错误、JS 异常、网络错误的运行期断言。
 * 数据来源 fixtures/diagnostics.ts（auto 挂载的 diagnosticsCollector）。
 *
 * @module steps/web/diagnostics.steps
 */

import { Then } from "./fixtures";
import { expect } from "@playwright/test";

Then("无 JavaScript 报错", async ({ diagnosticsCollector }) => {
  expect(
    diagnosticsCollector.pageErrors,
    "执行期间出现未捕获 JS 异常",
  ).toEqual([]);
});

Then("控制台无错误输出", async ({ diagnosticsCollector }) => {
  expect(
    diagnosticsCollector.consoleErrors,
    "执行期间控制台输出了 error 级别消息",
  ).toEqual([]);
});

Then("无网络请求失败", async ({ diagnosticsCollector }) => {
  expect(
    diagnosticsCollector.requestFailed,
    "执行期间存在网络请求失败（已排除 abort/favicon/sourcemap）",
  ).toEqual([]);
});

Then("网络无异常响应", async ({ diagnosticsCollector }) => {
  expect(
    diagnosticsCollector.badResponses,
    "执行期间存在 4xx/5xx 响应（已排除 favicon/sourcemap）",
  ).toEqual([]);
});
