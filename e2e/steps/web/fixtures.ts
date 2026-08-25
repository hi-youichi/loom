/**
 * e2e/steps/web/fixtures.ts
 *
 * BDD 装配点（唯一一处 fixture extend + createBdd）。
 * 所有 web step 文件从这里 import { Given, When, Then }；
 * 存量 spec 的重复 extend 样板在新 BDD 用例中不再出现。
 *
 * @module steps/web/fixtures
 */

import { test as base, createBdd } from "playwright-bdd";
import { mockanureo } from "../../fixtures/mock-anureo";
import { auth } from "../../fixtures/auth";
import { diagnostics } from "../../fixtures/diagnostics";

export const test = base.extend({
  ...mockanureo,
  ...auth,
  ...diagnostics,
});

export const { Given, When, Then } = createBdd(test);
