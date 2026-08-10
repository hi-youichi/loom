import crypto from "node:crypto";
import net from "node:net";
import { once } from "node:events";

function frame(text) {
  const payload = Buffer.from(text);
  if (payload.length < 126) {
    return Buffer.concat([Buffer.from([0x81, payload.length]), payload]);
  }
  const header = Buffer.alloc(4);
  header[0] = 0x81;
  header[1] = 126;
  header.writeUInt16BE(payload.length, 2);
  return Buffer.concat([header, payload]);
}

function decode(buffer) {
  if (buffer.length < 2) return null;
  const first = buffer[0];
  const second = buffer[1];
  let offset = 2;
  let length = second & 0x7f;
  if (length === 126) {
    if (buffer.length < 4) return null;
    length = buffer.readUInt16BE(2);
    offset = 4;
  } else if (length === 127) {
    if (buffer.length < 10) return null;
    const lengthBig = buffer.readBigUInt64BE(2);
    if (lengthBig > BigInt(Number.MAX_SAFE_INTEGER)) throw new Error("frame too large");
    length = Number(lengthBig);
    offset = 10;
  }
  const masked = (second & 0x80) !== 0;
  if (masked) offset += 4;
  if (buffer.length < offset + length) return null;

  const mask = masked ? buffer.subarray(offset - 4, offset) : null;
  const body = Buffer.from(buffer.subarray(offset, offset + length));
  if (mask) {
    for (let i = 0; i < body.length; i += 1) body[i] ^= mask[i % 4];
  }
  return {
    consumed: offset + length,
    opcode: first & 0x0f,
    text: body.toString("utf8"),
  };
}

export async function startFakeAcpServer() {
  const server = net.createServer((socket) => {
    let buffer = Buffer.alloc(0);
    let upgraded = false;
    let sessionId = "fake-session-1";

    socket.on("data", (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      if (!upgraded) {
        const end = buffer.indexOf("\r\n\r\n");
        if (end < 0) return;
        const header = buffer.subarray(0, end).toString("utf8");
        const key = header.match(/Sec-WebSocket-Key:\s*([^\r\n]+)/i)?.[1]?.trim();
        if (!key) {
          socket.destroy();
          return;
        }
        const accept = crypto
          .createHash("sha1")
          .update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
          .digest("base64");
        socket.write(
          `HTTP/1.1 101 Switching Protocols\r\n` +
          `Upgrade: websocket\r\nConnection: Upgrade\r\n` +
          `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
        );
        buffer = buffer.subarray(end + 4);
        upgraded = true;
      }

      while (upgraded) {
        const message = decode(buffer);
        if (!message) return;
        buffer = buffer.subarray(message.consumed);
        if (message.opcode !== 0x1) continue;
        const request = JSON.parse(message.text);
        let response;
        if (request.method === "initialize") {
          response = {
            jsonrpc: "2.0",
            id: request.id,
            result: {
              protocolVersion: 1,
              agentCapabilities: {},
            },
          };
        } else if (request.method === "session/new") {
          response = {
            jsonrpc: "2.0",
            id: request.id,
            result: { sessionId },
          };
        } else if (request.method === "session/load") {
          sessionId = request.params.sessionId;
          response = {
            jsonrpc: "2.0",
            id: request.id,
            result: {},
          };
        } else if (request.method === "session/prompt") {
          socket.write(frame(JSON.stringify({
            jsonrpc: "2.0",
            method: "session/update",
            params: {
              sessionId,
              update: {
                sessionUpdate: "agent_message_chunk",
                content: { type: "text", text: "fake response" },
              },
            },
          })));
          response = {
            jsonrpc: "2.0",
            id: request.id,
            result: { stopReason: "end_turn" },
          };
        } else {
          response = {
            jsonrpc: "2.0",
            id: request.id,
            error: { code: -32601, message: "Method not found" },
          };
        }
        socket.write(frame(JSON.stringify(response)));
      }
    });
  });

  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const { port } = server.address();
  return {
    url: `ws://127.0.0.1:${port}/acp`,
    async close() {
      server.close();
      await once(server, "close");
    },
  };
}
