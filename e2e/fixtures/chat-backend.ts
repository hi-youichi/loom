/**
 * Chat backend profiles used by Web BDD.
 *
 * `mock` keeps the browser on the normal UI route but supplies a deterministic
 * ACP JSON-RPC peer. `acp` deliberately installs no route handlers and uses
 * the configured real anureo `/acp` endpoint.
 */

import type { Page } from "@playwright/test";
import path from "node:path";
import { installMockHttpRoutes } from "./mock-anureo";

export type ChatBackendMode = "mock" | "acp";

export interface ChatBackendFixtures {
  chatBackendMode: ChatBackendMode;
}

function sendJson(ws: { send(message: string): void }, value: unknown): void {
  ws.send(JSON.stringify(value));
}

function installMockAcpWebSocket(page: Page): void {
  // Keep history outside the individual WebSocket callback. A page reload
  // creates a new ACP connection, so session/load must be able to replay the
  // turn produced by the previous connection.
  const history = new Map<string, { user: string; assistant: string }[]>();
  page.routeWebSocket("**/acp", (ws) => {
    let sessionId = "mock-session-1";
    const projectPath = path.resolve(process.cwd(), "..").replaceAll("\\", "/");

    ws.onMessage((raw) => {
      let request: { id?: string | number; method?: string; params?: any };
      try {
        request = JSON.parse(String(raw));
      } catch {
        return;
      }

      const respond = (result: unknown) => {
        if (request.id === undefined) return;
        sendJson(ws, { jsonrpc: "2.0", id: request.id, result });
      };

      switch (request.method) {
        case "_anureo.dev/auth/authenticate":
          // The web runtime may perform the pre-auth handshake before ACP
          // initialize. A mock peer must acknowledge it or the SDK never
          // reaches session/new/session/prompt.
          respond({ authenticated: true });
          break;
        case "initialize":
          respond({
            protocolVersion: 1,
            agentCapabilities: {
              loadSession: true,
              promptCapabilities: {
                image: false,
                audio: false,
                embeddedContext: false,
              },
            },
          });
          break;
        case "session/new":
          // Bootstrap may issue session/load for a stale cached session before
          // the draft is materialized. Always make the canonical new-session
          // response deterministic instead of letting that unrelated request
          // overwrite the ID used by the prompt/history contract.
          sessionId = "mock-session-1";
          respond({ sessionId });
          break;
        case "session/load":
          sessionId = request.params?.sessionId ?? sessionId;
          for (const [index, turn] of (history.get(sessionId) ?? []).entries()) {
            const messageId = `mock-user-${index + 1}`;
            const assistantId = `mock-assistant-${index + 1}`;
            setTimeout(() => sendJson(ws, {
              jsonrpc: "2.0",
              method: "session/update",
              params: {
                sessionId,
                update: {
                  sessionUpdate: "user_message_chunk",
                  messageId,
                  content: { type: "text", text: turn.user },
                },
              },
            }), index * 2);
            setTimeout(() => sendJson(ws, {
              jsonrpc: "2.0",
              method: "session/update",
              params: {
                sessionId,
                update: {
                  sessionUpdate: "agent_message_chunk",
                  messageId: assistantId,
                  content: { type: "text", text: turn.assistant },
                },
              },
            }), index * 2 + 1);
          }
          setTimeout(() => respond({
            _meta: {
              "anureo.dev": {
                sessionRecovery: {
                  version: 1,
                  mode: "full",
                  streamId: "mock-stream",
                  throughSeq: 0,
                  promptState: "idle",
                },
              },
            },
          }), 10);
          break;
        case "session/prompt":
          // Keep notification and request completion on separate turns of the
          // browser event loop. A real ACP peer delivers these over the
          // WebSocket asynchronously; emitting both synchronously can make
          // React project two native-store updates while it is still
          // rendering, producing a misleading "Maximum update depth" error.
          const promptBlocks = Array.isArray(request.params?.prompt)
            ? request.params.prompt
            : [];
          const userText = promptBlocks
            .map((block: any) => block?.text ?? "")
            .join("");
          const turns = history.get(sessionId) ?? [];
          turns.push({ user: userText, assistant: "fake response" });
          history.set(sessionId, turns);
          setTimeout(() => sendJson(ws, {
            jsonrpc: "2.0",
            method: "session/update",
            params: {
              sessionId,
              update: {
                sessionUpdate: "agent_message_chunk",
                content: { type: "text", text: "fake response" },
              },
            },
          }), 0);
          setTimeout(() => respond({ stopReason: "end_turn" }), 0);
          break;
        case "session/cancel":
          respond({});
          break;
        case "_anureo.dev/session/list-global":
        case "_anureo.dev/session/list":
          respond({
            sessions: [...history.keys()].map((id) => ({
              sessionId: id,
              cwd: projectPath,
              title: history.get(id)?.[0]?.user ?? "Mock session",
              metadata: {},
              lifecycle: "idle",
            })),
            nextCursor: null,
            hasMore: false,
          });
          break;
        case "_anureo.dev/project/list":
          respond({
            items: [{
              id: "mock-project",
              name: "Mock Project",
              path: projectPath,
              isActive: true,
            }],
          });
          break;
        case "_anureo.dev/model/list":
          respond({
            providers: [
              {
                id: "mock-provider",
                name: "Mock Provider",
                models: [{ id: "mock-model", name: "Mock Model" }],
              },
            ],
            default: { "mock-provider": "mock-model" },
          });
          break;
        case "_anureo.dev/provider/list":
          respond({ providers: [{ id: "mock-provider", name: "Mock Provider" }] });
          break;
        case "_anureo.dev/settings/load":
          respond({
            version: 1,
            settings: { defaultModel: "mock-provider/mock-model" },
          });
          break;
        case "_anureo.dev/settings/save":
          respond({ version: 1 });
          break;
        default:
          // The web shell performs capability/bootstrap calls before the
          // first prompt. Keep those calls harmless so the chat contract can
          // reach session/new without implementing every extension.
          respond(request.method?.includes("list") ? [] : {});
      }
    });
  });
}

export const chatBackend: Record<string, unknown> = {
  chatBackendMode: ["mock", { option: true }],
  page: async ({ page, chatBackendMode }, use) => {
    if (chatBackendMode === "mock") {
      const projectPath = path.resolve(process.cwd(), "..").replaceAll("\\", "/");
      await page.addInitScript(({ projectPath: initialPath }) => {
        const project = {
          id: "mock-project",
          path: initialPath,
          label: "Mock Project",
          addedAt: Date.now(),
        };
        localStorage.setItem("projects", JSON.stringify([project]));
        localStorage.setItem("activeProjectId", project.id);
        localStorage.setItem("lastDirectory", initialPath);
      }, { projectPath });
      await installMockHttpRoutes(page);
      installMockAcpWebSocket(page);
    }
    await use(page);
  },
};
