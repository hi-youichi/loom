// Smoke test: extension dispatch over the anureo ACP WebSocket.
const url = process.env.ANUREO_ACP_URL ?? "ws://127.0.0.1:3031/acp"
const ws = new WebSocket(url)
let nextId = 1
const pending = new Map()

function request(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = nextId++
    pending.set(id, { resolve, reject })
    ws.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }))
  })
}

ws.addEventListener("message", (ev) => {
  const msg = JSON.parse(typeof ev.data === "string" ? ev.data : ev.data.toString())
  if (msg.id !== undefined && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id)
    pending.delete(msg.id)
    if (msg.error) reject(new Error(JSON.stringify(msg.error)))
    else resolve(msg.result)
  }
})
ws.addEventListener("error", (e) => { console.error("WS error", e.message ?? e); process.exit(1) })

ws.addEventListener("open", async () => {
  try {
    const init = await request("initialize", {
      protocolVersion: 1,
      clientCapabilities: { fs: { readTextFile: false, writeTextFile: false } },
    })
    console.log("initialize:", JSON.stringify(init?.agentInfo ?? "?"))

    const home = await request("_anureo.dev/files/home", {})
    console.log("files/home:", JSON.stringify(home))

    const sessions = await request("session/list", {})
    console.log("session/list:", JSON.stringify(sessions)?.slice(0, 300))

    const commands = await request("_anureo.dev/command/list", {})
    console.log("command/list:", JSON.stringify(commands)?.slice(0, 200))

    ws.close()
    process.exit(0)
  } catch (e) {
    console.error("FAIL", e.message)
    process.exit(1)
  }
})
setTimeout(() => { console.error("timeout"); process.exit(1) }, 15000)
