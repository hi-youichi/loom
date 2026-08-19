const url = process.argv[2];
const ws = new WebSocket(url);
const t = setTimeout(() => { console.log('TIMEOUT'); process.exit(1); }, 8000);
ws.onopen = () => ws.send('{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"v1","clientInfo":{"name":"verify","version":"1"}}}');
ws.onmessage = (ev) => {
  console.log(ev.data.toString().slice(0, 220));
  clearTimeout(t);
  ws.close();
  process.exit(0);
};
ws.onerror = () => { console.log('WS_ERROR'); clearTimeout(t); process.exit(1); };
