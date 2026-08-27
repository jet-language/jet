#!/usr/bin/env python3
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import urlsplit


ITEMS = [
    {"id": 1, "name": "alpha", "qty": 3},
    {"id": 2, "name": "beta", "qty": 7},
    {"id": 3, "name": "gamma", "qty": 2},
    {"id": 4, "name": "delta", "qty": 5},
    {"id": 5, "name": "epsilon", "qty": 4},
    {"id": 6, "name": "zeta", "qty": 6},
]


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        path = urlsplit(self.path).path
        if path == "/items":
            status = 200
            payload = {"items": ITEMS}
        elif path.startswith("/items/"):
            try:
                item_id = int(path.rsplit("/", 1)[1])
            except ValueError:
                item_id = -1
            item = next((item for item in ITEMS if item["id"] == item_id), None)
            status = 200 if item is not None else 404
            payload = item if item is not None else {"error": "not found"}
        else:
            status = 404
            payload = {"error": "not found"}

        body = json.dumps(payload, separators=(",", ":")).encode("ascii")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format, *_args):
        pass


def main():
    server = HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
