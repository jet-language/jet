"""The hostile peer: hand over an unsealed memfd, then truncate it."""
import os
import socket
import sys

stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
stream.connect(sys.argv[1])
fd = os.memfd_create("jet-hostile-peer", os.MFD_CLOEXEC)
os.write(fd, b"x" * 65536)
socket.send_fds(stream, [b"m"], [fd])
assert stream.recv(1) == b"\x01"
os.ftruncate(fd, 0)
stream.sendall(b"t")
stream.close()
os.close(fd)
