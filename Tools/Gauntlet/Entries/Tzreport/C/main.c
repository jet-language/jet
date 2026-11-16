#define _DEFAULT_SOURCE /* timegm */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct {
    char name[64];
    time_t at; /* UTC seconds */
    size_t order;
} Meeting;

/* By UTC instant; equal instants keep file order. */
static int by_instant(const void *left, const void *right) {
    const Meeting *a = left, *b = right;
    if (a->at != b->at) return a->at < b->at ? -1 : 1;
    return a->order < b->order ? -1 : a->order > b->order;
}

/* RFC3339 with Z or a numeric +HH:MM / -HH:MM offset, as UTC seconds; -1 on a malformed stamp. */
static int parse_rfc3339(const char *text, time_t *out) {
    struct tm local = {0};
    int consumed = 0;
    if (sscanf(text, "%4d-%2d-%2dT%2d:%2d:%2d%n", &local.tm_year, &local.tm_mon, &local.tm_mday, &local.tm_hour,
               &local.tm_min, &local.tm_sec, &consumed) != 6 || consumed != 19)
        return -1;
    local.tm_year -= 1900;
    local.tm_mon -= 1;
    time_t seconds = timegm(&local);
    const char *zone = text + 19;
    if (zone[0] == 'Z') {
        *out = seconds;
        return 0;
    }
    int hours, minutes;
    if ((zone[0] != '+' && zone[0] != '-') || sscanf(zone + 1, "%2d:%2d", &hours, &minutes) != 2) return -1;
    long offset = hours * 3600L + minutes * 60L;
    *out = zone[0] == '+' ? seconds - offset : seconds + offset;
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fputs("usage: tzreport MEETINGS\n", stderr);
        return 2;
    }
    FILE *input = fopen(argv[1], "r");
    if (!input) {
        perror(argv[1]);
        return 1;
    }
    Meeting meetings[256];
    size_t count = 0;
    char line[256];
    while (fgets(line, sizeof line, input)) {
        line[strcspn(line, "\r\n")] = '\0';
        char *bar = strchr(line, '|');
        if (!bar || bar - line >= (long)sizeof meetings[0].name || count == sizeof meetings / sizeof meetings[0]) {
            fprintf(stderr, "bad meeting line: %s\n", line);
            return 1;
        }
        *bar = '\0';
        Meeting *m = &meetings[count];
        if (parse_rfc3339(bar + 1, &m->at) != 0) {
            fprintf(stderr, "bad timestamp: %s\n", bar + 1);
            return 1;
        }
        strcpy(m->name, line);
        m->order = count++;
    }
    fclose(input);
    qsort(meetings, count, sizeof *meetings, by_instant);

    for (size_t i = 0; i < count; i++) {
        struct tm utc;
        gmtime_r(&meetings[i].at, &utc);
        char stamp[32], weekday[8];
        strftime(stamp, sizeof stamp, "%Y-%m-%dT%H:%M:%SZ", &utc);
        strftime(weekday, sizeof weekday, "%a", &utc);
        if (i == 0)
            printf("%s utc=%s day=%s gap=-\n", meetings[i].name, stamp, weekday);
        else
            printf("%s utc=%s day=%s gap=%lld\n", meetings[i].name, stamp, weekday,
                   (long long)(meetings[i].at - meetings[i - 1].at) / 60);
    }
    printf("span %lld minutes %zu\n", (long long)(meetings[count - 1].at - meetings[0].at) / 60, count);
    return 0;
}
