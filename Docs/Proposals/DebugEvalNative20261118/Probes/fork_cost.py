"""Probe N6: what one fork per hit costs, by the target's resident size. The
process touches `size` MiB of private anonymous memory, then forks a child
that exits at once, 20 times; the median wall time of fork + waitpid is the
floor of a fork-per-hit condition (the eval call itself is extra). It also
prints how much of the process's anonymous memory the kernel backed with
transparent huge pages, since a fork copies one entry per huge page instead
of 512. Usage: python3 fork_cost.py <MiB>..."""
import mmap
import os
import statistics
import sys
import time


def huge_kib():
    for line in open("/proc/self/smaps_rollup"):
        if line.startswith("AnonHugePages:"):
            return int(line.split()[1])
    return 0


for size in (int(arg) for arg in sys.argv[1:]):
    block = mmap.mmap(-1, size << 20, flags=mmap.MAP_PRIVATE | mmap.MAP_ANONYMOUS)
    for offset in range(0, size << 20, 4096):
        block[offset] = 1
    huge = huge_kib() >> 10
    times = []
    for _ in range(20):
        start = time.perf_counter_ns()
        pid = os.fork()
        if pid == 0:
            os._exit(0)
        os.waitpid(pid, 0)
        times.append(time.perf_counter_ns() - start)
    print("{} MiB resident ({} MiB in huge pages): fork + exit + wait median {:.2f} ms, min {:.2f} ms".format(size, huge, statistics.median(times) / 1e6, min(times) / 1e6))
    block.close()
