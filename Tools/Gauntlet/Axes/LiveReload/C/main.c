#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int next_counter(void) {
    FILE *file = fopen(".axis-counter", "r+");
    if (file == NULL) file = fopen(".axis-counter", "w+");
    if (file == NULL) return 0;
    int value = 0;
    if (fscanf(file, "%d", &value) != 1) value = 0;
    rewind(file);
    fprintf(file, "%d\n", value + 1);
    fclose(file);
    return value + 1;
}

static void respond(int client, int counter) {
    char request[1024] = {0};
    (void)recv(client, request, sizeof(request) - 1, 0);
    const int is_ready = strstr(request, "GET /__axis_ready") != NULL;
    const int is_output = strstr(request, "GET /__axis_output") != NULL;
    const char *marker = "reload-before";
    char generated[64];
    char html[128];
    const char *body = marker;
    if (is_ready) {
        snprintf(generated, sizeof(generated), "%d", counter);
        body = generated;
    } else if (!is_output) {
        snprintf(html, sizeof(html), "<div id=\"axis-ready\">%s</div>", marker);
        body = html;
    }
    const char *content_type = is_ready || is_output ? "text/plain" : "text/html";
    char response[256];
    int length = snprintf(response, sizeof(response),
        "HTTP/1.1 200 OK\r\nContent-Type: %s\r\nContent-Length: %zu\r\nConnection: close\r\n\r\n%s",
        content_type, strlen(body), body);
    (void)send(client, response, (size_t)length, 0);
}

int main(int argc, char **argv) {
    if (argc != 2) return 64;
    char *end = NULL;
    long parsed = strtol(argv[1], &end, 10);
    if (end == argv[1] || *end != '\0' || parsed < 1 || parsed > 65535) return 64;

    int socket_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (socket_fd < 0) return 1;
    int reuse = 1;
    setsockopt(socket_fd, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
    struct sockaddr_in address = {0};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    address.sin_port = htons((unsigned short)parsed);
    if (bind(socket_fd, (struct sockaddr *)&address, sizeof(address)) < 0 || listen(socket_fd, 8) < 0) {
        close(socket_fd);
        return errno;
    }
    int counter = next_counter();
    for (;;) {
        int client = accept(socket_fd, NULL, NULL);
        if (client < 0) continue;
        respond(client, counter);
        close(client);
    }
}
