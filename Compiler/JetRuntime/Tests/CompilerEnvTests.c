// Link with the complete Jet-only runtime + CompilerEnvTests.jet. Run once
// without arguments and once with "empty" (fresh processes for startup capture).
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef int64_t W;
typedef struct { W len, cap; void *data; } List;
extern void jet_rt_host_arguments(W, char **, char **);
extern W jet_rt_compiler_env_fixture(const List *, const List *, const List *);
extern W jet_rt_compiler_env_empty_fixture(void);
extern W jet_rt_env_key_compare(const List *, const List *);
static List bytes(const void *p, size_t n) { return (List){(W)n, (W)n, (void *)p}; }
static List text(const char *p) { return bytes(p, strlen(p)); }
static void compare(const void *a, size_t an, const void *b, size_t bn, W order) {
    List left = bytes(a, an), right = bytes(b, bn);
    assert(jet_rt_env_key_compare(&left, &right) == order);
    assert(jet_rt_env_key_compare(&right, &left) == -order);
}
static void comparison_witness(void) {
    compare(NULL, 0, NULL, 0, 0);
    compare(NULL, 0, "a", 1, -1);
    compare("a", 1, "aa", 2, -1);
    compare("aa", 2, "ab", 2, -1);
    compare("PATH", 4, "path", 4, -1); // Unix case-sensitive; not Windows fold.
    compare("\x7f", 1, "\x80", 1, -1); // unsigned, not signed char ordering.
    compare("\xff", 1, "\xc3\xbf", 2, 1); // invalid UTF-8 is NOT U+00FF.
    compare("\xc3\xa9", 2, "e\xcc\x81", 3, 1); // no normalization.
    compare("a\0b", 3, "a\0c", 3, -1); // length-delimited, not strcmp.
    compare("\xff\0\x80", 3, "\xff\0\x80", 3, 0);
}
int main(int argc, char **argv) {
    comparison_witness();
    if (argc == 2) {
        assert(strcmp(argv[1], "empty") == 0);
        char *env[] = {NULL};
        jet_rt_host_arguments(argc, argv, env);
        assert(jet_rt_compiler_env_empty_fixture() == 0);
    } else {
        assert(argc == 1);
        char first[] = "Z=first";
        char *env[] = {first, "A=old", "\xff=v\xfe", "EMPTY=", "A=last",
                       "a=lower", "EQ=x=y", "=hidden=drive", "malformed", "", NULL};
        jet_rt_host_arguments(argc, argv, env);
        memset(first, 'x', sizeof first - 1); // capture must own its bytes.
        List startup_rows[] = {text("Z"), text("first"), text("A"), text("old"),
            text("\xff"), text("v\xfe"), text("EMPTY"), bytes(NULL, 0),
            text("A"), text("last"), text("a"), text("lower"),
            text("EQ"), text("x=y"), text("=hidden"), text("drive")};
        List logical_rows[] = {text("Z"), text("first"), text("\xff"), text("v\xfe"),
            text("EMPTY"), bytes(NULL, 0), text("A"), text("last"),
            text("a"), text("lower"), text("EQ"), text("x=y"),
            text("=hidden"), text("drive")};
        List changed_rows[] = {text("\xff"), text("v\xfe"), text("EMPTY"), bytes(NULL, 0),
            text("a"), text("lower"), text("EQ"), text("x=y"),
            text("=hidden"), text("drive"), text("Z"), text("changed")};
        List startup = bytes(startup_rows, 16), logical = bytes(logical_rows, 14),
             changed = bytes(changed_rows, 12);
        W result = jet_rt_compiler_env_fixture(&startup, &logical, &changed);
        if (result) { fprintf(stderr, "environment: assertion %lld\n", (long long)result); abort(); }
    }
    puts("compiler environment routes: Jet-only (Rust frozen)");
    return 0;
}
