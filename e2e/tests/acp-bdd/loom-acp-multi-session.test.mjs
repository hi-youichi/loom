import test from "node:test";
import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdtemp, rm, realpath } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";

const repoRoot = path.resolve(import.meta.dirname, "../../..");
const cargoTargetDir = process.env.CARGO_TARGET_DIR
  ? path.resolve(process.env.CARGO_TARGET_DIR)
  : path.join(repoRoot, "target");
const fixtureBinary = process.env.ACP_TEST_SERVER_BIN ?? path.join(
  cargoTargetDir,
  "debug",
  process.platform === "win32" ? "acp-test-server.exe" : "acp-test-server",
);

async function startFixture(env = process.env) {
  const child = spawn(fixtureBinary, ["--port", "0"], {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  child.stderr.setEncoding("utf8");
  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const url = await new Promise((resolve, reject) => {
    let output = "";
    const timer = setTimeout(() => reject(new Error(`fixture startup timed out: ${stderr}`)), 15_000);
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      output += chunk;
      const match = output.match(/ACP_TEST_SERVER_URL=(ws:\/\/[^\r\n]+)/);
      if (match) {
        clearTimeout(timer);
        resolve(match[1]);
      }
    });
    child.on("exit", (code) => reject(new Error(`fixture exited ${code}: ${stderr}`)));
  });
  return { child, url };
}

async function stopFixture(child) {
  child.kill();
  await Promise.race([once(child, "exit"), new Promise((resolve) => setTimeout(resolve, 1_000))]);
}

class AcpClient {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 1;
    this.pending = new Map();
    this.frames = [];
    socket.addEventListener("message", (event) => {
      const value = JSON.parse(event.data);
      this.frames.push({ sequence: this.frames.length, value });
      if (value.id !== undefined && this.pending.has(value.id)) {
        const { resolve, reject } = this.pending.get(value.id);
        this.pending.delete(value.id);
        if (value.error) reject(Object.assign(new Error(value.error.message), { rpc: value.error }));
        else resolve(value.result);
      }
    });
  }

  static async connect(url) {
    const socket = new WebSocket(url);
    await once(socket, "open");
    return new AcpClient(socket);
  }

  request(method, params = {}) {
    const id = this.nextId++;
    const promise = new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
    this.socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
    return promise;
  }

  initialize(capabilities = {}) {
    return this.request("initialize", { protocolVersion: 1, clientCapabilities: capabilities });
  }

  close() {
    this.socket.close();
  }
}

function prompt(sessionId, text) {
  return { sessionId, prompt: [{ type: "text", text }] };
}

test("BDD: one Loom server supports alternating and concurrent ACP sessions", { timeout: 30_000 }, async () => {
  const fixture = await startFixture();
  const workspaceA = await realpath(await mkdtemp(path.join(tmpdir(), "loom-acp-a-")));
  const workspaceB = await realpath(await mkdtemp(path.join(tmpdir(), "loom-acp-b-")));
  const client = await AcpClient.connect(fixture.url);
  try {
    const initialized = await client.initialize();
    assert.ok(initialized.agentCapabilities.sessionCapabilities.resume);

    const sessionA = (await client.request("session/new", { cwd: workspaceA, mcpServers: [] })).sessionId;
    const sessionB = (await client.request("session/new", { cwd: workspaceB, mcpServers: [] })).sessionId;

    await client.request("session/prompt", prompt(sessionA, "A1"));
    await client.request("session/prompt", prompt(sessionB, "B1"));
    await client.request("session/prompt", prompt(sessionA, "A2"));

    const routed = client.frames
      .filter(({ value }) => value.method === "session/update")
      .map(({ value }) => value.params.sessionId);
    assert.deepEqual(routed.slice(-3), [sessionA, sessionB, sessionA]);

    await Promise.all([
      client.request("session/prompt", prompt(sessionA, "SLOW concurrent A")),
      client.request("session/prompt", prompt(sessionB, "SLOW concurrent B")),
    ]);

    const slow = client.request("session/prompt", prompt(sessionA, "SLOW overlap"));
    await new Promise((resolve) => setTimeout(resolve, 30));
    await assert.rejects(
      client.request("session/prompt", prompt(sessionA, "second")),
      (error) => error.rpc?.code === -32010,
    );
    await slow;

    await assert.rejects(
      client.request("session/load", { sessionId: sessionA, cwd: workspaceB, mcpServers: [] }),
      (error) => error.rpc?.code === -32602,
    );
    await client.request("session/delete", { sessionId: "missing-session" });
  } finally {
    client.close();
    await stopFixture(fixture.child);
    await Promise.all([
      rm(workspaceA, { recursive: true, force: true }),
      rm(workspaceB, { recursive: true, force: true }),
    ]);
  }
});

test("BDD: a second ACP connection does not replace the first", { timeout: 20_000 }, async () => {
  const fixture = await startFixture();
  const workspace = await realpath(await mkdtemp(path.join(tmpdir(), "loom-acp-multi-")));
  const first = await AcpClient.connect(fixture.url);
  const second = await AcpClient.connect(fixture.url);
  try {
    await Promise.all([first.initialize(), second.initialize()]);
    const sessionA = (await first.request("session/new", { cwd: workspace, mcpServers: [] })).sessionId;
    const sessionB = (await second.request("session/new", { cwd: workspace, mcpServers: [] })).sessionId;
    second.close();
    await new Promise((resolve) => setTimeout(resolve, 300));
    await first.request("session/prompt", prompt(sessionA, "still alive"));
    assert.notEqual(sessionA, sessionB);
  } finally {
    first.close();
    second.close();
    await stopFixture(fixture.child);
    await rm(workspace, { recursive: true, force: true });
  }
});

test("BDD: session metadata survives a Loom server restart", { timeout: 25_000 }, async () => {
  const otherDir = await mkdtemp(path.join(tmpdir(), "loom-acp-other-"));
  const workspace = await realpath(await mkdtemp(path.join(tmpdir(), "loom-acp-persist-")));
  const env = { ...process.env };
  let fixture = await startFixture(env);
  let client = await AcpClient.connect(fixture.url);
  let sessionId;
  try {
    await client.initialize();
    sessionId = (await client.request("session/new", { cwd: workspace, mcpServers: [] })).sessionId;
    client.close();
    await stopFixture(fixture.child);

    fixture = await startFixture(env);
    client = await AcpClient.connect(fixture.url);
    await client.initialize();
    await client.request("session/load", { sessionId, cwd: workspace, mcpServers: [] });
    await assert.rejects(
      client.request("session/load", { sessionId, cwd: otherDir, mcpServers: [] }),
      (error) => error.rpc?.code === -32602,
    );
  } finally {
    client.close();
    await stopFixture(fixture.child);
    await Promise.all([
      rm(otherDir, { recursive: true, force: true }),
      rm(workspace, { recursive: true, force: true }),
    ]);
  }
});
