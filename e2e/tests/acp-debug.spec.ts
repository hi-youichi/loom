import { test, expect } from "@playwright/test";

const SESSION_URL = "http://localhost:5180/session/session-f5d6310a-9ec4-4cb9-bbe6-cb3e9bdb3fc4";

test("ACP 连接状态诊断", async ({ page }) => {
  const wsMessages: any[] = [];
  const apiRequests: any[] = [];
  const errors: string[] = [];

  page.on("websocket", (ws) => {
    console.log("WebSocket 连接:", ws.url());
    ws.on("framereceived", (frame) => {
      try {
        const data = JSON.parse(frame.payload.toString());
        wsMessages.push({ direction: "received", data });
        console.log("WS 收到:", data);
      } catch {
        wsMessages.push({ direction: "received", raw: frame.payload.toString() });
      }
    });
    ws.on("framesent", (frame) => {
      try {
        const data = JSON.parse(frame.payload.toString());
        wsMessages.push({ direction: "sent", data });
        console.log("WS 发送:", data);
      } catch {
        wsMessages.push({ direction: "sent", raw: frame.payload.toString() });
      }
    });
  });

  page.on("request", (request) => {
    const url = request.url();
    if (url.includes("/api/") || url.includes("/acp/")) {
      apiRequests.push({
        url,
        method: request.method()
      });
      console.log(`API 请求: ${request.method()} ${url}`);
    }
  });

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      errors.push(msg.text());
      console.log("控制台错误:", msg.text());
    } else if (msg.text().includes("acp") || msg.text().includes("ACP")) {
      console.log("ACP 相关日志:", msg.text());
    }
  });

  console.log("开始加载页面...");
  await page.goto(SESSION_URL, { waitUntil: "networkidle", timeout: 30000 });
  
  // 等待一段时间观察 WebSocket 和 API 活动
  await page.waitForTimeout(5000);

  console.log("\n=== WebSocket 连接统计 ===");
  console.log(`总共 ${wsMessages.length} 条 WebSocket 消息`);
  
  const acpMessages = wsMessages.filter(msg => 
    msg.data?.method?.includes("acp") || 
    msg.data?.result?.acp || 
    msg.data?.params?.acp
  );
  console.log(`其中 ${acpMessages.length} 条 ACP 相关消息`);

  console.log("\n=== API 请求统计 ===");
  console.log(`总共 ${apiRequests.length} 个 API 请求`);

  console.log("\n=== 控制台错误统计 ===");
  console.log(`总共 ${errors.length} 个错误`);
  if (errors.length > 0) {
    errors.forEach(err => console.log("-", err));
  }

  // 检查页面中的 ACP 状态
  const acpStatus = await page.evaluate(() => {
    try {
      // 尝试获取 ACP 运行时状态
      return (window as any).__acp_runtime_status__ || "unknown";
    } catch {
      return "unavailable";
    }
  });
  console.log(`\n=== ACP 运行时状态: ${acpStatus} ===`);

  // 截图
  await page.screenshot({ path: "acp-debug-screenshot.png", fullPage: true });
  console.log("\n截图已保存到 acp-debug-screenshot.png");
});