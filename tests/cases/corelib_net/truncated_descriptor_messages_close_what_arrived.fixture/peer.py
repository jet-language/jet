"""The hostile peer: leave room for two descriptors, then send eight."""
import os
import resource
import socket
import sys

stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
# A socket path longer than sockaddr_un holds connects through its
# directory, as Jet's own unix_connect does.
os.chdir(os.path.dirname(sys.argv[1]))
stream.connect(os.path.basename(sys.argv[1]))
assert stream.recv(1) == b"\x01"
jet = os.getppid()
highest = max(int(name) for name in os.listdir(f"/proc/{jet}/fd"))
soft, hard = resource.prlimit(jet, resource.RLIMIT_NOFILE)
resource.prlimit(jet, resource.RLIMIT_NOFILE, (highest + 3, hard))
nulls = [os.open("/dev/null", os.O_RDONLY | os.O_CLOEXEC) for _ in range(8)]
socket.send_fds(stream, [b"x"], nulls)
assert stream.recv(1) == b"\x02"
resource.prlimit(jet, resource.RLIMIT_NOFILE, (soft, hard))
socket.send_fds(stream, [b"y"], nulls[:1])
stream.close()
for fd in nulls:
    os.close(fd)
