/* http-client over POSIX sockets: HTTP/1.1 GET with Connection: close. C has no HTTP or JSON
   library, so the request is written and the peer's flat item objects are read by hand. */
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

typedef struct {
    long id, qty;
    char name[64];
} Item;

typedef struct {
    int status;
    char *body; /* NUL-terminated */
} Response;

static char host[256], port[16];

static void fail(const char *what) {
    fprintf(stderr, "%s\n", what);
    exit(1);
}

static Response get(const char *path) {
    struct addrinfo hints = {.ai_family = AF_UNSPEC, .ai_socktype = SOCK_STREAM}, *addresses;
    if (getaddrinfo(host, port, &hints, &addresses) != 0) fail("cannot resolve host");
    int fd = -1;
    for (struct addrinfo *a = addresses; a && fd < 0; a = a->ai_next) {
        fd = socket(a->ai_family, a->ai_socktype, a->ai_protocol);
        if (fd >= 0 && connect(fd, a->ai_addr, a->ai_addrlen) != 0) {
            close(fd);
            fd = -1;
        }
    }
    freeaddrinfo(addresses);
    if (fd < 0) fail("cannot connect");

    char request[512];
    int len = snprintf(request, sizeof request, "GET %s HTTP/1.1\r\nHost: %s:%s\r\nConnection: close\r\n\r\n", path, host, port);
    if (write(fd, request, (size_t)len) != len) fail("cannot send request");
    size_t size = 0, cap = 4096;
    char *raw = malloc(cap);
    ssize_t n;
    while ((n = read(fd, raw + size, cap - size - 1)) > 0) {
        size += (size_t)n;
        if (cap - size < 2) raw = realloc(raw, cap *= 2);
    }
    close(fd);
    raw[size] = '\0';

    Response response = {0};
    char *body = strstr(raw, "\r\n\r\n");
    if (sscanf(raw, "HTTP/1.%*d %d", &response.status) != 1 || !body) fail("malformed response");
    response.body = body + 4;
    return response;
}

/* The integer or string value of "key" inside the object text [object, end). */
static const char *value_of(const char *object, const char *end, const char *key) {
    char quoted[32];
    snprintf(quoted, sizeof quoted, "\"%s\"", key);
    for (const char *at = object; (at = strstr(at, quoted)) && at < end; at += strlen(quoted)) {
        const char *p = at + strlen(quoted);
        while (*p == ' ') p++;
        if (*p != ':') continue;
        p++;
        while (*p == ' ') p++;
        return p;
    }
    fail("missing field");
    return NULL;
}

/* One flat {"id":..,"name":"..","qty":..} object starting at `object`; returns its end. */
static const char *read_item(const char *object, Item *item) {
    const char *end = strchr(object, '}');
    if (!end) fail("unterminated item");
    item->id = strtol(value_of(object, end, "id"), NULL, 10);
    item->qty = strtol(value_of(object, end, "qty"), NULL, 10);
    const char *name = value_of(object, end, "name");
    if (*name != '"') fail("name is not a string");
    const char *quote = strchr(name + 1, '"');
    if (!quote || (size_t)(quote - name - 1) >= sizeof item->name) fail("bad name");
    size_t len = (size_t)(quote - name - 1);
    memcpy(item->name, name + 1, len);
    item->name[len] = '\0';
    return end + 1;
}

int main(int argc, char **argv) {
    const char *base = argc > 1 ? argv[1] : "http://127.0.0.1:18400";
    if (sscanf(base, "http://%255[^:/]:%15[0-9]", host, port) != 2) fail("base must be http://HOST:PORT");

    Response listing = get("/items");
    if (listing.status != 200) fail("listing request failed");
    Item items[64];
    size_t count = 0;
    const char *p = strchr(value_of(listing.body, listing.body + strlen(listing.body), "items"), '[');
    if (!p) fail("items is not a list");
    for (p++; (p = strpbrk(p, "{]")) && *p == '{' && count < 64;) p = read_item(p, &items[count++]);
    printf("items %zu\n", count);

    const int ids[] = {2, 5, 99};
    for (size_t i = 0; i < 3; i++) {
        char path[64];
        snprintf(path, sizeof path, "/items/%d", ids[i]);
        Response response = get(path);
        if (response.status == 404) {
            printf("item %d missing\n", ids[i]);
        } else {
            Item item;
            read_item(response.body, &item);
            printf("item %ld %s qty=%ld\n", item.id, item.name, item.qty);
        }
    }
    long total = 0;
    for (size_t i = 0; i < count; i++) total += items[i].qty;
    printf("total-qty %ld\n", total);
    return 0;
}
