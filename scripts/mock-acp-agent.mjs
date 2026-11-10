// Minimal ACP v1 agent used by crates/acp/tests/harness.rs.
// Speaks newline-delimited JSON-RPC over stdio.
// Env: MOCK_PERMISSION=1 makes it request permission during the prompt turn.
import { createInterface } from "node:readline";

const wantPermission = process.env.MOCK_PERMISSION === "1";
let sessionId = "mock-session-1";
let sawSystemPrompt = false;
let mcpServerCount = 0;
let nextId = 1000;
const pending = new Map();

function send(msg) {
  process.stdout.write(JSON.stringify(msg) + "\n");
}
function respond(id, result) {
  send({ jsonrpc: "2.0", id, result });
}
function notify(method, params) {
  send({ jsonrpc: "2.0", method, params });
}
function request(method, params) {
  const id = nextId++;
  return new Promise((resolve) => {
    pending.set(id, resolve);
    send({ jsonrpc: "2.0", id, method, params });
  });
}
function chunk(text) {
  notify("session/update", {
    sessionId,
    update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text } },
  });
}

async function handlePrompt(id) {
  chunk(`mock agent online; systemPrompt=${sawSystemPrompt}; mcpServers=${mcpServerCount}`);
  notify("session/update", {
    sessionId,
    update: {
      sessionUpdate: "tool_call",
      toolCallId: "tc-1",
      title: "Write mock output",
      kind: "edit",
      status: "pending",
    },
  });
  await request("fs/write_text_file", {
    sessionId,
    path: "mock-output.txt",
    content: "hello from mock agent\n",
  });
  if (wantPermission) {
    const answer = await request("session/request_permission", {
      sessionId,
      toolCall: { toolCallId: "tc-perm", title: "Run mock tests" },
      options: [
        { optionId: "opt-allow", name: "Allow once", kind: "allow_once" },
        { optionId: "opt-reject", name: "Reject once", kind: "reject_once" },
      ],
    });
    chunk(`permission outcome: ${JSON.stringify(answer.result?.outcome ?? answer.error)}`);
  }
  respond(id, { stopReason: "end_turn" });
}

const rl = createInterface({ input: process.stdin });
rl.on("line", (line) => {
  if (!line.trim()) return;
  const msg = JSON.parse(line);
  if (msg.id !== undefined && pending.has(msg.id)) {
    const resolve = pending.get(msg.id);
    pending.delete(msg.id);
    resolve(msg);
    return;
  }
  switch (msg.method) {
    case "initialize":
      respond(msg.id, {
        protocolVersion: 1,
        agentCapabilities: { sessionCapabilities: { systemPrompt: {} } },
        agentInfo: { name: "mock-acp-agent", version: "0.1.0" },
      });
      break;
    case "session/new":
      sawSystemPrompt = !!msg.params?.systemPrompt;
      mcpServerCount = msg.params?.mcpServers?.length ?? 0;
      respond(msg.id, { sessionId });
      break;
    case "session/prompt":
      handlePrompt(msg.id).catch((e) =>
        send({ jsonrpc: "2.0", id: msg.id, error: { code: -32603, message: String(e) } }),
      );
      break;
    case "session/cancel":
      break;
    default:
      if (msg.id !== undefined) {
        send({ jsonrpc: "2.0", id: msg.id, error: { code: -32601, message: `unknown method ${msg.method}` } });
      }
  }
});