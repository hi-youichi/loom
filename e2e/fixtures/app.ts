import { test as base, TestFixture } from "@playwright/test";

function getBaseUrl(): string {
  return process.env.E2E_BASE_URL ?? "http://localhost:3000";
}

export interface AppFixture {
  baseURL: string;
}

/**
 * App fixture — 提供 baseURL。
 *
 * 在测试中合并：
 * ```
 * import { test as base, expect } from "@playwright/test";
 * import { app } from "./fixtures/app";
 * const test = base.extend({ ...app });
 * ```
 */
export const app: Record<string, TestFixture> = {
  baseURL: async ({}, use) => {
    await use(getBaseUrl());
  },
};

export { expect } from "@playwright/test";
