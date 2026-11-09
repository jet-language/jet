"""The hostile peer: resize the shared memory it was handed, then report."""
import errno
import fcntl
import mmap
import os
import socket
import sys

F_GET_SEALS = 1034
F_SEAL_SHRINK = 2
F_SEAL_GROW = 4

stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
stream.connect(sys.argv[1])
_, fds, _, _ = socket.recv_fds(stream, 16, 4)
fd = fds[0]


def attempt(size):
    try:
        os.ftruncate(fd, size)
        return "allowed"
    except OSError as error:
        return errno.errorcode[error.errno]


shrink = attempt(0)
grow = attempt(8192)
seals = fcntl.fcntl(fd, F_GET_SEALS)
held = "+".join(name for bit, name in ((F_SEAL_SHRINK, "shrink"), (F_SEAL_GROW, "grow")) if seals & bit)
size = os.fstat(fd).st_size
with mmap.mmap(fd, size) as view:
    view[0:3] = b"JET"
stream.sendall(f"shrink {shrink}, grow {grow}, seals {held}, size {size}\n".encode())
stream.close()
