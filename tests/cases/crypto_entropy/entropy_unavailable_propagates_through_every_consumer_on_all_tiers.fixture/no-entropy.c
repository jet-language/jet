/* SETUP-PORT of tests/crypto_entropy.rs681-742. No assertions, real getrandom failure. */
#include <sys/prctl.h>
#include <unistd.h>
#include <errno.h>
#include <stdio.h>
#include <stdint.h>
struct SockFilter { uint16_t code; uint8_t jt, jf; uint32_t k; };
struct SockFprog { uint16_t len; const struct SockFilter *filter; };
#if defined(__x86_64__)
#define SYS_GETRANDOM 318
#elif defined(__aarch64__)
#define SYS_GETRANDOM 278
#else
#error original host cfg excludes this architecture
#endif
int main(int argc, char **argv) {
    static const struct SockFilter filter[] = {
        {0x20,0,0,0},{0x15,0,1,SYS_GETRANDOM},
        {0x06,0,0,0x00050000|38},{0x06,0,0,0x7fff0000}
    };
    struct SockFprog program = {4,filter};
    if (argc < 2) return 126;
    if (prctl(38,1UL,0UL,0UL,0UL) != 0 ||
        prctl(22,2UL,&program,0UL,0UL) != 0) { perror("prctl"); return 125; }
    execvp(argv[1],argv+1);
    perror("execvp");
    return 126;
}
