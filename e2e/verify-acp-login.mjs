const url = process.argv[2];
const ws = new WebSocket(url);
const t = setTimeout(() => { console.log('TIMEOUT'); process.exit(1); }, 8000);
ws.onopen = () => ws.send('{"jsonrpc":"2.0","id":1,"method":"_loomdesk.dev/auth/login","params":{"password":"e2e-test-pw","trustDevice":false}}');
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data.toString());
  console.log('LOGIN:', JSON.stringify(msg).slice(0, 160));
  if (!msg.result?.sessionToken) { clearTimeout(t); process.exit(1); }
  const token = msg.result.sessionToken;
  // Authenticate with the minted token on the same socket, then initialize.
  ws.send(JSON.stringify({ jsonrpc: '2.0', id: 2, method: '_loomdesk.dev/auth/authenticate', params: { sessionToken: token } }));
  ws.onmessage = (ev2) => {
    const m2 = JSON.parse(ev2.data.toString());
    console.log('AUTH:', JSON.stringify(m2).slice(0, 120));
    ws.send(JSON.stringify({ jsonrpc: '2.0', id: 3, method: 'initialize', params: { protocolVersion: 1, clientInfo: { name: 'verify', version: '1' } } }));
    ws.onmessage = (ev3) => {
      const m3 = JSON.parse(ev3.data.toString());
      const ok = m3.result?.protocolVersion ? 'PASS initialize result' : 'FAIL';
      console.log(ok, JSON.stringify(m3).slice(0, 120));
      clearTimeout(t); ws.close(); process.exit(m3.result ? 0 : 1);
    };
  };
};
ws.onerror = () => { console.log('WS_ERROR'); clearTimeout(t); process.exit(1); };
