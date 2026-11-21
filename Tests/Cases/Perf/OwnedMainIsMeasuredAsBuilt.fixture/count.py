# Runs the program argv[1] names with its stdout on a pipe and drains the
# pipe. Once the program has exited, and before it is reaped, prints the
# write calls it made, /proc/PID/io's syscw, as `syscw N`.
import os
import subprocess
import sys

child = subprocess.Popen([sys.argv[1]], stdout=subprocess.PIPE)
child.stdout.read()
os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOWAIT)
with open(f"/proc/{child.pid}/io") as io:
    writes = [line.split()[1] for line in io if line.startswith("syscw:")][0]
child.wait()
print(f"syscw {writes}")
