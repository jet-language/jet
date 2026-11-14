#define _GNU_SOURCE /* memmem, nftw */
#include <fcntl.h>
#include <ftw.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

typedef struct {
    char **items;
    size_t len, cap;
} Paths;

static Paths paths;
static long *counts;
static atomic_size_t next_job;
static const char *needle;
static size_t needle_len;

static int collect(const char *path, const struct stat *info, int type, struct FTW *ftw) {
    (void)ftw;
    size_t len = strlen(path);
    if (type == FTW_F && S_ISREG(info->st_mode) && len >= 4 && strcmp(path + len - 4, ".txt") == 0) {
        if (paths.len == paths.cap) {
            paths.cap = paths.cap ? paths.cap * 2 : 1024;
            paths.items = realloc(paths.items, paths.cap * sizeof *paths.items);
        }
        paths.items[paths.len++] = strdup(path);
    }
    return 0;
}

static int by_path(const void *left, const void *right) {
    return strcmp(*(char *const *)left, *(char *const *)right);
}

/* Non-overlapping needle occurrences, counted line by line so a match never spans a newline. */
static long count_file(const char *path, char **buffer, size_t *capacity) {
    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        perror(path);
        exit(1);
    }
    struct stat info;
    fstat(fd, &info);
    size_t size = (size_t)info.st_size;
    if (size > *capacity) {
        *capacity = size;
        *buffer = realloc(*buffer, size);
    }
    size_t got = 0;
    while (got < size) {
        ssize_t n = read(fd, *buffer + got, size - got);
        if (n <= 0) break;
        got += (size_t)n;
    }
    close(fd);
    long count = 0;
    const char *text = *buffer, *end = text + got;
    while (text < end) {
        const char *newline = memchr(text, '\n', (size_t)(end - text));
        const char *line_end = newline ? newline : end;
        for (const char *at = text; (at = memmem(at, (size_t)(line_end - at), needle, needle_len)); at += needle_len)
            count++;
        text = line_end + 1;
    }
    return count;
}

static void *worker(void *unused) {
    (void)unused;
    char *buffer = NULL;
    size_t capacity = 0;
    for (size_t i; (i = atomic_fetch_add(&next_job, 1)) < paths.len;) counts[i] = count_file(paths.items[i], &buffer, &capacity);
    free(buffer);
    return NULL;
}

int main(int argc, char **argv) {
    const char *root = argc > 1 ? argv[1] : "files";
    needle = argc > 2 ? argv[2] : "needle-7f";
    needle_len = strlen(needle);
    if (needle_len == 0 || nftw(root, collect, 64, FTW_PHYS) != 0) {
        fprintf(stderr, "cannot scan %s\n", root);
        return 1;
    }
    qsort(paths.items, paths.len, sizeof *paths.items, by_path);
    counts = calloc(paths.len ? paths.len : 1, sizeof *counts);

    long cpus = sysconf(_SC_NPROCESSORS_ONLN);
    size_t workers = cpus > 0 ? (size_t)cpus : 1;
    pthread_t *threads = malloc(workers * sizeof *threads);
    for (size_t i = 0; i < workers; i++) pthread_create(&threads[i], NULL, worker, NULL);
    for (size_t i = 0; i < workers; i++) pthread_join(threads[i], NULL);

    size_t matched = 0;
    long total = 0;
    for (size_t i = 0; i < paths.len; i++) {
        if (counts[i] == 0) continue;
        printf("%s:%ld\n", paths.items[i], counts[i]);
        matched++;
        total += counts[i];
    }
    printf("files %zu/%zu total %ld\n", matched, paths.len, total);
    return 0;
}
