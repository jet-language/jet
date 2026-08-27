#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *word;
    size_t count;
} Entry;

static unsigned long hash_word(const char *word) {
    unsigned long hash = 1469598103934665603UL;
    while (*word) {
        hash ^= (unsigned char)*word++;
        hash *= 1099511628211UL;
    }
    return hash;
}

static char *copy_word(const char *word) {
    size_t length = strlen(word) + 1;
    char *copy = malloc(length);
    if (copy) memcpy(copy, word, length);
    return copy;
}

static int compare_entries(const void *left_ptr, const void *right_ptr) {
    const Entry *left = left_ptr;
    const Entry *right = right_ptr;
    if (left->count != right->count) return left->count < right->count ? 1 : -1;
    return strcmp(left->word, right->word);
}

int main(int argc, char **argv) {
    FILE *input = fopen(argv[1], "rb");
    size_t capacity = 16384, distinct = 0, total = 0, length = 0;
    Entry *table = calloc(capacity, sizeof(*table));
    int ch;
    char buffer[128];
    if (!input || !table) return 1;

    while ((ch = fgetc(input)) != EOF) {
        if (ch == ' ' || ch == '\n' || ch == '\t' || ch == '\r' || ch == '\f' || ch == '\v') {
            if (length == 0) continue;
            buffer[length] = '\0';
            size_t slot = hash_word(buffer) & (capacity - 1);
            while (table[slot].word && strcmp(table[slot].word, buffer) != 0)
                slot = (slot + 1) & (capacity - 1);
            if (!table[slot].word) {
                table[slot].word = copy_word(buffer);
                table[slot].count = 0;
                distinct++;
            }
            table[slot].count++;
            total++;
            length = 0;
        } else if (length + 1 < sizeof(buffer)) {
            buffer[length++] = (char)ch;
        }
    }
    fclose(input);

    Entry *ranked = malloc(distinct * sizeof(*ranked));
    size_t next = 0;
    for (size_t i = 0; i < capacity; i++) {
        if (table[i].word) ranked[next++] = table[i];
    }
    qsort(ranked, distinct, sizeof(*ranked), compare_entries);
    for (size_t i = 0; i < 20; i++) printf("%zu %s\n", ranked[i].count, ranked[i].word);
    printf("distinct %zu total %zu\n", distinct, total);
    free(ranked);
    for (size_t i = 0; i < capacity; i++) free(table[i].word);
    free(table);
    return 0;
}
