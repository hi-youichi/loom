import { test, expect } from "@playwright/test";

const SESSION_URL = "http://localhost:5180/session/session-f5d6310a-9ec4-4cb9-bbe6-cb3e9bdb3fc4";

test("会话页面加载诊断", async ({ page }) => {
  const errors: string[] = [];
  const failedRequests: any[] = [];
  const apiRequests: any[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      errors.push(`Console error: ${msg.text()}`);
    } else if (msg.type() === "warning") {
      console.log(`Console warning: ${msg.text()}`);
    }
  });

  page.on("pageerror", (error) => {
    errors.push(`Page error: ${error.message}`);
  });

  page.on("requestfailed", (request) => {
    failedRequests.push({
      url: request.url(),
      method: request.method(),
      failure: request.failure()
    });
  });

  page.on("request", (request) => {
    if (request.url().includes("/api/") || request.url().includes("/acp/")) {
      apiRequests.push({
        url: request.url(),
        method: request.method()
      });
    }
  });

  console.log("开始加载页面:", SESSION_URL);
  
  const response = await page.goto(SESSION_URL, {
    waitUntil: "networkidle",
    timeout: 30000
  });

  console.log("页面响应状态:", response?.status());
  console.log("页面 URL:", page.url());

  await page.waitForTimeout(3000);

  console.log("\n=== 页面标题 ===");
  console.log(await page.title());

  console.log("\n=== API 请求列表 ===");
  apiRequests.forEach(req => {
    console.log(`${req.method} ${req.url}`);
  });

  console.log("\n=== 失败的请求 ===");
  failedRequests.forEach(failed => {
    console.log(`${failed.method} ${failed.url} - ${failed.failure?.errorText}`);
  });

  console.log("\n=== 控制台错误 ===");
  if (errors.length > 0) {
    errors.forEach(error => console.log(error));
  } else {
    console.log("无控制台错误");
  }

  console.log("\n=== 页面元素检查 ===");
  
  const bodyText = await page.locator("body").textContent();
  console.log("页面文本内容预览:", bodyText?.substring(0, 200));

  const emptyStates = page.locator(".empty-state, [data-testid*='empty'], .error, [role='alert']");
  const count = await emptyStates.count();
  console.log(`找到 ${count} 个空状态或错误元素`);
  
  if (count > 0) {
    for (let i = 0; i < Math.min(count, 5); i++) {
      const element = emptyStates.nth(i);
      console.log(await element.textContent());
    }
  }

  const sessionContainer = page.locator("[data-testid*='session'], .session-container, .session-content");
  const sessionCount = await sessionContainer.count();
  console.log(`找到 ${sessionCount} 个会话容器元素`);

  const messageElements = page.locator(".message, [data-testid*='message']");
  const messageCount = await messageElements.count();
  console.log(`找到 ${messageCount} 个消息元素`);

  console.log("\n=== 截图 ===");
  await page.screenshot({ path: "session-debug-screenshot.png", fullPage: true });
  console.log("截图已保存到 session-debug-screenshot.png");

  if (errors.length > 0 || failedRequests.length > 0) {
    throw new Error(`发现 ${errors.length} 个错误和 ${failedRequests.length} 个失败请求`);
  }
});