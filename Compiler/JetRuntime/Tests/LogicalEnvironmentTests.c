// Link with the complete Jet runtime object and -pthread. No Rust provider.
#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <string.h>

typedef struct { unsigned char *ptr; int64_t len, cap; } Text;
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern void jet_rt_logical_env_seed(void);
extern Text *jet_rt_logical_env_get(Text *);
extern int64_t jet_rt_logical_env_get_os(Text *, unsigned char **, int64_t *);
extern int64_t jet_rt_logical_env_set(Text *, Text *);
extern int64_t jet_rt_logical_env_unset(Text *);
extern void jet_rt_free(void *, int64_t, int64_t);
extern void jet_rt_string_drop(Text *);

static Text text(const char *s) { return (Text){(unsigned char *)s, (int64_t)strlen(s), 0}; }
static void drop_text(Text *t) { jet_rt_string_drop(t); }
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
    }
    return 0;
}
int main(void) {
    char mutable[] = "A=first";
    char *args[] = {"logical-env", 0};
    char *env[] = {mutable, "A=last", "NONUTF=val_\xfe", "EMPTY=", 0};
    jet_rt_host_arguments(1, args, env);
    memset(mutable, 'x', sizeof(mutable) - 1);
    jet_rt_logical_env_seed();
    jet_rt_logical_env_seed();
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
