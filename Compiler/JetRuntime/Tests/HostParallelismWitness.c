// HostParallelism.jet against fake procfs/sysfs trees. Expected values follow
// Rust std 1.96 sys/thread/unix.rs available_parallelism + cgroups::quota.
#define _GNU_SOURCE
#include <errno.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

long long jet_rt_host_cpu_quota_fixture(const char *root, long long root_len);
long long jet_rt_host_cpu_online_fixture(const char *root, long long root_len);
long long jet_rt_host_cpu_parallelism_fixture(const char *root, long long root_len);

static const long long unlimited = 9223372036854775807LL;
static char base[256];
static int failures;

static void put(const char *root, const char *relative, const char *text) {
    char path[1024];
    snprintf(path, sizeof path, "%s/%s", root, relative);
    for (char *at = path + strlen(root) + 1; *at; at++) {
        if (*at == '/') {
            *at = 0;
            if (mkdir(path, 0700) != 0 && errno != EEXIST) { perror(path); exit(2); }
            *at = '/';
        }
    }
    if (text == NULL) {
        if (mkdir(path, 0700) != 0 && errno != EEXIST) { perror(path); exit(2); }
        return;
    }
    FILE *file = fopen(path, "w");
    if (!file) { perror(path); exit(2); }
    fputs(text, file);
    fclose(file);
}

static const char *fresh(const char *name) {
    static char root[512];
    snprintf(root, sizeof root, "%s/%s", base, name);
    if (mkdir(root, 0700) != 0) { perror(root); exit(2); }
    return root;
}

static void expect(const char *name, long long actual, long long expected) {
    if (actual != expected) {
        fprintf(stderr, "%s: got %lld, want %lld\n", name, actual, expected);
        failures++;
    }
}

static long long quota(const char *root) { return jet_rt_host_cpu_quota_fixture(root, (long long)strlen(root)); }

int main(void) {
    snprintf(base, sizeof base, "%s/jet-cpu-XXXXXX", getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp");
    if (!mkdtemp(base)) { perror("mkdtemp"); return 2; }
    const char *root;

    // v2: limits are read from the group and every ancestor; the minimum wins.
    root = fresh("v2");
    put(root, "proc/self/cgroup", "0::/a/b\n");
    put(root, "sys/fs/cgroup/a/b/cgroup.controllers", "cpu\n");
    put(root, "sys/fs/cgroup/a/b/cpu.max", "max 100000\n");
    put(root, "sys/fs/cgroup/a/cpu.max", "250000 100000\n");
    put(root, "sys/fs/cgroup/cpu.max", "900000 100000\n");
    expect("v2 ancestor minimum", quota(root), 2);

    // v2 without cgroup.controllers is not cgroup2: unlimited.
    root = fresh("v2-missing");
    put(root, "proc/self/cgroup", "0::/a\n");
    put(root, "sys/fs/cgroup/a/cpu.max", "100000 100000\n");
    expect("v2 no controllers", quota(root), unlimited);

    // A cpu.max with one field ends the walk (try `?`), keeping prior quota.
    root = fresh("v2-abort");
    put(root, "proc/self/cgroup", "0::/a\n");
    put(root, "sys/fs/cgroup/a/cgroup.controllers", "cpu\n");
    put(root, "sys/fs/cgroup/a/cpu.max", "max\n");
    put(root, "sys/fs/cgroup/cpu.max", "100000 100000\n");
    expect("v2 malformed aborts", quota(root), unlimited);

    // Quota rounds down and may be 0; available_parallelism clamps it to 1.
    root = fresh("v2-zero");
    put(root, "proc/self/cgroup", "0::/\n");
    put(root, "sys/fs/cgroup/cgroup.controllers", "cpu\n");
    put(root, "sys/fs/cgroup/cpu.max", "50000 100000\n");
    expect("v2 root fractional", quota(root), 0);
    expect("clamped parallelism", jet_rt_host_cpu_parallelism_fixture(root, (long long)strlen(root)), 1);

    // v1 controller line trumps a later v2 line; cpu,cpuacct mount guess.
    root = fresh("v1");
    put(root, "proc/self/cgroup", "4:cpu,cpuacct:/g\n0::/\n");
    put(root, "sys/fs/cgroup/cpu,cpuacct/g/cpu.cfs_quota_us", "300000\n");
    put(root, "sys/fs/cgroup/cpu,cpuacct/g/cpu.cfs_period_us", "100000\n");
    put(root, "sys/fs/cgroup/cpu,cpuacct/cpu.cfs_quota_us", "-1\n");
    put(root, "sys/fs/cgroup/cpu,cpuacct/cpu.cfs_period_us", "100000\n");
    expect("v1 cpu,cpuacct", quota(root), 3);

    // v1 bind mount found through mountinfo; the bound root is trimmed.
    root = fresh("v1-mountinfo");
    put(root, "proc/self/cgroup", "3:cpu:/docker/x\n");
    char mountinfo[1024];
    snprintf(mountinfo, sizeof mountinfo, "22 1 0:20 / /proc rw - proc proc rw\n35 25 0:30 /docker /mnt/cg rw,nosuid - cgroup cgroup rw,cpu,cpuacct\n");
    put(root, "proc/self/mountinfo", mountinfo);
    put(root, "mnt/cg/x/cpu.cfs_quota_us", "150000\n");
    put(root, "mnt/cg/x/cpu.cfs_period_us", "100000\n");
    expect("v1 mountinfo", quota(root), 1);

    // A malformed mountinfo line (fewer than eight fields) aborts the scan.
    root = fresh("v1-short");
    put(root, "proc/self/cgroup", "3:cpu:/x\n");
    put(root, "proc/self/mountinfo", "1 2 3\n35 25 0:30 / /mnt/cg rw - cgroup cgroup rw,cpu\n");
    put(root, "mnt/cg/x/cpu.cfs_quota_us", "100000\n");
    put(root, "mnt/cg/x/cpu.cfs_period_us", "100000\n");
    expect("v1 short mountinfo", quota(root), unlimited);

    // No cpu controller and no v2 line, or no cgroup file at all.
    root = fresh("none");
    put(root, "proc/self/cgroup", "2:memory:/m\n");
    expect("no cpu controller", quota(root), unlimited);
    root = fresh("absent");
    expect("no cgroup file", quota(root), unlimited);

    // glibc _SC_NPROCESSORS_ONLN online list.
    root = fresh("online");
    put(root, "sys/devices/system/cpu/online", "0-3,8,10-11\n");
    expect("online list", jet_rt_host_cpu_online_fixture(root, (long long)strlen(root)), 7);

    // Production probe: the real affinity count under the real cgroup quota.
    cpu_set_t set;
    CPU_ZERO(&set);
    if (sched_getaffinity(0, sizeof set, &set) != 0) { perror("sched_getaffinity"); return 2; }
    long long real = jet_rt_host_cpu_parallelism_fixture("", 0);
    long long real_quota = jet_rt_host_cpu_quota_fixture("", 0);
    long long expected = CPU_COUNT(&set);
    if (real_quota < 1) real_quota = 1;
    if (real_quota < expected) expected = real_quota;
    expect("real probe", real, expected);

    if (failures) return 1;
    return 0;
}
