#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char name[64];
    size_t count;
    double sum;
} Group;

/* The next RFC-4180 field of [*at, end): its text (between the quotes when quoted, "" left
   doubled) in [*start, *stop). Returns 0 past the last field or on a malformed quoted field. */
static int next_field(const char *end, const char **at, const char **start, const char **stop) {
    const char *p = *at;
    if (p > end) return 0;
    if (p < end && *p == '"') {
        const char *value = ++p;
        for (; p < end; p++) {
            if (*p != '"') continue;
            if (p + 1 < end && p[1] == '"') {
                p++;
                continue;
            }
            *start = value;
            *stop = p++;
            if (p < end && *p != ',') return 0;
            *at = p + 1; /* past the comma, or past end on the last field */
            return 1;
        }
        return 0;
    }
    const char *comma = memchr(p, ',', (size_t)(end - p));
    *start = p;
    *stop = comma ? comma : end;
    *at = (comma ? comma : end) + 1;
    return 1;
}

static int by_name(const void *left, const void *right) {
    return strcmp(((const Group *)left)->name, ((const Group *)right)->name);
}

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "sales.csv";
    FILE *input = fopen(path, "r");
    if (!input) {
        perror(path);
        return 1;
    }
    Group groups[64];
    size_t group_count = 0, total_count = 0;
    double total_sum = 0.0;
    char line[1024];
    if (!fgets(line, sizeof line, input)) return 1; /* header */
    while (fgets(line, sizeof line, input)) {
        const char *end = line + strcspn(line, "\r\n");
        if (end == line) continue;
        const char *at = line, *start, *stop, *region, *region_end, *amount, *amount_end;
        if (!next_field(end, &at, &start, &stop)) continue; /* id */
        if (!next_field(end, &at, &region, &region_end)) continue;
        if (!next_field(end, &at, &start, &stop)) continue; /* category */
        if (!next_field(end, &at, &amount, &amount_end)) continue;
        char amount_text[64];
        size_t amount_len = (size_t)(amount_end - amount);
        if (amount_len == 0 || amount_len >= sizeof amount_text) continue;
        memcpy(amount_text, amount, amount_len);
        amount_text[amount_len] = '\0';
        char *parsed_end;
        double value = strtod(amount_text, &parsed_end);
        if (*parsed_end != '\0' || !(value > 0.0)) continue;

        size_t region_len = (size_t)(region_end - region);
        if (region_len >= sizeof groups[0].name) continue;
        Group *group = NULL;
        for (size_t i = 0; i < group_count; i++) {
            if (strlen(groups[i].name) == region_len && memcmp(groups[i].name, region, region_len) == 0) {
                group = &groups[i];
                break;
            }
        }
        if (!group) {
            if (group_count == sizeof groups / sizeof groups[0]) {
                fputs("too many regions\n", stderr);
                return 1;
            }
            group = &groups[group_count++];
            memcpy(group->name, region, region_len);
            group->name[region_len] = '\0';
            group->count = 0;
            group->sum = 0.0;
        }
        group->count++;
        group->sum += value;
        total_count++;
        total_sum += value;
    }
    fclose(input);
    qsort(groups, group_count, sizeof *groups, by_name);
    for (size_t i = 0; i < group_count; i++) printf("%s n=%zu sum=%.2f\n", groups[i].name, groups[i].count, groups[i].sum);
    printf("total n=%zu sum=%.2f\n", total_count, total_sum);
    return 0;
}
