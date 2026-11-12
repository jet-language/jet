# The peer the programs talk to: answers each connection to port argv[1]
# with one line, until it is stopped.
import socket
import sys

server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", int(sys.argv[1])))
server.listen(16)
while True:
    conn, _ = server.accept()
    with conn:
        conn.sendall(b"peer says hi\n")
