/* SETUP-PORT of tests/debug.rs:824-870; authorizer asserts nothing. */
#include <sys/prctl.h>
#include <unistd.h>
#include <stdio.h>
#include <stdint.h>
int main(int argc, char **argv) {
    if (argc < 2) return 126;
    if (prctl(0x59616d61, (unsigned long)-1, 0, 0, 0) != 0) return 125;
    execl(argv[1], argv[1], (char *)0);
    perror("exec");
    return 126;
}
