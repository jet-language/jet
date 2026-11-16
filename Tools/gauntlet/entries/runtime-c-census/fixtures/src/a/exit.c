extern void _Exit(int) __attribute__((noreturn)); void exit(int c) { _Exit(c); }
