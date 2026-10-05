/*
 * Load and run a Jet-backend in-memory image (Image/Memory.jet) the way the
 * Jet loader (Image/Loader.jet) does: map the block read+write, copy it in,
 * bind every runtime import slot to the compiled runtime's C-ABI export of
 * that name, flip the block to read+execute (never writable and executable at
 * once), and run the entry through jet_rt_main. It is the out-of-process C
 * reference the in-process Jet loader is checked against, linked against the
 * compiled runtime by check-native-fixtures.mjs:
 *
 *     cc load-memory-image.c libjet_runtime_c.a -rdynamic -Wl,-u,<symbol>... \
 *        -lpthread -ldl -lm -o load-memory-image
 *
 * usage: load-memory-image <name>.image <name>.mem
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

extern int32_t jet_rt_main(void (*entry)(void));

static int fail(const char *what, const char *detail) {
    fprintf(stderr, "load-memory-image: %s %s\n", what, detail);
    return 71;
}

int main(int argc, char **argv) {
    if (argc != 3) return fail("usage:", "load-memory-image <name>.image <name>.mem");

    FILE *image = fopen(argv[1], "rb");
    if (!image) return fail("cannot open", argv[1]);
    fseek(image, 0, SEEK_END);
    long length = ftell(image);
    fseek(image, 0, SEEK_SET);
    long page = sysconf(_SC_PAGESIZE);
    size_t size = (size_t)((length + page - 1) / page * page);
    if (size == 0) return fail("empty image", argv[1]);

    FILE *manifest = fopen(argv[2], "r");
    if (!manifest) return fail("cannot open", argv[2]);
    /* The manifest starts with the origin alignment the image's data needs:
     * over-map by it, then trim whole pages on both sides. */
    char alignment_word[16];
    long alignment;
    if (fscanf(manifest, "%15s %ld", alignment_word, &alignment) != 2 || strcmp(alignment_word, "align") != 0 || alignment < 16 || (alignment & (alignment - 1)) != 0)
        return fail("invalid image alignment in", argv[2]);
    size_t origin_align = (size_t)alignment > (size_t)page ? (size_t)alignment : (size_t)page;
    size_t reserved_size = size + origin_align - (size_t)page;
    unsigned char *mapping = mmap(NULL, reserved_size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (mapping == MAP_FAILED) return fail("mmap failed for", argv[1]);
    size_t prefix = (origin_align - (uintptr_t)mapping % origin_align) % origin_align;
    unsigned char *base = mapping + prefix;
    size_t suffix = reserved_size - prefix - size;
    if (prefix && munmap(mapping, prefix) != 0) return fail("munmap prefix failed for", argv[1]);
    if (suffix && munmap(base + size, suffix) != 0) return fail("munmap suffix failed for", argv[1]);
    if (fread(base, 1, (size_t)length, image) != (size_t)length) return fail("cannot read", argv[1]);
    fclose(image);

    long entry = -1;
    char word[16];
    long at;
    while (fscanf(manifest, "%15s %ld", word, &at) == 2) {
        if (strcmp(word, "entry") == 0) {
            if (at < 0 || at >= length) return fail("entry outside the image in", argv[2]);
            entry = at;
        } else if (strcmp(word, "import") == 0) {
            char symbol[256];
            if (fscanf(manifest, "%255s", symbol) != 1) return fail("import without a symbol in", argv[2]);
            if (at < 0 || at + 8 > length) return fail("import slot outside the image in", argv[2]);
            void *address = dlsym(RTLD_DEFAULT, symbol);
            if (!address) return fail("unresolved runtime symbol", symbol);
            memcpy(base + at, &address, sizeof address);
        } else if (strcmp(word, "reloc") == 0) {
            long target;
            if (fscanf(manifest, "%ld", &target) != 1) return fail("reloc without a target in", argv[2]);
            if (at < 0 || at > length - 8 || at % 8 != 0 || target < 0 || target >= length) return fail("data relocation outside the image in", argv[2]);
            void *address = base + target;
            memcpy(base + at, &address, sizeof address);
        } else {
            return fail("unknown manifest line in", argv[2]);
        }
    }
    fclose(manifest);
    if (entry < 0) return fail("no entry in", argv[2]);

    if (mprotect(base, size, PROT_READ | PROT_EXEC) != 0) return fail("mprotect failed for", argv[1]);
    void (*run)(void) = (void (*)(void))(base + entry);
    return jet_rt_main(run);
}
