import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
from urllib.error import HTTPError
from urllib.request import Request, urlopen


if len(sys.argv) != 2:
    raise SystemExit("usage: http_api INPUT_FILE")


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


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
thread = Thread(target=server.handle_request)
thread.start()
try:
    request = Request(
        f"http://127.0.0.1:{server.server_port}/agent",
        data=Path(sys.argv[1]).read_bytes(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urlopen(request) as response:
            status, body = response.status, response.read().decode()
    except HTTPError as error:
        status, body = error.code, error.read().decode()
    print(f"{status}|{body}")
finally:
    thread.join(timeout=2)
    server.server_close()
