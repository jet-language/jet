#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char name[64];
    size_t length;
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

/* The amount in [start, stop) as a double; returns 0 when the text isn't a number.
   Plain decimals with at most 15 significant digits take the exact fast path
   (an exact integer divided by an exact power of ten rounds once, as strtod
   would); anything else goes to strtod. */
static int parse_amount(const char *start, const char *stop, double *out) {
    static const double powers[] = {1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15};
    const char *p = start;
    int negative = p < stop && (*p == '-' || *p == '+') ? *p++ == '-' : 0;
    long long mantissa = 0;
    int digits = 0, fraction = -1;
    for (; p < stop; p++) {
        if (*p == '.' && fraction < 0) {
            fraction = 0;
        } else if (*p >= '0' && *p <= '9' && digits < 15) {
            mantissa = mantissa * 10 + (*p - '0');
            digits++;
            if (fraction >= 0) fraction++;
        } else {
            break;
        }
    }
    if (p == stop && digits > 0) {
        double value = (double)mantissa / powers[fraction < 0 ? 0 : fraction];
        *out = negative ? -value : value;
        return 1;
    }
    char text[64];
    size_t length = (size_t)(stop - start);
    if (length == 0 || length >= sizeof text) return 0;
    memcpy(text, start, length);
    text[length] = '\0';
    char *parsed_end;
    *out = strtod(text, &parsed_end);
    return *parsed_end == '\0';
}

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "sales.csv";
    FILE *input = fopen(path, "rb");
    if (!input || fseek(input, 0, SEEK_END) != 0) {
        perror(path);
        return 1;
    }
    long size = ftell(input);
    rewind(input);
    char *text = malloc((size_t)size + 1);
    if (!text || fread(text, 1, (size_t)size, input) != (size_t)size) {
        perror(path);
        return 1;
    }
    fclose(input);
    const char *const text_end = text + size;

    Group groups[64];
    size_t group_count = 0, total_count = 0;
    double total_sum = 0.0;
    const char *line = memchr(text, '\n', (size_t)size); /* skip the header */
    line = line ? line + 1 : text_end;
    while (line < text_end) {
        const char *newline = memchr(line, '\n', (size_t)(text_end - line));
        const char *end = newline ? newline : text_end;
        const char *next = newline ? newline + 1 : text_end;
        if (end > line && end[-1] == '\r') end--;
        const char *at = line, *start, *stop, *region, *region_end, *amount, *amount_end;
        line = next;
        if (end == at) continue;
        if (!next_field(end, &at, &start, &stop)) continue; /* id */
        if (!next_field(end, &at, &region, &region_end)) continue;
        if (!next_field(end, &at, &start, &stop)) continue; /* category */
        if (!next_field(end, &at, &amount, &amount_end)) continue;
        double value;
        if (!parse_amount(amount, amount_end, &value) || !(value > 0.0)) continue;

        size_t region_len = (size_t)(region_end - region);
        if (region_len >= sizeof groups[0].name) continue;
        Group *group = NULL;
        for (size_t i = 0; i < group_count; i++) {
            if (groups[i].length == region_len && memcmp(groups[i].name, region, region_len) == 0) {
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
            group->length = region_len;
            group->count = 0;
            group->sum = 0.0;
        }
        group->count++;
        group->sum += value;
        total_count++;
        total_sum += value;
    }
    qsort(groups, group_count, sizeof *groups, by_name);
    for (size_t i = 0; i < group_count; i++) printf("%s n=%zu sum=%.2f\n", groups[i].name, groups[i].count, groups[i].sum);
    printf("total n=%zu sum=%.2f\n", total_count, total_sum);
    return 0;
}
