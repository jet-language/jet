// Link against the complete composed Jet runtime object, not an allocator-only
// seed. Expected stdout bytes: "one\ntwo\n\xff\x00B"; stderr: "err\n".
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>

typedef struct { unsigned char *ptr; int64_t len, cap; } Text;
typedef struct { int64_t len, cap; uint8_t *data; } Bytes;
typedef struct {
    int64_t operation;
    Text *resource;
    int64_t has_os_code, os_code;
    Text *cause;
} IOContext;
_Static_assert(sizeof(IOContext) == 40, "IOContext size");
typedef struct {
    int64_t kind, operation;
    Text *resource;
    int64_t has_os_code, os_code;
    Text *cause;
    int64_t limit;
} IOError;
_Static_assert(sizeof(Text) == 24 && sizeof(Bytes) == 24, "carrier sizes");
_Static_assert(sizeof(IOError) == 56, "IOError size");
typedef struct { int64_t kind, has_os_code, os_code; Text *message; } ResolveError;
_Static_assert(sizeof(ResolveError) == 32, "ResolveError size");
extern ResolveError *jet_rt_record_new_jet_resolve_error(int64_t, int64_t, int64_t, Text *);
extern ResolveError *jet_rt_handle_clone_jet_resolve_error(ResolveError *);
extern void jet_rt_handle_drop_jet_resolve_error(ResolveError *);
extern void jet_rt_host_program_start(void);
extern void jet_stdio_stderr_bytes_raw(const uint8_t *, int64_t, int64_t *, uint8_t *);
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern int64_t jet_rt_env_count(void);
extern void jet_rt_env_entry(int64_t, Bytes *, Bytes *);
extern int64_t jet_sys_startup_env_count_raw(void);
extern int64_t jet_sys_startup_env_name_raw(int64_t, uint8_t **);
extern int64_t jet_sys_startup_env_value_raw(int64_t, uint8_t **);
extern int64_t jet_rt_arg_count(void);
extern void jet_rt_arg_entry(int64_t, Bytes *);
extern void jet_rt_free(void *, int64_t, int64_t);
extern void jet_rt_handle_drop_io_error(IOError *);
extern IOError *jet_rt_handle_clone_io_error(IOError *);
extern IOContext *jet_rt_record_new_io_context(int64_t, Text **, int64_t *, Text **);
extern void jet_rt_handle_drop_io_context(IOContext *);
extern IOContext *jet_rt_handle_clone_io_context(IOContext *);
extern int64_t jet_rt_record_get_io_context_os_code(IOContext *, int64_t *);
extern IOError *jet_rt_record_new_io_error_other(IOContext *);
extern IOContext *jet_rt_record_get_io_error_other(IOError *);
extern void *jet_std_io_stdout(void);
extern void *jet_std_io_stderr(void);
extern void jet_rt_handle_drop_jet_stdout(void *);
extern void jet_rt_handle_drop_jet_stderr(void *);
extern int64_t jet_std_io_stdout_write(void *, Text *, IOError **);
extern int64_t jet_std_io_stdout_write_line(void *, Text *, IOError **);
extern int64_t jet_std_io_stdout_write_bytes(void *, const uint8_t *, int64_t, IOError **);
extern int64_t jet_std_io_stdout_flush(void *, IOError **);
extern int64_t jet_std_io_stderr_write_line(void *, Text *, IOError **);
extern int64_t jet_std_io_stderr_flush(void *, IOError **);
extern bool jet_std_io_stdout_is_tty(void *);

static void check_bytes(Bytes value, const char *expected, size_t length) {
    assert(value.len == (int64_t)length && value.cap == value.len);
    assert((value.data == NULL) == (length == 0));
    for (size_t i = 0; i < length; ++i) assert(value.data[i] == (unsigned char)expected[i]);
    jet_rt_free(value.data, value.cap, 8);
}
static void check_text(Text *value, const char *expected) {
    assert(value && value->len == (int64_t)strlen(expected));
    assert(memcmp(value->ptr, expected, (size_t)value->len) == 0);
}
static Text borrowed(const char *value) {
    return (Text){(unsigned char *)value, (int64_t)strlen(value), 0};
}

int main(void) {
    static char *args[] = {"exe", "\xff", "", NULL};
    static char mutable_entry[] = "A=1";
    static char *env[] = {mutable_entry, "broken", "=FOO=bar", "=bad", "", "NONUTF_\xff=val_\xfe", "EMPTY=", "A=last", NULL};
    // Garbage upper bits in argc must not enter the startup snapshot.
    jet_rt_host_arguments(INT64_C(0x123400000003), args, env);
    jet_rt_host_arguments(3, args, env);
    // Entryless hosts may reuse their capture buffers after the call. Both
    // key and value must remain the originally captured bytes.
    mutable_entry[0] = 'Z';
    mutable_entry[2] = '9';
    assert(jet_rt_arg_count() == 3);
    assert(jet_rt_env_count() == 5 && jet_sys_startup_env_count_raw() == 5);
    const char *names[] = {"A", "=FOO", "NONUTF_\xff", "EMPTY", "A"};
    const char *values[] = {"1", "bar", "val_\xfe", "", "last"};
    for (int64_t i = 0; i < 5; ++i) {
        Bytes name = {0}, value = {0};
        jet_rt_env_entry(i, &name, &value);
        check_bytes(name, names[i], strlen(names[i]));
        check_bytes(value, values[i], strlen(values[i]));
        uint8_t *data = NULL;
        int64_t length = jet_sys_startup_env_name_raw(i, &data);
        check_bytes((Bytes){length, length, data}, names[i], strlen(names[i]));
        length = jet_sys_startup_env_value_raw(i, &data);
        check_bytes((Bytes){length, length, data}, values[i], strlen(values[i]));
    }
    for (int64_t i = 0; i < 3; ++i) {
        Bytes value = {0};
        jet_rt_arg_entry(i, &value);
        check_bytes(value, args[i], strlen(args[i]));
    }
    Text resolve_message = borrowed("resolver message");
    ResolveError *resolve = jet_rt_record_new_jet_resolve_error(2, INT64_C(0x100), 17, &resolve_message);
    assert(resolve->kind == 2 && resolve->has_os_code == 0 && resolve->os_code == 17);
    ResolveError *resolve_clone = jet_rt_handle_clone_jet_resolve_error(resolve);
    assert(resolve_clone != resolve && resolve_clone->message == resolve->message);
    check_text(resolve_clone->message, "resolver message");
    jet_rt_handle_drop_jet_resolve_error(resolve);
    jet_rt_handle_drop_jet_resolve_error(resolve_clone);
    resolve = jet_rt_record_new_jet_resolve_error(2, INT64_C(0x101), 17, &resolve_message);
    assert(resolve->has_os_code == 1);
    jet_rt_handle_drop_jet_resolve_error(resolve);
    Text resource = borrowed("resource"), cause = borrowed("cause");
    Text *resource_pointer = &resource, *cause_pointer = &cause;
    int64_t os_code = 9, os_word = 0;
    IOContext *context = jet_rt_record_new_io_context(0, &resource_pointer, &os_code, &cause_pointer);
    assert(context->operation == 0 && context->has_os_code == 1 && context->os_code == 9);
    assert(context->resource == &resource && context->cause == &cause);
    assert(jet_rt_record_get_io_context_os_code(context, &os_word) == 1 && os_word == 9);
    IOContext *context_clone = jet_rt_handle_clone_io_context(context);
    assert(context_clone != context && context_clone->resource == context->resource);
    jet_rt_handle_drop_io_context(context_clone);
    // Construction consumes only the context header; its text owners move.
    Text *moved_resource = context->resource;
    IOError *context_error = jet_rt_record_new_io_error_other(context);
    assert(context_error->resource == moved_resource);
    IOContext *projected = jet_rt_record_get_io_error_other(context_error);
    assert(projected->resource == context_error->resource && projected->os_code == 9);
    check_text(projected->resource, "resource");
    check_text(projected->cause, "cause");
    jet_rt_handle_drop_io_context(projected);
    jet_rt_handle_drop_io_error(context_error);
    void *out = jet_std_io_stdout(), *err = jet_std_io_stderr();
    assert(out == (void *)(uintptr_t)1 && err == (void *)(uintptr_t)1);
    IOError *error = NULL;
    Text one = borrowed("one"), two = borrowed("two\n"), report = borrowed("err"), pending = borrowed("B");
    assert(jet_std_io_stdout_write_line(out, &one, &error) == 1);
    assert(jet_std_io_stdout_write(out, &two, &error) == 1);
    const uint8_t binary[] = {255, 0};
    assert(jet_std_io_stdout_write_bytes(out, binary, 2, &error) == 1);
    // stderr drains the pending stdout buffer before its immediate frame.
    assert(jet_std_io_stderr_write_line(err, &report, &error) == 1);
    assert(jet_std_io_stderr_flush(err, &error) == 1);
    int saved = dup(1);
    assert(saved >= 0 && close(1) == 0);
    assert(!jet_std_io_stdout_is_tty(out));
    assert(jet_std_io_stdout_write(out, &pending, &error) == 1);
    assert(jet_std_io_stdout_flush(out, &error) == 0);
    assert(error && error->has_os_code == 0 && error->os_code == 0);
    check_text(error->resource, "stdout");
    check_text(error->cause, "Bad file descriptor (os error 9)");
    IOError *copy = jet_rt_handle_clone_io_error(error);
    assert(copy != error && copy->resource == error->resource && copy->cause == error->cause);
    check_text(copy->resource, "stdout");
    check_text(copy->cause, "Bad file descriptor (os error 9)");
    jet_rt_handle_drop_io_error(error);
    jet_rt_handle_drop_io_error(copy);
    assert(dup2(saved, 1) == 1 && close(saved) == 0);
    // Failed flush retains exactly the pending suffix, not a duplicate prefix.
    assert(jet_std_io_stdout_flush(out, &error) == 1);
    // Entryless libraries leave SIGPIPE untouched. A host opting into program
    // lifecycle must receive the kernel EPIPE fact instead of dying by SIGPIPE.
    jet_rt_host_program_start();
    int broken_pipe[2], saved_stderr = dup(2);
    assert(saved_stderr >= 0 && pipe(broken_pipe) == 0);
    assert(close(broken_pipe[0]) == 0 && dup2(broken_pipe[1], 2) == 2);
    assert(close(broken_pipe[1]) == 0);
    int64_t pipe_errno = 0;
    uint8_t write_zero = 1, pipe_byte = 'X';
    jet_stdio_stderr_bytes_raw(&pipe_byte, 1, &pipe_errno, &write_zero);
    assert(pipe_errno == 32 && write_zero == 0);
    assert(dup2(saved_stderr, 2) == 2 && close(saved_stderr) == 0);
    jet_rt_handle_drop_jet_stdout(out);
    jet_rt_handle_drop_jet_stderr(err);
    return 0;
}
