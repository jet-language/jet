// The stand-in compiler of the hot-text entry: a static program without libc
// whose functions come before the stand-in runtime pack's (pack.c) in its
// image, as a built jet's do. Each command it is run with enters a different
// set of its functions, in a different order; f_unused is never entered.
void rt_enter(void);
void rt_unused(void);

#define STEP(name) __attribute__((noinline)) void name(void) { __asm__ volatile("" ::: "memory"); }
STEP(f_parse)
STEP(f_check)
STEP(f_lower)
STEP(f_emit)
STEP(f_link)
STEP(f_unused)

int main(int argc, char **argv) {
    char first = argc > 1 ? argv[1][0] : 0;
    if (first == 's') { f_parse(); f_check(); f_lower(); }
    if (first == 'r') { f_parse(); f_emit(); }
    if (first == 'c') { f_check(); f_parse(); }
    if (first == 'b') { f_link(); f_emit(); f_parse(); }
    rt_enter();
    return 0;
}

__asm__(".globl _start\n_start:\n mov (%rsp), %rdi\n lea 8(%rsp), %rsi\n and $-16, %rsp\n call main\n mov %eax, %edi\n mov $60, %eax\n syscall\n");
