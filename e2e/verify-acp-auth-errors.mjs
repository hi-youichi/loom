const url = process.argv[2];
const run = (payload) => new Promise((resolve, reject) => {
  const ws = new WebSocket(url);
  const t = setTimeout(() => { ws.close(); reject(new Error('timeout')); }, 8000);
  ws.onopen = () => ws.send(JSON.stringify(payload));
  ws.onmessage = (ev) => { clearTimeout(t); console.log(JSON.parse(ev.data.toString())); ws.close(); resolve(); };
  ws.onerror = () => { clearTimeout(t); reject(new Error('ws error')); };
});
await run({ jsonrpc: '2.0', id: 1, method: '_anureo.dev/auth/status', params: {} });
await run({ jsonrpc: '2.0', id: 2, method: '_anureo.dev/auth/login', params: { password: 'wrong-pw' } });
await run({ jsonrpc: '2.0', id: 3, method: '_anureo.dev/auth/authenticate', params: { sessionToken: 'garbage.token' } });
await run({ jsonrpc: '2.0', id: 4, method: 'initialize', params: {} });
