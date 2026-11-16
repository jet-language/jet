# Prints the line the peer on port 47613 answers with.
import socket

with socket.create_connection(("127.0.0.1", 47613)) as conn:
    data = b""
    while not data.endswith(b"\n"):
        part = conn.recv(1024)
        if not part:
            break
        data += part
print(data.decode(), end="")
