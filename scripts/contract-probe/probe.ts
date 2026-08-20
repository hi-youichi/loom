// Contract probe: compare Express HTTP responses vs loom ACP responses for
// migrated domains. Usage:
//   bun scripts/contract-probe/probe.ts --domain config-entity --wd <projectDir>
// Requires: Express dev stack (3101) + loom dev stack (3031) running.
import { parseArgs } from "node:util";

const { values } = parseArgs({
  options: {
    domain: { type: "string", default: "config-entity" },
    express: { type: "string", default: "http://127.0.0.1:3101" },
    wd: { type: "string", default: process.cwd() },
  },
});

type Json = Record<string, unknown>;

function shape(value: unknown, depth = 0): unknown {
  if (Array.isArray(value)) {
    return value.length ? [shape(value[0], depth + 1)] : [];
  }
  if (value !== null && typeof value === "object") {
    if (depth > 4) return "{...}";
    const out: Json = {};
    for (const key of Object.keys(value as Json).sort()) {
      out[key] = shape((value as Json)[key], depth + 1);
    }
    return out;
  }
  return typeof value;
}

function diff(a: unknown, b: unknown, path = "$"): string[] {
  const issues: string[] = [];
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b)) {
      issues.push(`${path}: array vs ${Array.isArray(b) ? "array" : "scalar"}`);
      return issues;
    }
    if (!a.length || !b.length) return issues;
    return diff(a[0], b[0], `${path}[0]`);
  }
  if (a !== null && typeof a === "object" && b !== null && typeof b === "object") {
    const ak = Object.keys(a as Json).sort();
    const bk = Object.keys(b as Json).sort();
    for (const key of ak) {
      if (!bk.includes(key)) issues.push(`${path}.${key}: missing in ACP`);
    }
    for (const key of bk) {
      if (!ak.includes(key)) issues.push(`${path}.${key}: extra in ACP`);
    }
    for (const key of ak.filter((k) => bk.includes(k))) {
      issues.push(...diff((a as Json)[key], (b as Json)[key], `${path}.${key}`));
    }
    return issues;
  }
  const at = a === null ? "null" : typeof a;
  const bt = b === null ? "null" : typeof b;
  if (at !== bt) issues.push(`${path}: express=${at} acp=${bt}`);
  return issues;
}

async function expressFetch(path: string, init?: RequestInit): Promise<Json> {
  const res = await fetch(`${values.express}${path}`, init);
  const body = (await res.json()) as Json;
  return { __status: res.status, ...body };
}

let nextId = 1;
async function acpCall(method: string, params: Json): Promise<Json> {
  // Reuses the FE ACP transport contract: single WS, JSON-RPC, _loomdesk.dev/<domain>.<method>
  const url = "ws://127.0.0.1:3031/acp";
  const ws = new WebSocket(url);
  await new Promise<void>((resolve, reject) => {
    ws.onopen = () => resolve();
    ws.onerror = () => reject(new Error("ws connect failed"));
  });
  const initId = nextId++;
  ws.send(JSON.stringify({
    jsonrpc: "2.0",
    id: initId,
    method: "initialize",
    params: { protocolVersion: 1, clientCapabilities: { fs: { readTextFile: false, writeTextFile: false } }, workingDirectory: values.wd },
  }));
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("initialize timeout")), 5000);
    ws.onmessage = (event) => {
      const msg = JSON.parse(String(event.data));
      if (msg.id === initId) {
        clearTimeout(timer);
        if (msg.error) reject(new Error(`initialize failed: ${msg.error.message}`));
        else resolve();
      }
    };
  });
  const callId = nextId++;
  const reply = await new Promise<Json>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${method} timeout`)), 8000);
    ws.onmessage = (event) => {
      const msg = JSON.parse(String(event.data));
      if (msg.id === callId) {
        clearTimeout(timer);
        resolve(msg);
      }
    };
    ws.onerror = () => reject(new Error("ws error"));
    ws.send(JSON.stringify({ jsonrpc: "2.0", id: callId, method, params }));
  });
  ws.close();
  return reply;
}

const probes: Record<string, () => Promise<void>> = {
  "config-entity": async () => {
    const name = `probe-${Date.now()}`;

    const exSources = await expressFetch(`/api/config/agents/${name}`);
    const acpSources = await acpCall("_loomdesk.dev/config_entity/agents_sources", { name, cwd: values.wd });
    if (acpSources.error) console.log(`acp error: ${JSON.stringify(acpSources.error)}`);
    const acpSourcesBody = (acpSources.result ?? acpSources) as Json;
    report("agents_sources (missing agent)", exSources, acpSourcesBody);

    const exCreated = await expressFetch(`/api/config/agents/${name}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ scope: "user", description: "probe", prompt: "p" }),
    });
    const acpCreated = await acpCall("_loomdesk.dev/config_entity/agents_create", {
      name: `${name}-ac`, cwd: values.wd,
      description: "probe",
      prompt: "p",
    });
    report("agents_create", exCreated, (acpCreated.result ?? acpCreated) as Json);

    const exExpand = await expressFetch(`/api/config/snippets/expand`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ text: "no #snippets here" }),
    });
    const acpExpand = await acpCall("_loomdesk.dev/config_entity/snippets_expand", {
      text: "no #snippets here", cwd: values.wd,
    });
    report("snippets_expand", exExpand, (acpExpand.result ?? acpExpand) as Json);

    // cleanup
    await expressFetch(`/api/config/agents/${name}`, { method: "DELETE" });
    await acpCall("_loomdesk.dev/config_entity/agents_delete", { name: `${name}-ac`, cwd: values.wd });
  },
};

function report(label: string, express: Json, acp: Json) {
  const issues = diff(express, acp);
  console.log(`\n== ${label}`);
  console.log(`express shape: ${JSON.stringify(shape(express))}`);
  console.log(`acp shape:     ${JSON.stringify(shape(acp))}`);
  if (issues.length) {
    console.log("ISSUES:");
    for (const issue of issues) console.log(`  - ${issue}`);
  } else {
    console.log("PASS: shapes compatible");
  }
}

const probe = probes[values.domain];
if (!probe) {
  console.error(`unknown domain: ${values.domain} (available: ${Object.keys(probes).join(", ")})`);
  process.exit(1);
}
await probe();
