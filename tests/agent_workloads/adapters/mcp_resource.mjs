import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

if (process.argv.length !== 3) {
  throw new Error("usage: mcp_resource INPUT_ROOT");
}

const root = resolve(process.argv[2]);
const task = process.env.JET_CORPUS_TASK;
const project = `${root}/${task === "mcp-environment-denied" ? "denied" : "readonly"}`;
const rootUri = `file://${project}`;
function frame(body) {
  return `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`;
}
const payload = [
  JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { rootUri, capabilities: {} } }),
  '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}',
  '{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}',
  '{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"jet://environment"}}',
  '{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"jet://missing"}}',
  '{"jsonrpc":"2.0","id":5,"method":"shutdown","params":{}}',
  '{"jsonrpc":"2.0","method":"exit","params":{}}',
].map(frame).join("");
const result = spawnSync(process.env.JET_CORPUS_JET, ["self", "lsp"], {
  cwd: project,
  env: { ...process.env, HOME: "/agent-secret" },
  input: payload,
  encoding: "utf8",
  timeout: 10000,
});
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(result.stderr || "MCP server failed");
const response = result.stdout;
if (task === "mcp-environment-denied") {
  if (!response.includes("-32002")) throw new Error("MCP denied case did not fail closed");
  if (response.includes("/agent-secret")) throw new Error("MCP denied case leaked HOME");
  console.log("mcp=denied\nerror=-32002\nsecret=redacted");
} else {
  for (const marker of ["active_environment", "codex-agent", "mcp.read", "generated.txt", "lint"]) {
    if (!response.includes(marker)) throw new Error(`MCP resource omitted ${marker}`);
  }
  if (response.includes("/agent-secret")) throw new Error("MCP resource leaked HOME");
  console.log("mcp=readonly\nresource=jet://environment\ngrant=mcp.read\nside-effects=none");
}
