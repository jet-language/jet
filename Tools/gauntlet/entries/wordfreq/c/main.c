#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *word;
    size_t length;
    size_t count;
} Entry;

static uint64_t hash_word(const char *word, size_t length) {
    uint64_t hash = UINT64_C(14695981039346656037);
    for (size_t i = 0; i < length; i++) {
        hash ^= (unsigned char)word[i];
        hash *= UINT64_C(1099511628211);
    }
    return hash;
}

static int valid_utf8(const unsigned char *text, size_t length) {
    for (size_t i = 0; i < length;) {
        unsigned first = text[i++];
        if (first < 0x80) continue;
        size_t trailing;
        if (first >= 0xc2 && first <= 0xdf) trailing = 1;
        else if (first >= 0xe0 && first <= 0xef) trailing = 2;
        else if (first >= 0xf0 && first <= 0xf4) trailing = 3;
        else return 0;
        if (trailing > length - i) return 0;
        unsigned second = text[i];
        if ((first == 0xe0 && second < 0xa0) ||
            (first == 0xed && second >= 0xa0) ||
            (first == 0xf0 && second < 0x90) ||
            (first == 0xf4 && second >= 0x90)) return 0;
        for (size_t j = 0; j < trailing; j++) {
            if (text[i + j] < 0x80 || text[i + j] > 0xbf) return 0;
        }
        i += trailing;
    }
    return 1;
}

static int grow_table(Entry **table, size_t *capacity) {
    if (*capacity > SIZE_MAX / (2 * sizeof(Entry))) return 0;
    size_t next_capacity = *capacity * 2;
    Entry *next = calloc(next_capacity, sizeof(*next));
    if (!next) return 0;
    for (size_t i = 0; i < *capacity; i++) {
        Entry item = (*table)[i];
        if (!item.word) continue;
        size_t slot = hash_word(item.word, item.length) & (next_capacity - 1);
        while (next[slot].word) slot = (slot + 1) & (next_capacity - 1);
        next[slot] = item;
    }
    free(*table);
    *table = next;
    *capacity = next_capacity;
    return 1;
}

static int count_word(Entry **table, size_t *capacity, size_t *distinct,
                      size_t *total, const char *word, size_t length) {
    if (!valid_utf8((const unsigned char *)word, length) || *total == SIZE_MAX) return 0;
    if (*distinct >= *capacity - *capacity / 4 && !grow_table(table, capacity)) return 0;
    size_t slot = hash_word(word, length) & (*capacity - 1);
    while ((*table)[slot].word) {
        Entry *item = &(*table)[slot];
        if (item->length == length && memcmp(item->word, word, length) == 0) {
            if (item->count == SIZE_MAX) return 0;
            item->count++;
            (*total)++;
            return 1;
        }
        slot = (slot + 1) & (*capacity - 1);
    }
    char *copy = malloc(length);
    if (!copy) return 0;
    memcpy(copy, word, length);
    (*table)[slot] = (Entry){.word = copy, .length = length, .count = 1};
    (*distinct)++;
    (*total)++;
    return 1;
}

static int compare_entries(const void *left_ptr, const void *right_ptr) {
    const Entry *left = left_ptr;
    const Entry *right = right_ptr;
    if (left->count != right->count) return left->count < right->count ? 1 : -1;
    size_t common = left->length < right->length ? left->length : right->length;
    int order = memcmp(left->word, right->word, common);
    if (order) return order;
    return (left->length > right->length) - (left->length < right->length);
}

int main(int argc, char **argv) {
    if (argc < 2) return 1;
    FILE *input = fopen(argv[1], "rb");
    size_t capacity = 16384, distinct = 0, total = 0, length = 0, buffer_capacity = 128;
    Entry *table = calloc(capacity, sizeof(*table));
    char *buffer = malloc(buffer_capacity);
    Entry *ranked = NULL;
    int status = 1;
    if (!input || !table || !buffer) goto cleanup;

    int ch;
    while ((ch = fgetc(input)) != EOF) {
        if (ch == ' ' || (ch >= '\t' && ch <= '\r')) {
            if (length && !count_word(&table, &capacity, &distinct, &total, buffer, length)) goto cleanup;
            length = 0;
        } else {
            if (length == buffer_capacity) {
                if (buffer_capacity > SIZE_MAX / 2) goto cleanup;
                size_t next_capacity = buffer_capacity * 2;
                char *next = realloc(buffer, next_capacity);
                if (!next) goto cleanup;
                buffer = next;
                buffer_capacity = next_capacity;
            }
            buffer[length++] = (char)ch;
        }
    }
    if (ferror(input)) goto cleanup;
    if (length && !count_word(&table, &capacity, &distinct, &total, buffer, length)) goto cleanup;

    if (distinct) {
        if (distinct > SIZE_MAX / sizeof(*ranked)) goto cleanup;
        ranked = malloc(distinct * sizeof(*ranked));
        if (!ranked) goto cleanup;
    }
    size_t next = 0;
    for (size_t i = 0; i < capacity; i++) {
        if (table[i].word) ranked[next++] = table[i];
    }
    if (distinct) qsort(ranked, distinct, sizeof(*ranked), compare_entries);
    for (size_t i = 0; i < distinct && i < 20; i++) {
        if (printf("%zu ", ranked[i].count) < 0 ||
            fwrite(ranked[i].word, 1, ranked[i].length, stdout) != ranked[i].length ||
            putchar('\n') == EOF) goto cleanup;
    }
    if (printf("distinct %zu total %zu\n", distinct, total) < 0) goto cleanup;
    status = 0;

cleanup:
    if (input) fclose(input);
    free(buffer);
    free(ranked);
    if (table) {
        for (size_t i = 0; i < capacity; i++) free(table[i].word);
        free(table);
    }
    return status;
}
