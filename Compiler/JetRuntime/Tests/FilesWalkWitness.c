// Link this caller with the complete Jet-only runtime and FilesWalkTests.jet.
// Usage: files-walk-witness (TMPDIR names a disk-backed scratch directory)
#define _GNU_SOURCE
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef int64_t W;
typedef struct { unsigned char *data; W len, cap; } Text;
extern W jet_rt_files_ignore_fixture(void);
extern W jet_rt_files_walk_fixture(Text *);
static void witness(const char *name, W result) {
    if (result) { fprintf(stderr, "%s: assertion %lld\n", name, (long long)result); abort(); }
}
int main(void) {
    const char *scratch = getenv("TMPDIR");
    assert(scratch && *scratch);
    size_t capacity = strlen(scratch) + 64;
    char *root = malloc(capacity);
    assert(root);
    snprintf(root, capacity, "%s/jet-files-walk-XXXXXX", scratch);
    assert(mkdtemp(root));
    Text root_text = {(unsigned char *)root, (W)strlen(root), 0};
    witness("gitignore matcher and path components", jet_rt_files_ignore_fixture());
    witness("walk slots, ignore rules and registry lifetime", jet_rt_files_walk_fixture(&root_text));
    free(root);
    return 0;
}
