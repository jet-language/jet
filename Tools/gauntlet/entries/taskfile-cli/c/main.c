/* taskfile-cli: a JSON task store in tasks.json, {"tasks":[{"id":1,"text":"...","done":false},...]}.
   C has no JSON library, so the store's own shape is read and written by hand. */
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define STORE_PATH "tasks.json"

typedef struct {
    long id;
    char *text;
    int done;
} Task;

typedef struct {
    Task *tasks;
    size_t len, cap;
} Store;

typedef struct {
    const char *p, *end;
} Cursor;

static void skip_space(Cursor *c) {
    while (c->p < c->end && isspace((unsigned char)*c->p)) c->p++;
}

static int expect(Cursor *c, char ch) {
    skip_space(c);
    if (c->p < c->end && *c->p == ch) {
        c->p++;
        return 1;
    }
    return 0;
}

static void put_utf8(char **out, unsigned long code) {
    char *o = *out;
    if (code < 0x80) {
        *o++ = (char)code;
    } else if (code < 0x800) {
        *o++ = (char)(0xC0 | code >> 6);
        *o++ = (char)(0x80 | (code & 0x3F));
    } else if (code < 0x10000) {
        *o++ = (char)(0xE0 | code >> 12);
        *o++ = (char)(0x80 | (code >> 6 & 0x3F));
        *o++ = (char)(0x80 | (code & 0x3F));
    } else {
        *o++ = (char)(0xF0 | code >> 18);
        *o++ = (char)(0x80 | (code >> 12 & 0x3F));
        *o++ = (char)(0x80 | (code >> 6 & 0x3F));
        *o++ = (char)(0x80 | (code & 0x3F));
    }
    *out = o;
}

static int hex4(Cursor *c, unsigned long *code) {
    if (c->end - c->p < 4) return 0;
    char digits[5] = {c->p[0], c->p[1], c->p[2], c->p[3], 0};
    char *stop;
    *code = strtoul(digits, &stop, 16);
    c->p += 4;
    return *stop == '\0';
}

/* A JSON string, decoded into a fresh NUL-terminated buffer; NULL when malformed. */
static char *read_string(Cursor *c) {
    if (!expect(c, '"')) return NULL;
    char *text = malloc((size_t)(c->end - c->p) + 1), *o = text;
    while (c->p < c->end && *c->p != '"') {
        char ch = *c->p++;
        if (ch != '\\') {
            *o++ = ch;
            continue;
        }
        if (c->p == c->end) break;
        unsigned long code;
        switch (*c->p++) {
        case '"': *o++ = '"'; break;
        case '\\': *o++ = '\\'; break;
        case '/': *o++ = '/'; break;
        case 'b': *o++ = '\b'; break;
        case 'f': *o++ = '\f'; break;
        case 'n': *o++ = '\n'; break;
        case 'r': *o++ = '\r'; break;
        case 't': *o++ = '\t'; break;
        case 'u':
            if (!hex4(c, &code)) goto bad;
            if (code >= 0xD800 && code < 0xDC00 && c->end - c->p >= 6 && c->p[0] == '\\' && c->p[1] == 'u') {
                unsigned long low;
                c->p += 2;
                if (!hex4(c, &low) || low < 0xDC00 || low > 0xDFFF) goto bad;
                code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
            }
            put_utf8(&o, code);
            break;
        default: goto bad;
        }
    }
    if (c->p == c->end) goto bad;
    c->p++;
    *o = '\0';
    return text;
bad:
    free(text);
    return NULL;
}

static int read_task(Cursor *c, Task *task) {
    task->id = 0;
    task->text = NULL;
    task->done = 0;
    if (!expect(c, '{')) return 0;
    if (expect(c, '}')) return 1;
    do {
        char *key = read_string(c);
        if (!key || !expect(c, ':')) return 0;
        skip_space(c);
        if (strcmp(key, "id") == 0) {
            char *stop;
            task->id = strtol(c->p, &stop, 10);
            if (stop == c->p) return 0;
            c->p = stop;
        } else if (strcmp(key, "text") == 0) {
            if (!(task->text = read_string(c))) return 0;
        } else if (strcmp(key, "done") == 0) {
            if (c->end - c->p >= 4 && strncmp(c->p, "true", 4) == 0) {
                task->done = 1;
                c->p += 4;
            } else if (c->end - c->p >= 5 && strncmp(c->p, "false", 5) == 0) {
                c->p += 5;
            } else {
                return 0;
            }
        } else {
            return 0;
        }
        free(key);
    } while (expect(c, ','));
    if (!task->text) task->text = strdup("");
    return expect(c, '}');
}

static void push(Store *store, Task task) {
    if (store->len == store->cap) {
        store->cap = store->cap ? store->cap * 2 : 16;
        store->tasks = realloc(store->tasks, store->cap * sizeof *store->tasks);
    }
    store->tasks[store->len++] = task;
}

/* A missing or malformed store is an empty one, as in the other rails. */
static Store load(void) {
    Store store = {0};
    FILE *file = fopen(STORE_PATH, "rb");
    if (!file) return store;
    fseek(file, 0, SEEK_END);
    long size = ftell(file);
    rewind(file);
    char *bytes = malloc((size_t)size);
    size_t got = fread(bytes, 1, (size_t)size, file);
    fclose(file);
    Cursor c = {bytes, bytes + got};
    char *key;
    if (!expect(&c, '{') || !(key = read_string(&c)) || strcmp(key, "tasks") != 0 || !expect(&c, ':') ||
        !expect(&c, '['))
        return store;
    if (!expect(&c, ']')) {
        do {
            Task task;
            if (!read_task(&c, &task)) return (Store){0};
            push(&store, task);
        } while (expect(&c, ','));
        if (!expect(&c, ']')) return (Store){0};
    }
    return store;
}

static void write_string(FILE *out, const char *text) {
    fputc('"', out);
    for (const unsigned char *p = (const unsigned char *)text; *p; p++) {
        if (*p == '"' || *p == '\\') fprintf(out, "\\%c", *p);
        else if (*p == '\n') fputs("\\n", out);
        else if (*p == '\r') fputs("\\r", out);
        else if (*p == '\t') fputs("\\t", out);
        else if (*p < 0x20) fprintf(out, "\\u%04x", *p);
        else fputc(*p, out);
    }
    fputc('"', out);
}

static void save(const Store *store) {
    FILE *out = fopen(STORE_PATH, "wb");
    if (!out) {
        perror("could not write " STORE_PATH);
        exit(1);
    }
    fputs("{\"tasks\":[", out);
    for (size_t i = 0; i < store->len; i++) {
        fprintf(out, "%s{\"id\":%ld,\"text\":", i ? "," : "", store->tasks[i].id);
        write_string(out, store->tasks[i].text);
        fprintf(out, ",\"done\":%s}", store->tasks[i].done ? "true" : "false");
    }
    fputs("]}", out);
    if (fclose(out) != 0) {
        perror("could not write " STORE_PATH);
        exit(1);
    }
}

int main(int argc, char **argv) {
    const char *command = argc > 1 ? argv[1] : "";
    Store store = load();
    if (strcmp(command, "add") == 0) {
        Task task = {(long)store.len + 1, strdup(argc > 2 ? argv[2] : ""), 0};
        push(&store, task);
        save(&store);
        printf("added %ld %s\n", task.id, task.text);
    } else if (strcmp(command, "done") == 0) {
        long id = -1;
        if (argc > 2) sscanf(argv[2], "%ld", &id);
        for (size_t i = 0; i < store.len; i++) {
            if (store.tasks[i].id == id) {
                store.tasks[i].done = 1;
                save(&store);
                printf("done %ld\n", id);
                return 0;
            }
        }
        printf("no task %ld\n", id);
    } else if (strcmp(command, "list") == 0) {
        size_t open = 0, done = 0;
        for (size_t i = 0; i < store.len; i++) {
            printf("[%c] %ld %s\n", store.tasks[i].done ? 'x' : ' ', store.tasks[i].id, store.tasks[i].text);
            if (store.tasks[i].done) done++;
            else open++;
        }
        printf("open %zu done %zu\n", open, done);
    }
    return 0;
}
