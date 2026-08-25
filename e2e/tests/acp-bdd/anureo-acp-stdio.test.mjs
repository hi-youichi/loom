import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { once } from "node:events";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import test from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "../../..");
const cargoTargetDir = process.env.CARGO_TARGET_DIR
  ? path.resolve(process.env.CARGO_TARGET_DIR)
  : path.join(repoRoot, "target");
const anureoBinary = process.env.ANUREO_BIN ?? defaultanureoBinary();
const timeoutMs = Number(process.env.ACP_BDD_TIMEOUT_MS ?? 15_000);

function defaultanureoBinary() {
  return process.platform === "win32"
    ? path.join(cargoTargetDir, "debug", "anureo.exe")
    : path.join(cargoTargetDir, "debug", "anureo");
}

function requireanureoBinary() {
  if (!anureoBinary) {
    throw new Error("Set ANUREO_BIN to a built anureo executable");
  }
}

function waitForLine(stream, predicate, label) {
  return new Promise((resolve, reject) => {
    let buffer = "";
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`Timed out waiting for ${label}`));
    }, timeoutMs);

    const onData = (chunk) => {
      buffer += chunk.toString();
      const lines = buffer.split(/\r?\n/);
      buffer = lines.pop() ?? "";
      for (const line of lines) {
        if (predicate(line)) {
          cleanup();
          resolve(line);
          return;
        }
      }
    };
    const onError = (error) => {
      cleanup();
      reject(error);
    };
    const cleanup = () => {
      clearTimeout(timer);
      stream.off("data", onData);
      stream.off("error", onError);
    };
    stream.on("data", onData);
    stream.on("error", onError);
  });
}

async function startServer(env, home) {
  requireanureoBinary();
  const child = spawn(anureoBinary, ["server", "--host", "127.0.0.1", "--port", "0", "--home", home], {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });

  const line = await waitForLine(
    child.stdout,
    (value) => value.includes("anureo server listening on http://"),
    "anureo server address",
  );
  const match = line.match(/http:\/\/(127\.0\.0\.1:\d+)/);
  assert.ok(match, `unexpected server address line: ${line}`);

  return {
    child,
    wsUrl: `ws://${match[1]}/acp`,
    async close() {
      child.kill();
      await Promise.race([once(child, "exit"), new Promise((resolve) => setTimeout(resolve, 500))]);
    },
  };
}

class JsonRpcLines {
  #nextId = 1;
  #pending = new Map();
  #buffer = "";
  #notifications = [];

  constructor(child) {
    this.child = child;
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => this.#consume(chunk));
    child.on("exit", (code, signal) => {
      for (const pending of this.#pending.values()) {
        pending.reject(new Error(`ACP bridge exited: code=${code}, signal=${signal}`));
      }
      this.#pending.clear();
    });
  }

  #consume(chunk) {
    this.#buffer += chunk;
    const lines = this.#buffer.split(/\r?\n/);
    this.#buffer = lines.pop() ?? "";
    for (const line of lines) {
      if (!line.trim()) continue;
      const message = JSON.parse(line);
      if (message.id !== undefined) {
        const pending = this.#pending.get(message.id);
        if (!pending) continue;
        this.#pending.delete(message.id);
        if (message.error) pending.reject(message.error);
        else pending.resolve(message.result);
      } else {
        this.#notifications.push(message);
      }
    }
  }

  request(method, params) {
    const id = this.#nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`Timed out waiting for ${method} response`));
      }, timeoutMs);
      this.#pending.set(id, {
        resolve: (value) => { clearTimeout(timer); resolve(value); },
        reject: (error) => { clearTimeout(timer); reject(error); },
      });
      this.child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  }

  async close() {
    this.child.stdin.end();
    this.child.kill();
    await Promise.race([once(this.child, "exit"), new Promise((resolve) => setTimeout(resolve, 500))]);
  }
}

async function startBridge(wsUrl, env, home) {
  requireanureoBinary();
  const child = spawn(anureoBinary, ["acp", "--home", home, wsUrl], {
    cwd: repoRoot,
    env,
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  return new JsonRpcLines(child);
}

async function withAcpScenario(run) {
  const home = await mkdtemp(path.join(tmpdir(), "anureo-acp-bdd-"));
  const env = { ...process.env };
  const server = await startServer(env, home);
  try {
    await run(server, env, home);
  } finally {
    await server.close();
    await rm(home, { recursive: true, force: true });
  }
}

function initializeParams() {
  return {
    protocolVersion: 1,
    clientInfo: { name: "anureo-node-bdd", version: "0.1.0" },
    clientCapabilities: {},
  };
}

function zedInitializeParams() {
  return {
    protocolVersion: 1,
    clientInfo: {
      name: "zed",
      title: "Zed",
      version: "0.228.0+stable.203.8421009ef8a022df1196d54bb42fd94366ec0988",
    },
    clientCapabilities: {
      fs: { readTextFile: true, writeTextFile: true },
      terminal: true,
      auth: { terminal: false },
      _meta: { terminal_output: true, "terminal-auth": true },
    },
  };
}

test("BDD: Given a anureo server, When I initialize through anureo acp, Then ACP responds", async () => {
  await withAcpScenario(async (server, env, home) => {
    const bridge = await startBridge(server.wsUrl, env, home);
    try {
      const result = await bridge.request("initialize", initializeParams());
      assert.equal(typeof result.protocolVersion, "number");
      assert.ok(result.agentCapabilities);
    } finally {
      await bridge.close();
    }
  });
});

test("BDD: Given a session, When I restart anureo acp, Then I can load it", async () => {
  await withAcpScenario(async (server, env, home) => {
    const cwd = repoRoot;
    const first = await startBridge(server.wsUrl, env, home);
    const created = await first.request("initialize", initializeParams())
      .then(() => first.request("session/new", { cwd, mcpServers: [] }));
    assert.ok(created.sessionId);
    await first.close();

    const second = await startBridge(server.wsUrl, env, home);
    try {
      await second.request("initialize", initializeParams());
      const loaded = await second.request("session/load", {
        sessionId: created.sessionId,
        cwd,
        mcpServers: [],
      });
      assert.ok(loaded);
    } finally {
      await second.close();
    }
  });
});

test("Zed ACP smoke: two stdio bridges share one server without session takeover", async () => {
  await withAcpScenario(async (server, env, home) => {
    const first = await startBridge(server.wsUrl, env, home);
    const second = await startBridge(server.wsUrl, env, home);
    try {
      await Promise.all([
        first.request("initialize", zedInitializeParams()),
        second.request("initialize", zedInitializeParams()),
      ]);
      const firstSession = await first.request("session/new", {
        cwd: repoRoot,
        mcpServers: [],
      });
      const secondSession = await second.request("session/new", {
        cwd: path.join(repoRoot, "e2e"),
        mcpServers: [],
      });
      assert.notEqual(firstSession.sessionId, secondSession.sessionId);

      const listed = await first.request("session/list", {});
      const listedIds = new Set(listed.sessions.map((session) => session.sessionId));
      assert.ok(listedIds.has(firstSession.sessionId));
      assert.ok(listedIds.has(secondSession.sessionId));

      await second.close();
      const thirdSession = await first.request("session/new", {
        cwd: repoRoot,
        mcpServers: [],
      });
      assert.ok(thirdSession.sessionId);
    } finally {
      await first.close();
    }
  });
});
