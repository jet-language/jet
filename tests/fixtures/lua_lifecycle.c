#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <string.h>
#include <time.h>

extern int64_t jet_lua_ops_open(void);
extern int64_t jet_lua_ops_take_error(void);
extern const char *jet_lua_ops_invoke_transform(int64_t, const char *, int64_t);
extern const char *jet_lua_ops_invoke_echo(int64_t, const char *, int64_t);
extern const char *jet_lua_ops_invoke_fail_call(int64_t, const char *, int64_t);
extern const char *jet_lua_ops_invoke_spin(int64_t, const char *, int64_t);
extern void jet_lua_ops_cancel(int64_t);
extern void jet_lua_ops_close(int64_t);

static int64_t handle;
static int64_t code;
static void *spin(void *unused) {
    (void)unused;
    jet_lua_ops_invoke_spin(handle, "null", 10000);
    code = jet_lua_ops_take_error();
    return 0;
}

int main(void) {
    handle = jet_lua_ops_open();
    int64_t other = jet_lua_ops_open();
    assert(handle && other);
    const char *first = jet_lua_ops_invoke_transform(handle, "{\"map\":{\"x\":1},\"list\":[true,null],\"scalar\":2.5}", 1000);
    assert(!jet_lua_ops_take_error() && strstr(first, "\"calls\":1"));
    const char *second = jet_lua_ops_invoke_transform(handle, "false", 1000);
    assert(!jet_lua_ops_take_error() && strstr(second, "\"calls\":2"));
    const char *isolated = jet_lua_ops_invoke_transform(other, "false", 1000);
    assert(!jet_lua_ops_take_error() && strstr(isolated, "\"calls\":1"));
    jet_lua_ops_invoke_fail_call(handle, "\"private\"", 1000);
    assert(jet_lua_ops_take_error() == 4);
    jet_lua_ops_invoke_echo(handle, "{invalid", 1000);
    assert(jet_lua_ops_take_error() == 6);
    jet_lua_ops_invoke_spin(handle, "null", 5);
    assert(jet_lua_ops_take_error() == 2);
    assert(!strcmp(jet_lua_ops_invoke_echo(handle, "\"deadline recovery\"", 1000), "\"deadline recovery\""));
    assert(!jet_lua_ops_take_error());
    pthread_t thread;
    assert(!pthread_create(&thread, 0, spin, 0));
    struct timespec wait = {0, 20000000};
    nanosleep(&wait, 0);
    jet_lua_ops_cancel(handle);
    pthread_join(thread, 0);
    assert(code == 3);
    assert(!strcmp(jet_lua_ops_invoke_echo(handle, "\"cancel recovery\"", 1000), "\"cancel recovery\""));
    assert(!jet_lua_ops_take_error());
    jet_lua_ops_close(handle);
    jet_lua_ops_invoke_echo(handle, "null", 1000);
    assert(jet_lua_ops_take_error() == 1);
    jet_lua_ops_close(handle);
    assert(jet_lua_ops_take_error() == 1);
    jet_lua_ops_close(other);
    return 0;
}
