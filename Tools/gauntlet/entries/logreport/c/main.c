#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char *const levels[4] = {"DEBUG", "INFO", "WARN", "ERROR"};

typedef struct {
    char name[32];
    size_t length;
    long errors;
} Component;

static long level_counts[4];
static Component components[64];
static size_t component_count;
static char first[32], last[32];
static long long first_seconds, last_seconds;

/* Most ERROR records first, then name ascending. */
static int by_errors(const void *left, const void *right) {
    const Component *a = left, *b = right;
    if (a->errors != b->errors) return a->errors > b->errors ? -1 : 1;
    return strcmp(a->name, b->name);
}

/* The decimal value of text[start, start + length). */
static long long digits(const char *text, int start, int length) {
    long long value = 0;
    for (int i = start; i < start + length; i++) value = value * 10 + (text[i] - '0');
    return value;
}

/* Seconds since the epoch of an RFC3339 UTC stamp YYYY-MM-DDTHH:MM:SSZ, by fixed field offsets. */
static long long parse_timestamp(const char *text) {
    static const int month_days[12] = {0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334};
    long long year = digits(text, 0, 4), month = digits(text, 5, 2), day = digits(text, 8, 2);
    long long days_before_year = 365 * (year - 1970) + (year - 1969) / 4 - (year - 1901) / 100 + (year - 1601) / 400;
    int leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    long long days = days_before_year + month_days[month - 1] + day - 1 + (leap && month > 2);
    return days * 86400 + digits(text, 11, 2) * 3600 + digits(text, 14, 2) * 60 + digits(text, 17, 2);
}

/* The next space-separated field of [*at, end) in [*start, *stop); 0 past the last field. */
static int next_field(const char *end, const char **at, const char **start, const char **stop) {
    if (*at > end) return 0;
    const char *space = memchr(*at, ' ', (size_t)(end - *at));
    *start = *at;
    *stop = space ? space : end;
    *at = *stop + 1;
    return 1;
}

/* Folds one record [line, end) into the counts. */
static void record(const char *line, const char *end) {
    if (end > line && end[-1] == '\r') end--;
    const char *at = line, *timestamp, *timestamp_end, *level, *level_end, *component, *component_end;
    if (!next_field(end, &at, &timestamp, &timestamp_end) || !next_field(end, &at, &level, &level_end) ||
        !next_field(end, &at, &component, &component_end))
        return;
    size_t timestamp_len = (size_t)(timestamp_end - timestamp);
    if (timestamp_len < 19 || timestamp_len >= sizeof last) return;
    long long seconds = parse_timestamp(timestamp);
    memcpy(last, timestamp, timestamp_len);
    last[timestamp_len] = '\0';
    last_seconds = seconds;
    if (!first[0]) {
        memcpy(first, last, timestamp_len + 1);
        first_seconds = seconds;
    }
    size_t level_len = (size_t)(level_end - level);
    int level_index = -1;
    for (int i = 0; i < 4; i++) {
        if (strlen(levels[i]) == level_len && memcmp(level, levels[i], level_len) == 0) {
            level_index = i;
            break;
        }
    }
    if (level_index < 0) return;
    level_counts[level_index]++;
    size_t component_len = (size_t)(component_end - component);
    if (level_index != 3 || component_len >= sizeof components[0].name) return;
    size_t i = 0;
    while (i < component_count &&
           (components[i].length != component_len || memcmp(components[i].name, component, component_len) != 0))
        i++;
    if (i == component_count) {
        if (component_count == sizeof components / sizeof components[0]) {
            fputs("too many components\n", stderr);
            exit(1);
        }
        memcpy(components[i].name, component, component_len);
        components[i].name[component_len] = '\0';
        components[i].length = component_len;
        components[i].errors = 0;
        component_count++;
    }
    components[i].errors++;
}

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "app.log";
    FILE *input = fopen(path, "rb");
    if (!input) {
        perror(path);
        return 1;
    }
    /* Stream the log through one fixed buffer; a partial last line moves to the front. */
    static char buffer[64 * 1024];
    size_t filled = 0;
    for (;;) {
        size_t got = fread(buffer + filled, 1, sizeof buffer - filled, input);
        filled += got;
        const char *line = buffer, *const end = buffer + filled;
        for (const char *newline; (newline = memchr(line, '\n', (size_t)(end - line))); line = newline + 1)
            record(line, newline);
        if (got == 0) {
            if (line < end) record(line, end);
            break;
        }
        filled = (size_t)(end - line);
        if (filled == sizeof buffer) {
            fputs("line too long\n", stderr);
            return 1;
        }
        memmove(buffer, line, filled);
    }
    fclose(input);

    for (int i = 0; i < 4; i++) printf("%s %ld\n", levels[i], level_counts[i]);
    puts("top-error-components:");
    qsort(components, component_count, sizeof *components, by_errors);
    for (size_t i = 0; i < component_count && i < 3; i++) printf("%ld %s\n", components[i].errors, components[i].name);
    printf("span %s .. %s (%llds)\n", first, last, last_seconds - first_seconds);
    return 0;
}
