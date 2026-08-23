#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || { printf '%s\n' 'usage: mcp_resource INPUT_ROOT' >&2; exit 2; }
root=$1
task=${JET_CORPUS_TASK:?missing task identity}
jet=${JET_CORPUS_JET:?missing Jet binary}
if [[ $task == mcp-environment-denied ]]; then
  project="$root/denied"
else
  project="$root/readonly"
fi

frame() {
  local body=$1
  printf 'Content-Length: %d\r\n\r\n%s' "${#body}" "$body"
}

root_uri="file://$project"
payload=""
payload+="$(frame "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"rootUri\":\"$root_uri\",\"capabilities\":{}}}")"
payload+="$(frame '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}')"
payload+="$(frame '{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}')"
payload+="$(frame '{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"jet://environment"}}')"
payload+="$(frame '{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"jet://missing"}}')"
payload+="$(frame '{"jsonrpc":"2.0","id":5,"method":"shutdown","params":{}}')"
payload+="$(frame '{"jsonrpc":"2.0","method":"exit","params":{}}')"

response=$(printf '%s' "$payload" | HOME=/agent-secret "$jet" self lsp 2>/dev/null)
if [[ $task == mcp-environment-denied ]]; then
  [[ $response == *'-32002'* ]] || { printf '%s\n' 'MCP denied case did not fail closed' >&2; exit 1; }
  [[ $response != *'/agent-secret'* ]] || { printf '%s\n' 'MCP denied case leaked HOME' >&2; exit 1; }
  printf '%s\n' 'mcp=denied' 'error=-32002' 'secret=redacted'
else
  [[ $response == *'active_environment'* ]] || { printf '%s\n' 'MCP resource omitted environment' >&2; exit 1; }
  [[ $response == *'codex-agent'* && $response == *'mcp.read'* ]] || { printf '%s\n' 'MCP grant projection missing' >&2; exit 1; }
  [[ $response == *'generated.txt'* && $response == *'lint'* ]] || { printf '%s\n' 'MCP resource omitted declared facts' >&2; exit 1; }
  [[ $response != *'/agent-secret'* ]] || { printf '%s\n' 'MCP resource leaked HOME' >&2; exit 1; }
  printf '%s\n' 'mcp=readonly' 'resource=jet://environment' 'grant=mcp.read' 'side-effects=none'
fi
