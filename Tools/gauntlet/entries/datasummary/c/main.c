#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* The groups in output order; each keeps its values in file order. */
static const char *const names[5] = {"alpha", "beta", "delta", "epsilon", "gamma"};

typedef struct {
    double *values;
    size_t len, cap;
} Values;

static int ascending(const void *left, const void *right) {
    double a = *(const double *)left, b = *(const double *)right;
    return (a > b) - (a < b);
}

static void summary(const char *name, const Values *group) {
    size_t n = group->len;
    double sum = 0.0;
    for (size_t i = 0; i < n; i++) sum += group->values[i];
    double mean = sum / (double)n;
    double *ordered = malloc(n * sizeof *ordered);
    memcpy(ordered, group->values, n * sizeof *ordered);
    qsort(ordered, n, sizeof *ordered, ascending);
    double squares = 0.0;
    for (size_t i = 0; i < n; i++) {
        double d = group->values[i] - mean;
        squares += d * d;
    }
    printf("%s n=%zu mean=%.2f median=%.2f p95=%.2f sd=%.2f\n", name, n, mean, ordered[(n - 1) / 2],
           ordered[(19 * n + 19) / 20 - 1], sqrt(squares / (double)n));
    free(ordered);
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fputs("usage: datasummary FILE\n", stderr);
        return 2;
    }
    FILE *input = fopen(argv[1], "r");
    if (!input) {
        perror(argv[1]);
        return 1;
    }
    Values groups[5] = {0};
    char line[256];
    if (!fgets(line, sizeof line, input)) return 1; /* header */
    while (fgets(line, sizeof line, input)) {
        char *comma = strchr(line, ',');
        if (!comma) continue;
        *comma = '\0';
        int index = -1;
        for (int i = 0; i < 5; i++) {
            if (strcmp(line, names[i]) == 0) {
                index = i;
                break;
            }
        }
        if (index < 0) {
            fprintf(stderr, "unknown group %s\n", line);
            return 1;
        }
        Values *group = &groups[index];
        if (group->len == group->cap) {
            group->cap = group->cap ? group->cap * 2 : 4096;
            group->values = realloc(group->values, group->cap * sizeof *group->values);
        }
        group->values[group->len++] = strtod(comma + 1, NULL);
    }
    fclose(input);

    double total = 0.0;
    size_t count = 0;
    for (int i = 0; i < 5; i++) {
        for (size_t j = 0; j < groups[i].len; j++) total += groups[i].values[j];
        count += groups[i].len;
        summary(names[i], &groups[i]);
    }
    printf("overall n=%zu mean=%.2f\n", count, total / (double)count);
    return 0;
}
