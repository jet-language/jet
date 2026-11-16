#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

typedef struct {
    unsigned long number;
    char *name;
} Photo;

/* Order by numeric suffix; equal numbers (IMG_7 and IMG_07) keep name order. */
static int by_number(const void *left, const void *right) {
    const Photo *a = left, *b = right;
    if (a->number != b->number) return a->number < b->number ? -1 : 1;
    return strcmp(a->name, b->name);
}

/* The number of IMG_<digits>.jpeg, or -1 when the name is not one (or overflows u32). */
static long photo_number(const char *name) {
    size_t length = strlen(name);
    if (length < 4 + 1 + 5 || strncmp(name, "IMG_", 4) != 0 || strcmp(name + length - 5, ".jpeg") != 0) return -1;
    unsigned long number = 0;
    for (const char *p = name + 4; p < name + length - 5; p++) {
        if (*p < '0' || *p > '9') return -1;
        number = number * 10 + (unsigned long)(*p - '0');
        if (number > 0xffffffffUL) return -1;
    }
    return (long)number;
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fputs("usage: bulkrename DIR\n", stderr);
        return 2;
    }
    const char *directory = argv[1];
    DIR *dir = opendir(directory);
    if (!dir) {
        perror("cannot list directory");
        return 1;
    }
    size_t capacity = 512, count = 0, files = 0;
    Photo *photos = malloc(capacity * sizeof *photos);
    char path[4096];
    struct dirent *entry;
    while ((entry = readdir(dir))) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;
        snprintf(path, sizeof path, "%s/%s", directory, entry->d_name);
        struct stat info;
        /* stat follows a symlink, so a link to a file counts as a file, as in the other rails. */
        if (stat(path, &info) != 0 || !S_ISREG(info.st_mode)) continue;
        files++;
        long number = photo_number(entry->d_name);
        if (number < 0) continue;
        if (count == capacity) {
            capacity *= 2;
            photos = realloc(photos, capacity * sizeof *photos);
        }
        photos[count].number = (unsigned long)number;
        photos[count].name = strdup(entry->d_name);
        count++;
    }
    closedir(dir);
    qsort(photos, count, sizeof *photos, by_number);

    char target[4096];
    for (size_t i = 0; i < count; i++) {
        char target_name[32];
        snprintf(target_name, sizeof target_name, "photo-%04zu.jpg", i + 1);
        snprintf(path, sizeof path, "%s/%s", directory, photos[i].name);
        snprintf(target, sizeof target, "%s/%s", directory, target_name);
        if (rename(path, target) != 0) {
            fflush(stdout);
            perror("cannot rename photo");
            return 1;
        }
        printf("renamed %s -> %s\n", photos[i].name, target_name);
    }
    printf("renamed %zu skipped %zu\n", count, files - count);
    return 0;
}
