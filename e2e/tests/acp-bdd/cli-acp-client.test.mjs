import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { once } from "node:events";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { startFakeAcpServer } from "./fake-acp-server.mjs";

const repoRoot = path.resolve(import.meta.dirname, "../../..");
const cargoTargetDir = process.env.CARGO_TARGET_DIR
  ? path.resolve(process.env.CARGO_TARGET_DIR)
  : path.join(repoRoot, "target");
const loomBinary = process.env.LOOM_BIN ?? path.join(
  cargoTargetDir,
  "debug",
  process.platform === "win32" ? "loom.exe" : "loom",
);
const timeoutMs = 15_000;

function waitForServerLine(stream) {
  return new Promise((resolve, reject) => {
    let buffer = "";
    const timer = setTimeout(() => reject(new Error("timed out waiting for loom server")), timeoutMs);
    stream.setEncoding("utf8");
    stream.on("data", (chunk) => {
      buffer += chunk;
      const line = buffer.split(/\r?\n/).find((item) => item.includes("loom server listening on http://"));
      if (!line) return;
      clearTimeout(timer);
      const match = line.match(/http:\/\/(127\.0\.0\.1:\d+)/);
      if (!match) reject(new Error(`unexpected server line: ${line}`));
      else resolve(`ws://${match[1]}/acp`);
    });
    stream.on("error", reject);
  });
}

async function startServer(env, home) {
  const child = spawn(loomBinary, ["server", "--host", "127.0.0.1", "--port", "0", "--home", home], {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  const wsUrl = await waitForServerLine(child.stdout);
  return { child, wsUrl };
}

async function stop(child) {
  child.kill();
  await Promise.race([once(child, "exit"), new Promise((resolve) => setTimeout(resolve, 500))]);
}

async function runCli(args, env) {
  const child = spawn(loomBinary, args, {
    cwd: repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const [code, signal] = await Promise.race([
    once(child, "exit"),
    new Promise((_, reject) => setTimeout(() => reject(new Error(`CLI timed out: ${stderr}`)), timeoutMs)),
  ]);
  return { code, signal, stdout, stderr };
}

async function withServer(run) {
  const home = await mkdtemp(path.join(tmpdir(), "loom-cli-acp-bdd-"));
  const env = { ...process.env };
  const server = await startServer(env, home);
  try {
    await run(server.wsUrl, env, home);
  } finally {
    await stop(server.child);
    await rm(home, { recursive: true, force: true });
  }
}

test("BDD: Given a server, When I run loom --acp, Then the CLI client creates a session", async () => {
  await withServer(async (wsUrl, env, home) => {
    const result = await runCli(["--home", home, "--acp", "--json", "--acp-url", wsUrl], env);
    assert.equal(result.code, 0, result.stderr);
    const frames = result.stdout.trim().split(/\r?\n/).map(JSON.parse);
    const sessionFrame = frames.find((frame) => frame.method === "session/new");
    assert.ok(sessionFrame?.result?.sessionId);
  });
});

test("BDD: Given a session id, When I run loom --acp, Then the CLI client resumes the session", async () => {
  await withServer(async (wsUrl, env, home) => {
    const first = await runCli(["--home", home, "--acp", "--json", "--acp-url", wsUrl], env);
    assert.equal(first.code, 0, first.stderr);
    const created = first.stdout.trim().split(/\r?\n/).map(JSON.parse)
      .find((frame) => frame.method === "session/new");
    const sessionId = created?.result?.sessionId;
    assert.ok(sessionId);

    const second = await runCli([
      "--home", home, "--acp", "--json", "--acp-url", wsUrl, "--session-id", sessionId,
    ], env);
    assert.equal(second.code, 0, second.stderr);
    const loaded = second.stdout.trim().split(/\r?\n/).map(JSON.parse)
      .find((frame) => frame.method === "session/load");
    assert.ok(loaded?.result, `session/load frame missing: ${second.stdout}`);
  });
});

test("BDD: Given --json, When I run loom --acp, Then every output line is JSON", async () => {
  await withServer(async (wsUrl, env, home) => {
    const result = await runCli(["--home", home, "--acp", "--json", "--acp-url", wsUrl], env);
    assert.equal(result.code, 0, result.stderr);
    for (const line of result.stdout.trim().split(/\r?\n/)) {
      assert.doesNotThrow(() => JSON.parse(line));
    }
  });
});

test("BDD: Given a deterministic ACP server, When I run loom --acp with a prompt, Then the CLI client streams and completes", async () => {
  const fake = await startFakeAcpServer();
  const home = await mkdtemp(path.join(tmpdir(), "loom-cli-acp-prompt-"));
  try {
    const result = await runCli([
      "--home", home, "--acp", "--json", "--acp-url", fake.url, "hello",
    ], process.env);
    assert.equal(result.code, 0, result.stderr);
    const frames = result.stdout.trim().split(/\r?\n/).map(JSON.parse);
    const update = frames.find((frame) => frame.method === "session/update");
    assert.equal(update?.params?.update?.content?.text, "fake response");
    const prompt = frames.find((frame) => frame.method === "session/prompt");
    assert.equal(prompt?.result?.stopReason, "end_turn");
  } finally {
    await fake.close();
    await rm(home, { recursive: true, force: true });
  }
});
