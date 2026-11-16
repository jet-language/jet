// Link this caller with the complete Jet-only runtime and CompilerRoutesTests.jet.
// Usage: compiler-routes-tests DISK_BACKED_SCRATCH_DIRECTORY
#define _GNU_SOURCE
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <unistd.h>
typedef int64_t W;
typedef struct { unsigned char *data; W len, cap; } Text;
extern W jet_rt_compiler_fs_fixture(Text *);
extern W jet_rt_compiler_wait_fixture(W, Text *);
extern W jet_rt_compiler_status_history_fixture(void);
static Text text(char *s) { return (Text){(unsigned char *)s, (W)strlen(s), 0}; }
static void witness(const char *name, W result) {
    if (result) { fprintf(stderr, "%s: assertion %lld\n", name, (long long)result); abort(); }
}
int main(int argc, char **argv) {
    assert(argc == 2);
    assert(strncmp(argv[1], "/tmp", 4) != 0);
    size_t capacity = strlen(argv[1]) + 64;
    char *root = malloc(capacity);
    assert(root);
    snprintf(root, capacity, "%s/jet-compiler-routes-XXXXXX", argv[1]);
    assert(mkdtemp(root));
    Text root_text = text(root);
    witness("filesystem/nonblocking/mapped registry", jet_rt_compiler_fs_fixture(&root_text));
    assert(rmdir(root) == 0);
    free(root);
    pid_t pid = fork();
    assert(pid >= 0);
    if (!pid) _exit(7);
    Text exited = text("exit status: 7");
    witness("waited exit survives child drop", jet_rt_compiler_wait_fixture(pid, &exited));
    pid = fork();
    assert(pid >= 0);
    if (!pid) { for (;;) pause(); }
    assert(kill(pid, SIGTERM) == 0);
    Text signalled = text("signal: 15 (SIGTERM)");
    witness("waited signal", jet_rt_compiler_wait_fixture(pid, &signalled));
    witness("64-record history, newest pid, platform names", jet_rt_compiler_status_history_fixture());
    puts("compiler routes: Jet-only (Rust frozen)");
    return 0;
}
