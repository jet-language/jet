#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || {
  printf '%s\n' 'usage: http_api INPUT_FILE' >&2
  exit 2
}

port_file=.http_api_port
body_file=.http_api_body
server_pid=
cleanup() {
  if [[ -n $server_pid ]]; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -f -- "$port_file" "$body_file"
}
trap cleanup EXIT

python3 - "$port_file" <<'PY' &
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        try:
            payload = json.loads(self.rfile.read(length))
        except (TypeError, ValueError, json.JSONDecodeError):
            status, body = 400, "invalid-json"
        else:
            if self.path != "/agent":
                status, body = 404, "not-found"
            elif payload.get("name") == "":
                status, body = 422, "invalid-name"
            else:
                status, body = 200, f"accepted|{payload['name']}|{payload['role']}"
        encoded = body.encode()
        self.send_response(status)
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def log_message(self, _format, *_args):
        pass


server = HTTPServer(("127.0.0.1", 0), Handler)
Path(sys.argv[1]).write_text(str(server.server_port))
server.handle_request()
server.server_close()
PY
server_pid=$!

for _ in {1..200}; do
  [[ -s $port_file ]] && break
  sleep 0.01
done
port=$(<"$port_file")
status=$(curl --silent --show-error --output "$body_file" --write-out '%{http_code}' \
  -X POST -H 'Content-Type: application/json' --data-binary "@$1" \
  "http://127.0.0.1:$port/agent")
printf '%s|%s\n' "$status" "$(<"$body_file")"
wait "$server_pid"
server_pid=
