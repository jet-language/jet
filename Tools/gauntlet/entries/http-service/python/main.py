import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import urlsplit


class Handler(BaseHTTPRequestHandler):
    values = {}

    def reply(self, status, payload):
        body = json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def key(self):
        path = urlsplit(self.path).path
        return path[4:] if path.startswith("/kv/") else None

    def do_GET(self):
        path = urlsplit(self.path).path
        if path == "/health":
            self.reply(200, {"status": "ok"})
        elif path == "/shutdown":
            self.reply(200, {"bye": True})
            self.server.stopping = True
        elif (key := self.key()) is not None:
            if key in self.values:
                self.reply(200, {"key": key, "value": self.values[key]})
            else:
                self.reply(404, {"error": "not found"})
        else:
            self.reply(404, {"error": "not found"})

    def do_PUT(self):
        key = self.key()
        if key is None:
            self.reply(404, {"error": "not found"})
            return
        length = int(self.headers.get("Content-Length", "0"))
        self.values[key] = self.rfile.read(length).decode()
        self.reply(200, {"stored": key})

    def log_message(self, *_args):
        pass


def main():
    server = HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler)
    server.stopping = False
    while not server.stopping:
        server.handle_request()


if __name__ == "__main__":
    main()
