import json
import os
import subprocess
import sys


if len(sys.argv) != 2:
    raise SystemExit("usage: mcp_resource INPUT_ROOT")

root = os.path.abspath(sys.argv[1])
task = os.environ["JET_CORPUS_TASK"]
project = os.path.join(root, "denied" if task == "mcp-environment-denied" else "readonly")
root_uri = "file://" + project


def frame(body):
    return f"Content-Length: {len(body.encode())}\r\n\r\n{body}"


bodies = [
    json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"rootUri": root_uri, "capabilities": {}}}, separators=(",", ":")),
    '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}',
    '{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}',
    '{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"jet://environment"}}',
    '{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"jet://missing"}}',
    '{"jsonrpc":"2.0","id":5,"method":"shutdown","params":{}}',
    '{"jsonrpc":"2.0","method":"exit","params":{}}',
]
payload = "".join(frame(body) for body in bodies)
completed = subprocess.run(
    [os.environ["JET_CORPUS_JET"], "self", "lsp"],
    cwd=project,
    env={**os.environ, "HOME": "/agent-secret"},
    input=payload,
    text=True,
    capture_output=True,
    timeout=10,
    check=False,
)
if completed.returncode != 0:
    raise SystemExit(completed.stderr or "MCP server failed")
response = completed.stdout
if task == "mcp-environment-denied":
    if "-32002" not in response:
        raise SystemExit("MCP denied case did not fail closed")
    if "/agent-secret" in response:
        raise SystemExit("MCP denied case leaked HOME")
    print("mcp=denied\nerror=-32002\nsecret=redacted")
else:
    for marker in ("active_environment", "codex-agent", "mcp.read", "generated.txt", "lint"):
        if marker not in response:
            raise SystemExit(f"MCP resource omitted {marker}")
    if "/agent-secret" in response:
        raise SystemExit("MCP resource leaked HOME")
    print("mcp=readonly\nresource=jet://environment\ngrant=mcp.read\nside-effects=none")
