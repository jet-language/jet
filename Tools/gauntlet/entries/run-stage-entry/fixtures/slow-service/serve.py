# A line service on port argv[1]: answers `ready` with `ready`, `ping` with
# `pong` and `quit` with `bye`, and exits after `quit`. Each answer waits
# 2 ms first, so no latency it shows is under a millisecond and the
# comparison never takes the sub-millisecond evidence branch, whose output
# would depend on how loaded the machine is.
import socket
import sys
import time

ANSWERS = {"ready\n": "ready\n", "ping\n": "pong\n", "quit\n": "bye\n"}

server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", int(sys.argv[1])))
server.listen(16)
while True:
    conn, _ = server.accept()
    with conn:
        data = b""
        while not data.endswith(b"\n"):
            part = conn.recv(1024)
            if not part:
                break
            data += part
        line = data.decode()
        time.sleep(0.002)
        conn.sendall(ANSWERS.get(line, "?\n").encode())
    if line == "quit\n":
        break
server.close()
