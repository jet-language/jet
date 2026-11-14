/* http-service: a single-process JSON key-value HTTP/1.1 service over POSIX sockets. C has no
   HTTP library, so each connection's one request is parsed by hand and answered with
   Connection: close. */
#include <arpa/inet.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <unistd.h>

typedef struct {
    char *key, *value;
} Pair;

static Pair *pairs;
static size_t pair_count, pair_cap;

static Pair *find(const char *key) {
    for (size_t i = 0; i < pair_count; i++)
        if (strcmp(pairs[i].key, key) == 0) return &pairs[i];
    return NULL;
}

static void store(const char *key, const char *value, size_t len) {
    Pair *pair = find(key);
    if (!pair) {
        if (pair_count == pair_cap) pairs = realloc(pairs, (pair_cap = pair_cap ? pair_cap * 2 : 16) * sizeof *pairs);
        pair = &pairs[pair_count++];
        pair->key = strdup(key);
    } else {
        free(pair->value);
    }
    pair->value = strndup(value, len);
}

static void reply(int conn, int status, const char *body) {
    char head[256];
    int len = snprintf(head, sizeof head,
                       "HTTP/1.1 %d %s\r\nContent-Type: application/json\r\nContent-Length: %zu\r\nConnection: close\r\n\r\n",
                       status, status == 200 ? "OK" : status == 404 ? "Not Found" : "Bad Request", strlen(body));
    if (write(conn, head, (size_t)len) < 0 || write(conn, body, strlen(body)) < 0) perror("write");
}

/* Reads and answers one request; returns 1 when it was GET /shutdown. */
static int serve(int conn) {
    size_t size = 0, cap = 8192;
    char *raw = malloc(cap);
    size_t body_at = 0; /* offset of the body in raw; 0 until the header block has arrived */
    size_t content_length = 0;
    ssize_t n;
    for (;;) {
        if (cap - size < 2) raw = realloc(raw, cap *= 2);
        if ((n = read(conn, raw + size, cap - size - 1)) <= 0) break;
        size += (size_t)n;
        raw[size] = '\0';
        if (!body_at) {
            char *blank = strstr(raw, "\r\n\r\n");
            if (!blank) continue;
            body_at = (size_t)(blank - raw) + 4;
            for (char *line = strstr(raw, "\r\n") + 2; line < blank; line = strstr(line, "\r\n") + 2)
                if (strncasecmp(line, "Content-Length:", 15) == 0) content_length = strtoul(line + 15, NULL, 10);
        }
        if (size - body_at >= content_length) break;
    }
    int stop = 0;
    char method[16], target[1024];
    const char *body = raw + body_at;
    if (!body_at || sscanf(raw, "%15s %1023s", method, target) != 2) {
        reply(conn, 400, "{\"error\":\"bad request\"}");
    } else {
        char *query = strchr(target, '?');
        if (query) *query = '\0';
        const char *key = strncmp(target, "/kv/", 4) == 0 ? target + 4 : NULL;
        char response[2048];
        if (strcmp(method, "GET") == 0 && strcmp(target, "/health") == 0) {
            reply(conn, 200, "{\"status\":\"ok\"}");
        } else if (strcmp(method, "GET") == 0 && strcmp(target, "/shutdown") == 0) {
            reply(conn, 200, "{\"bye\":true}");
            stop = 1;
        } else if (strcmp(method, "PUT") == 0 && key) {
            store(key, body, content_length);
            snprintf(response, sizeof response, "{\"stored\":\"%s\"}", key);
            reply(conn, 200, response);
        } else if (strcmp(method, "GET") == 0 && key && find(key)) {
            snprintf(response, sizeof response, "{\"key\":\"%s\",\"value\":\"%s\"}", key, find(key)->value);
            reply(conn, 200, response);
        } else {
            reply(conn, 404, "{\"error\":\"not found\"}");
        }
    }
    free(raw);
    close(conn);
    return stop;
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fputs("usage: http-service PORT\n", stderr);
        return 2;
    }
    int listener = socket(AF_INET, SOCK_STREAM, 0);
    setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &(int){1}, sizeof(int));
    struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = htons((unsigned short)atoi(argv[1])),
                                  .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
    if (bind(listener, (struct sockaddr *)&address, sizeof address) != 0 || listen(listener, 64) != 0) {
        perror("listen");
        return 1;
    }
    for (int stopping = 0; !stopping;) {
        int conn = accept(listener, NULL, NULL);
        if (conn < 0) break;
        stopping = serve(conn);
    }
    close(listener);
    return 0;
}
