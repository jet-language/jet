#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    FILE *input = fopen(argv[1], "rb");
    if (!input) return 2;
    if (fseek(input, 0, SEEK_END) != 0) return 2;
    long end = ftell(input);
    if (end < 0 || fseek(input, 0, SEEK_SET) != 0) return 2;

    size_t size = (size_t)end;
    char *text = malloc(size + 1);
    if (!text) return 2;
    if (fread(text, 1, size, input) != size) {
        free(text);
        fclose(input);
        return 2;
    }
    text[size] = '\0';

    const char *needle = "struct";
    const size_t needle_len = strlen(needle);
    size_t matches = 0;
    for (char *cursor = text; (cursor = strstr(cursor, needle)) != NULL; cursor += needle_len) {
        matches++;
    }

    printf("matches %zu\n", matches);
    free(text);
    fclose(input);
    return 0;
}
