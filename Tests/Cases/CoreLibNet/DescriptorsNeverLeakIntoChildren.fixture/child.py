"""The child: list any memfd or socket it inherited past stdin, out and err."""
import os

inherited = []
for name in sorted(os.listdir("/proc/self/fd"), key=int):
    if int(name) <= 2:
        continue
    try:
        target = os.readlink(f"/proc/self/fd/{name}")
    except OSError:
        continue
    if target.startswith("/memfd:") or target.startswith("socket:"):
        inherited.append(target)
print(f"inherited memfds and sockets {inherited}")
