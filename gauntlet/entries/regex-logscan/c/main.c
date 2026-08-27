#include <regex.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char ip[64];
    int count;
} Client;

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    FILE *input = fopen(argv[1], "r");
    if (!input) return 2;

    regex_t pattern;
    const char *source = "^([0-9.]+) - - \\[[^]]+\\] \"GET (/api/[^ ]+) HTTP/1[.]1\" 5[0-9][0-9] ";
    if (regcomp(&pattern, source, REG_EXTENDED) != 0) return 2;
    Client clients[50] = {0};
    size_t line_capacity = 0;
    char *line = NULL;
    int matches = 0;

    while (getline(&line, &line_capacity, input) != -1) {
        regmatch_t groups[3];
        if (regexec(&pattern, line, 3, groups, 0) != 0) continue;
        int length = (int)(groups[1].rm_eo - groups[1].rm_so);
        char ip[64];
        memcpy(ip, line + groups[1].rm_so, (size_t)length);
        ip[length] = '\0';
        int index = 0;
        while (index < 50 && strcmp(clients[index].ip, ip) != 0 && clients[index].ip[0] != '\0') index++;
        if (index == 50) continue;
        if (clients[index].ip[0] == '\0') strcpy(clients[index].ip, ip);
        clients[index].count++;
        matches++;
    }

    printf("matches %d\n", matches);
    for (int rank = 0; rank < 5; rank++) {
        int best = -1;
        for (int i = 0; i < 50; i++) {
            if (clients[i].ip[0] == '\0' || clients[i].count == -1) continue;
            if (best == -1 || clients[i].count > clients[best].count ||
                (clients[i].count == clients[best].count && strcmp(clients[i].ip, clients[best].ip) < 0)) best = i;
        }
        if (best != -1) {
            printf("%d %s\n", clients[best].count, clients[best].ip);
            clients[best].count = -1;
        }
    }

    free(line);
    regfree(&pattern);
    fclose(input);
    return 0;
}
