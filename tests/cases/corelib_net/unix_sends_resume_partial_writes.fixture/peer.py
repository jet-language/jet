"""The slow peer: a small receive buffer, read a page at a time."""
import array
import os
import socket
import sys
import time

size = int(sys.argv[2])
stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
stream.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4096)
stream.connect(sys.argv[1])
received = bytearray()
descriptors = []
carried = []
reads = 0
space = socket.CMSG_SPACE(253 * array.array("i").itemsize)
while len(received) < size:
    data, ancillary, _, _ = stream.recvmsg(4096, space)
    if not data:
        break
    for level, kind, payload in ancillary:
        if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
            fds = array.array("i")
            fds.frombytes(payload[: len(payload) - len(payload) % fds.itemsize])
            descriptors.extend(fds)
            carried.append(len(received))
    received += data
    reads += 1
    if reads % 16 == 0:
        time.sleep(0.001)
intact = all(byte == index % 251 for index, byte in enumerate(received))
for fd in descriptors:
    os.close(fd)
where = ", ".join(f"byte {at}" for at in carried)
pieces = "many reads" if reads > 1 else "one read"
stream.sendall(f"{len(received)} bytes in {pieces}, in order {str(intact).lower()}, {len(descriptors)} descriptors at {where}\n".encode())
stream.close()
