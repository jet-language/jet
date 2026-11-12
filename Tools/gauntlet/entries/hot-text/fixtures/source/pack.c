// The stand-in runtime pack of the hot-text entry: its functions mark where
// the stand-in compiler's own functions end.
__attribute__((noinline)) void rt_enter(void) { __asm__ volatile("" ::: "memory"); }
__attribute__((noinline)) void rt_unused(void) { __asm__ volatile("" ::: "memory"); }
