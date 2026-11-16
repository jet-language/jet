#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int by_bytes(const void *left, const void *right) {
    return strcmp(*(char *const *)left, *(char *const *)right);
}

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "notes.txt";
    FILE *input = fopen(path, "rb");
    if (!input) {
        perror("read failed");
        return 1;
    }
    fseek(input, 0, SEEK_END);
    long size = ftell(input);
    rewind(input);
    char *text = malloc((size_t)size + 1);
    if (fread(text, 1, (size_t)size, input) != (size_t)size) {
        perror("read failed");
        return 1;
    }
    fclose(input);
    text[size] = '\0';

    size_t count = 0, capacity = 64;
    char **lines = malloc(capacity * sizeof *lines);
    for (char *line = text; line < text + size;) {
        char *end = strchr(line, '\n');
        char *next = end ? end + 1 : text + size;
        if (!end) end = text + size;
        /* Trim surrounding whitespace in place, then keep the line if anything is left. */
        while (line < end && isspace((unsigned char)*line)) line++;
        while (end > line && isspace((unsigned char)end[-1])) end--;
        if (line < end) {
            *end = '\0';
            if (count == capacity) {
                capacity *= 2;
                lines = realloc(lines, capacity * sizeof *lines);
            }
            lines[count++] = line;
        }
        line = next;
    }
    qsort(lines, count, sizeof *lines, by_bytes);

    printf("lines %zu\n", count);
    for (size_t i = 0; i < count; i++) puts(lines[i]);
    return 0;
}
