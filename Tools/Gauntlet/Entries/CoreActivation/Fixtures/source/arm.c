// The binaries the core-activation entry's stand-in jet builds: static
// programs without libc that print 42 and exit EXIT. BIG adds functions, as an
// arm that calls into a battery adds code.
#ifndef EXIT
#define EXIT 0
#endif

static long sys3(long n, long a, long b, long c) {
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory");
    return r;
}

#ifdef BIG
#define STEP(name) __attribute__((noinline)) void name(void) { __asm__ volatile("" ::: "memory"); }
STEP(battery_open)
STEP(battery_call)
STEP(battery_close)
#endif

void _start(void) {
#ifdef BIG
    battery_open();
    battery_call();
    battery_close();
#endif
    sys3(1, 1, (long)"42\n", 3);
    sys3(60, EXIT, 0, 0);
}
