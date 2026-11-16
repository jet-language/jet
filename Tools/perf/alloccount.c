// LD_PRELOAD allocation counter: counts malloc/calloc/realloc/aligned calls and
// tracks peak live bytes via malloc_usable_size. Writes one line at exit to the
// file named by ALLOCCOUNT_OUT: "allocations=N frees=F peak_live_bytes=P total_bytes=T".
#define _GNU_SOURCE
#include <malloc.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <stdatomic.h>

extern void *__libc_malloc(size_t);
extern void *__libc_calloc(size_t, size_t);
extern void *__libc_realloc(void *, size_t);
extern void *__libc_memalign(size_t, size_t);
extern void __libc_free(void *);

static atomic_ullong allocs, frees, live, peak, total;

static void note_alloc(void *p) {
    if (!p) return;
    size_t n = malloc_usable_size(p);
    atomic_fetch_add(&allocs, 1);
    atomic_fetch_add(&total, n);
    unsigned long long now = atomic_fetch_add(&live, n) + n;
    unsigned long long old = atomic_load(&peak);
    while (now > old && !atomic_compare_exchange_weak(&peak, &old, now)) {}
}

static void note_free(void *p) {
    if (!p) return;
    atomic_fetch_add(&frees, 1);
    atomic_fetch_sub(&live, malloc_usable_size(p));
}

void *malloc(size_t n) { void *p = __libc_malloc(n); note_alloc(p); return p; }
void *calloc(size_t a, size_t b) { void *p = __libc_calloc(a, b); note_alloc(p); return p; }
void *realloc(void *q, size_t n) {
    if (q) note_free(q);
    void *p = __libc_realloc(q, n);
    note_alloc(p);
    return p;
}
void free(void *p) { note_free(p); __libc_free(p); }
int posix_memalign(void **out, size_t align, size_t n) {
    void *p = __libc_memalign(align, n);
    if (!p) return 12;
    note_alloc(p);
    *out = p;
    return 0;
}
void *aligned_alloc(size_t align, size_t n) { void *p = __libc_memalign(align, n); note_alloc(p); return p; }
void *memalign(size_t align, size_t n) { void *p = __libc_memalign(align, n); note_alloc(p); return p; }

__attribute__((destructor)) static void report(void) {
    const char *path = getenv("ALLOCCOUNT_OUT");
    if (!path) return;
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return;
    char buf[256];
    int len = snprintf(buf, sizeof buf, "allocations=%llu frees=%llu peak_live_bytes=%llu total_bytes=%llu\n",
                       (unsigned long long)allocs, (unsigned long long)frees,
                       (unsigned long long)peak, (unsigned long long)total);
    if (len > 0) { ssize_t w = write(fd, buf, (size_t)len); (void)w; }
    close(fd);
}
