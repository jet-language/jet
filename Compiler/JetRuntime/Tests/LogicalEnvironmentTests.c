// Link with the complete Jet runtime object and -pthread. No Rust provider.
#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <string.h>

typedef struct { unsigned char *ptr; int64_t len, cap; } Text;
typedef struct { int64_t len, cap; void *data; } NativeList;
_Static_assert(sizeof(NativeList) == 24, "canonical native List header");
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern void jet_rt_logical_env_seed(void);
extern Text *jet_rt_logical_env_get(Text *);
extern int64_t jet_rt_logical_env_get_os(Text *, unsigned char **, int64_t *);
extern int64_t jet_rt_logical_env_set(Text *, Text *);
extern int64_t jet_rt_logical_env_unset(Text *);
extern void jet_rt_logical_env_columns(NativeList *);
extern void jet_rt_startup_env_columns(NativeList *);
extern int64_t jet_rt_env_key_compare(NativeList *, NativeList *);
extern void jet_rt_free(void *, int64_t, int64_t);

static Text text(const char *s) { return (Text){(unsigned char *)s, (int64_t)strlen(s), 0}; }
static void drop_text(Text *t) {
    if (t->cap) jet_rt_free(t->ptr, t->cap, 1);
    jet_rt_free(t, 24, 8);
}
static void strict(Text *key, const char *expected) {
    Text *actual = jet_rt_logical_env_get(key);
    assert(actual && actual->len == (int64_t)strlen(expected));
    assert(memcmp(actual->ptr, expected, (size_t)actual->len) == 0);
    drop_text(actual);
}
static void raw(Text *key, int64_t presence, const char *expected, int64_t length) {
    unsigned char *data = (unsigned char *)1;
    int64_t len = -1;
    assert(jet_rt_logical_env_get_os(key, &data, &len) == presence);
    assert(len == length && (data == 0) == (length == 0));
    if (length) {
        assert(memcmp(data, expected, (size_t)length) == 0);
        jet_rt_free(data, length, 1);
    }
}
static void drop_snapshot(NativeList *columns) {
    NativeList *items = columns->data;
    assert(columns->len == columns->cap);
    for (int64_t i = 0; i < columns->len; ++i) {
        assert(items[i].len == items[i].cap && (items[i].data == 0) == (items[i].len == 0));
        if (items[i].cap) jet_rt_free(items[i].data, items[i].cap, 8);
    }
    if (columns->cap) jet_rt_free(columns->data, columns->cap * 24, 8);
}
static void snapshot_golden(NativeList *columns, const char **names, const char **values, int64_t pairs) {
    assert(columns->len == pairs * 2);
    NativeList *items = columns->data;
    for (int64_t i = 0; i < pairs; ++i) {
        assert(items[2*i].len == (int64_t)strlen(names[i]));
        assert(memcmp(items[2*i].data, names[i], (size_t)items[2*i].len) == 0);
        assert(items[2*i+1].len == (int64_t)strlen(values[i]));
        if (items[2*i+1].len) assert(memcmp(items[2*i+1].data, values[i], (size_t)items[2*i+1].len) == 0);
    }
    drop_snapshot(columns);
}
static Text concurrent_key = {(unsigned char *)"CONCURRENT", 10, 0};
static void *writer(void *unused) {
    (void)unused;
    Text alpha = text("alpha"), beta = text("beta");
    for (int i = 0; i < 500; ++i)
        assert(jet_rt_logical_env_set(&concurrent_key, i & 1 ? &alpha : &beta) == 0);
    return 0;
}
static void *reader(void *unused) {
    (void)unused;
    for (int i = 0; i < 500; ++i) {
        Text *value = jet_rt_logical_env_get(&concurrent_key);
        assert(value && ((value->len == 5 && memcmp(value->ptr, "alpha", 5) == 0) ||
                         (value->len == 4 && memcmp(value->ptr, "beta", 4) == 0)));
        drop_text(value);
        unsigned char *bytes = 0;
        int64_t length = 0;
        assert(jet_rt_logical_env_get_os(&concurrent_key, &bytes, &length) == 1);
        assert((length == 5 && memcmp(bytes, "alpha", 5) == 0) ||
               (length == 4 && memcmp(bytes, "beta", 4) == 0));
        jet_rt_free(bytes, length, 1);
        NativeList columns = {0};
        jet_rt_logical_env_columns(&columns);
        assert(columns.len == 8); // NONUTF, EMPTY, RAW_\xff and CONCURRENT.
        NativeList *items = columns.data;
        int found = 0;
        for (int64_t j = 0; j < columns.len; j += 2) {
            if (items[j].len == 10 && memcmp(items[j].data, "CONCURRENT", 10) == 0) {
                ++found;
                assert((items[j+1].len == 5 && memcmp(items[j+1].data, "alpha", 5) == 0) ||
                       (items[j+1].len == 4 && memcmp(items[j+1].data, "beta", 4) == 0));
            }
        }
        assert(found == 1); // Writer's remove/push never tears the snapshot.
        drop_snapshot(&columns);
    }
    return 0;
}
int main(int argc, char **argv) {
    char mutable[] = "A=first";
    char *args[] = {"logical-env", 0};
    if (argc == 2 && strcmp(argv[1], "empty") == 0) {
        jet_rt_host_arguments(1, args, 0);
        NativeList logical = {-1, -1, (void *)1}, startup = logical;
        jet_rt_logical_env_columns(&logical);
        jet_rt_startup_env_columns(&startup);
        assert(logical.len == 0 && logical.cap == 0 && logical.data == 0);
        assert(startup.len == 0 && startup.cap == 0 && startup.data == 0);
        return 0;
    }
    char *env[] = {mutable, "A=last", "NONUTF=val_\xfe", "EMPTY=", "RAW_\xff=value", 0};
    jet_rt_host_arguments(1, args, env);
    memset(mutable, 'x', sizeof(mutable) - 1);
    jet_rt_logical_env_seed();
    jet_rt_logical_env_seed();
    NativeList startup = {0}, logical = {0};
    jet_rt_startup_env_columns(&startup);
    const char *startup_names[] = {"A", "A", "NONUTF", "EMPTY", "RAW_\xff"};
    const char *startup_values[] = {"first", "last", "val_\xfe", "", "value"};
    snapshot_golden(&startup, startup_names, startup_values, 5);
    jet_rt_logical_env_columns(&logical);
    const char *logical_names[] = {"A", "NONUTF", "EMPTY", "RAW_\xff"};
    const char *logical_values[] = {"last", "val_\xfe", "", "value"};
    snapshot_golden(&logical, logical_names, logical_values, 4);
    NativeList high = {1, 0, (void *)"\xff"}, low = {1, 0, (void *)"\x7f"};
    NativeList prefix = {2, 0, (void *)"\xffx"};
    assert(jet_rt_env_key_compare(&high, &low) == 1);
    assert(jet_rt_env_key_compare(&low, &high) == -1);
    assert(jet_rt_env_key_compare(&high, &high) == 0);
    assert(jet_rt_env_key_compare(&high, &prefix) == -1);
    assert(jet_rt_env_key_compare(&prefix, &high) == 1);
    Text a = text("A"), absent = text("ABSENT"), invalid_utf8 = text("NONUTF"), empty = text("EMPTY");
    strict(&a, "last");
    raw(&a, 1, "last", 4);
    assert(jet_rt_logical_env_get(&absent) == 0);
    raw(&absent, 0, "", 0);
    assert(jet_rt_logical_env_get(&invalid_utf8) == 0);
    raw(&invalid_utf8, 1, "val_\xfe", 5);
    strict(&empty, "");
    raw(&empty, 1, "", 0);

    Text value = text("changed"), invalid_empty = text(""), invalid_equals = text("A=B");
    Text invalid_nul_name = {(unsigned char *)"A\0B", 3, 0};
    Text invalid_nul_value = {(unsigned char *)"v\0x", 3, 0};
    // One explicit witness for each approved status, plus all name constraints.
    assert(jet_rt_logical_env_set(&invalid_empty, &value) == -1);
    assert(jet_rt_logical_env_set(&invalid_equals, &value) == -1);
    assert(jet_rt_logical_env_set(&invalid_nul_name, &value) == -1);
    assert(jet_rt_logical_env_set(&a, &invalid_nul_value) == -2);
    strict(&a, "last");
    assert(jet_rt_logical_env_set(&a, &value) == 0);
    strict(&a, "changed");
    raw(&a, 1, "changed", 7);
    assert(jet_rt_logical_env_unset(&invalid_empty) == -1);
    assert(jet_rt_logical_env_unset(&invalid_equals) == -1);
    assert(jet_rt_logical_env_unset(&invalid_nul_name) == -1);
    assert(jet_rt_logical_env_unset(&a) == 1);
    assert(jet_rt_logical_env_get(&a) == 0);
    raw(&a, 0, "", 0);
    assert(jet_rt_logical_env_unset(&a) == 0);

    Text alpha = text("alpha");
    assert(jet_rt_logical_env_set(&concurrent_key, &alpha) == 0);
    pthread_t reads[2], writes;
    assert(pthread_create(&reads[0], 0, reader, 0) == 0);
    assert(pthread_create(&reads[1], 0, reader, 0) == 0);
    assert(pthread_create(&writes, 0, writer, 0) == 0);
    assert(pthread_join(reads[0], 0) == 0);
    assert(pthread_join(reads[1], 0) == 0);
    assert(pthread_join(writes, 0) == 0);
    return 0;
}
