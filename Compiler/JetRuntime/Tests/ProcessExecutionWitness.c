// Real child plain ProcessSpec native ABI witness. No Rust layout reads: Spec,
// ProcessReceipt and ProcessChild are opaque; receipt fields are read by the
// physical offsets in ProcessExecutionReceipt.jet (code0 output8 errors16
// success24 signal32 timed_out40; signal is an inline Int? word, 2^62 when
// absent). IOError is 56 bytes (kind0 op8 resource16 has_code24 code32 cause40
// limit48).
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <signal.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>

typedef struct { unsigned char *data; int64_t len, capacity; } Text;
typedef struct {
    int64_t kind, operation;
    Text *resource;
    int64_t has_code, code;
    Text *cause;
    int64_t limit;
} IOError;
_Static_assert(sizeof(Text) == 24, "Text24");
_Static_assert(sizeof(IOError) == 56 && offsetof(IOError, cause) == 40, "IOError56");

typedef struct Spec Spec;
typedef struct ProcessReceipt ProcessReceipt;
typedef struct ProcessChild ProcessChild;
typedef struct JetAuthority JetAuthority;

// HOST_PROCESS_STREAM_* (HostEnums.jet): STREAM=0 INHERIT=1 CAPTURE=2.
#define STREAM_INHERIT 1
#define STREAM_CAPTURE 2
// HOST_IO_KIND_*: INVALID_INPUT=0 NOT_FOUND=1 RESOURCE_LIMIT=8 OTHER=7.
#define KIND_INVALID_INPUT 0
#define KIND_NOT_FOUND 1
#define KIND_OTHER 7
#define KIND_RESOURCE_LIMIT 8
// HOST_IO_OP_*: CLOSE=5 RESOLVE=6.
#define OP_CLOSE 5
#define OP_RESOLVE 6

extern void jet_rt_alloc_init(void);
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern Text *jet_rt_string_from_static(const void *, int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern Spec *jet_std_process_cmd(Text *const *, int64_t);
extern Spec *jet_process_spec_stdout(const Spec *, int64_t);
extern Spec *jet_process_spec_stderr(const Spec *, int64_t);
extern Spec *jet_process_spec_timeout(const Spec *, int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
extern Spec *jet_process_spec_output_limit(const Spec *, int64_t);
extern Spec *jet_process_spec_env(const Spec *, const Text *, const Text *);
extern Spec *jet_process_spec_terminal(const Spec *);
extern Spec *jet_process_spec_detached(const Spec *);
extern Spec *jet_std_process_spec_under(const Spec *, JetAuthority *);
extern JetAuthority *jet_authority_from_rights(const uint64_t *, int64_t);
extern void jet_rt_handle_drop_jet_authority(JetAuthority *);
extern void jet_rt_handle_drop_process_spec(Spec *);
extern int64_t jet_process_spec_run(Spec *, ProcessReceipt **, IOError **);
extern int64_t jet_process_spec_run_checked(Spec *, ProcessReceipt **, IOError **);
extern int64_t jet_process_spec_spawn(Spec *, ProcessChild **, IOError **);
extern int64_t jet_process_spec_plan(Spec *, void *, IOError **);
extern int64_t jet_process_child_wait(ProcessChild *, ProcessReceipt **, IOError **);
extern int64_t jet_process_child_id(ProcessChild *);
extern void jet_rt_handle_drop_process_child(ProcessChild *);
extern void jet_rt_handle_drop_process_receipt(ProcessReceipt *);
extern int64_t jet_std_process_pipeline(Spec *const *, int64_t, ProcessReceipt **, IOError **);
extern void jet_rt_handle_drop_io_error(IOError *);
extern int64_t jet_rt_world_new(void);
extern void jet_rt_world_enter(int64_t, int64_t *);
extern void jet_rt_world_leave(int64_t *);
extern void jet_rt_world_drop(int64_t);
extern int64_t jet_rt_root_begin(int64_t *);
extern void jet_rt_root_leave(int64_t, int64_t *);
extern int64_t jet_rt_root_take_stop(int64_t, int64_t *);
extern void jet_rt_stop_drop(int64_t);
extern int64_t jet_scheduler_native_status(void);
extern int64_t jet_rt_handle_field_process_child_stdout(ProcessChild *);
extern int64_t jet_rt_handle_clone_process_stdout_stream(int64_t);
extern void jet_rt_handle_drop_process_stdout_stream(int64_t);
extern int64_t jet_process_stream_line_bytes_raw(int64_t, int64_t *, int64_t *, int64_t *);
extern void jet_rt_free(int64_t, int64_t, int64_t);
#ifdef JET_PROCESS_JET_FIXTURES
// The fresh native library exports this Jet fixture; Spec is an opaque word.
extern int64_t jet_rt_process_execution_controlled_fixture(Spec *);
#else
// Use the actual pack's public World/root and launch providers; no alternate
// process implementation or separately rebuilt runtime object participates.
static int64_t jet_rt_process_execution_controlled_fixture(Spec *spec) {
    int64_t root_out[4]={0}, save[4]={0}, code=0;
    int64_t world=jet_rt_world_new(), root=jet_rt_root_begin(root_out);
    jet_rt_world_enter(world,save);
    ProcessChild *child=(ProcessChild *)(uintptr_t)0x1234;
    IOError *error=(IOError *)(uintptr_t)0x5678;
    int64_t result=jet_process_spec_spawn(spec,&child,&error);
    bool refused=result==0 && child==(ProcessChild *)(uintptr_t)0x1234 &&
        error==(IOError *)(uintptr_t)0x5678 && jet_scheduler_native_status()==5;
    jet_rt_world_leave(save);
    int64_t stop=jet_rt_root_take_stop(root,&code);
    jet_rt_stop_drop(stop);
    jet_rt_root_leave(root,root_out);
    jet_rt_world_drop(world);
    return refused && stop!=0;
}
#endif

static void check(bool ok, const char *what) {
    if (!ok) {
        fprintf(stderr, "ProcessExecution witness failed: %s\n", what);
        exit(1);
    }
}

static Text *txt(const char *s) { return jet_rt_string_from_static(s, (int64_t)strlen(s)); }

static bool eq(const Text *t, const char *s) {
    size_t n = strlen(s);
    return t && (size_t)t->len == n && memcmp(t->data, s, n) == 0;
}

static bool has(const Text *t, const char *s) {
    size_t n = strlen(s);
    if (!t || (size_t)t->len < n) return false;
    for (int64_t i = 0; i + (int64_t)n <= t->len; ++i)
        if (memcmp(t->data + i, s, n) == 0) return true;
    return false;
}

static int64_t word(const void *p, size_t off) {
    int64_t v;
    memcpy(&v, (const unsigned char *)p + off, 8);
    return v;
}

// Receipt accessors (ProcessExecutionReceipt.jet offsets).
static int64_t receipt_code(const ProcessReceipt *r) { return jet_int_owned_to_i64(word(r, 0)); }
static const Text *receipt_output(const ProcessReceipt *r) { return (const Text *)(intptr_t)word(r, 8); }
static const Text *receipt_errors(const ProcessReceipt *r) { return (const Text *)(intptr_t)word(r, 16); }
static int64_t receipt_success(const ProcessReceipt *r) { return word(r, 24); }
static bool receipt_has_signal(const ProcessReceipt *r) { return word(r, 32) != (INT64_C(1) << 62); }
static int64_t receipt_signal(const ProcessReceipt *r) { return jet_int_owned_to_i64(word(r, 32)); }
static int64_t receipt_timed_out(const ProcessReceipt *r) { return word(r, 40); }

static Spec *replace(Spec *old, Spec *next) {
    jet_rt_handle_drop_process_spec(old);
    return next;
}

static Spec *spec_texts(Text **words, int64_t n) { return jet_std_process_cmd(words, n); }

static Spec *spec_of(const char *const *words, int64_t n) {
    Text *arr[8];
    check(n <= 8, "spec words fit");
    for (int64_t i = 0; i < n; ++i) arr[i] = txt(words[i]);
    return spec_texts(arr, n);
}

#define SPEC(...) spec_of((const char *[]){__VA_ARGS__}, sizeof((const char *[]){__VA_ARGS__}) / sizeof(const char *))

// Expected error: result 0, error non-null, kind/op when >= 0, exact cause when non-NULL.
static void expect_error(int64_t ok, IOError *e, int64_t kind, int64_t op, const char *cause) {
    check(ok == 0 && e != NULL, "expected error result");
    if (kind >= 0) check(e->kind == kind, "error kind");
    if (op >= 0) check(e->operation == op, "error operation");
    if (cause) check(eq(e->cause, cause), "error cause text");
    jet_rt_handle_drop_io_error(e);
}

static void drop_receipt(ProcessReceipt *r) { jet_rt_handle_drop_process_receipt(r); }

int main(int argc, char **argv, char **envp) {
    jet_rt_alloc_init();
    jet_rt_host_arguments(argc, argv, envp);

    // 1. Capture stdout and stderr from a real child.
    {
        Spec *s = SPEC("/bin/sh", "-c", "printf out; printf err >&2");
        s = replace(s, jet_process_spec_stdout(s, STREAM_CAPTURE));
        s = replace(s, jet_process_spec_stderr(s, STREAM_CAPTURE));
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_run(s, &r, &e);
        check(ok == 1 && e == NULL && r != NULL, "capture run ok");
        check(receipt_code(r) == 0 && receipt_success(r) == 1, "capture success");
        check(receipt_timed_out(r) == 0 && !receipt_has_signal(r), "capture not killed");
        check(has(receipt_output(r), "out"), "captured stdout");
        check(has(receipt_errors(r), "err"), "captured stderr");
        drop_receipt(r);
        jet_rt_handle_drop_process_spec(s);
    }

    // 2. Inherited stdout is not captured; stderr still is.
    {
        Spec *s = SPEC("/bin/sh", "-c", "printf err >&2");
        s = replace(s, jet_process_spec_stdout(s, STREAM_INHERIT));
        s = replace(s, jet_process_spec_stderr(s, STREAM_CAPTURE));
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        check(jet_process_spec_run(s, &r, &e) == 1 && e == NULL, "inherit run ok");
        check(receipt_code(r) == 0 && receipt_output(r)->len == 0, "inherited stdout not captured");
        check(has(receipt_errors(r), "err"), "captured stderr under inherit");
        drop_receipt(r);
        jet_rt_handle_drop_process_spec(s);
    }

    // 3. Nonzero exit is a receipt for run, not an error.
    {
        Spec *s = SPEC("/bin/sh", "-c", "exit 7");
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        check(jet_process_spec_run(s, &r, &e) == 1 && e == NULL, "exit7 run returns receipt");
        check(receipt_code(r) == 7 && receipt_success(r) == 0, "exit7 code and failure");
        drop_receipt(r);
        jet_rt_handle_drop_process_spec(s);
    }

    // 4. run_checked: success path yields the receipt.
    {
        Spec *s = SPEC("/bin/sh", "-c", "exit 0");
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        check(jet_process_spec_run_checked(s, &r, &e) == 1 && e == NULL && r != NULL, "checked success");
        check(receipt_success(r) == 1, "checked success receipt");
        drop_receipt(r);
        jet_rt_handle_drop_process_spec(s);
    }

    // 5. run_checked: nonzero exit is a CLOSE error carrying the exit code.
    {
        Spec *s = SPEC("/bin/sh", "-c", "exit 7");
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_run_checked(s, &r, &e);
        check(ok == 0 && r == NULL && e != NULL, "checked nonzero is error");
        check(e->operation == OP_CLOSE, "checked nonzero operation");
        check(has(e->cause, "process exited unsuccessfully: code=7"), "checked nonzero cause");
        check(has(e->cause, ", stderr="), "checked nonzero stderr field");
        jet_rt_handle_drop_io_error(e);
        jet_rt_handle_drop_process_spec(s);
    }

    // 6. spawn then child wait yields the exit receipt; the child PID is reaped and gone.
    {
        Spec *s = SPEC("/bin/sh", "-c", "exit 3");
        ProcessChild *child = NULL;
        IOError *e = NULL;
        check(jet_process_spec_spawn(s, &child, &e) == 1 && e == NULL && child != NULL, "spawn ok");
        int64_t owned_pid = jet_process_child_id(child);
        int64_t pid = jet_int_owned_to_i64(owned_pid);
        jet_rt_int_drop(owned_pid);
        check(pid > 0, "spawned child has pid");
        ProcessReceipt *r = NULL;
        check(jet_process_child_wait(child, &r, &e) == 1 && e == NULL && r != NULL, "child wait ok");
        check(receipt_code(r) == 3 && receipt_success(r) == 0, "spawned child exit code");
        check(kill((pid_t)pid, 0) == -1 && errno == ESRCH, "child pid reaped after wait");
        drop_receipt(r);
        jet_rt_handle_drop_process_child(child);
        jet_rt_handle_drop_process_spec(s);
    }

    // 6b. A streamed stdout reads the collector's capture through the child's
    // own cell: lines arrive in order, a clone of the cell keeps the child
    // after its last alias drops, and the receipt still holds every byte.
    {
        Spec *s = SPEC("/bin/sh", "-c", "printf 'a\\nb'");
        s = replace(s, jet_process_spec_stdout(s, 0));
        ProcessChild *child = NULL;
        IOError *e = NULL;
        check(jet_process_spec_spawn(s, &child, &e) == 1 && e == NULL && child != NULL, "stream spawn ok");
        int64_t pipe = jet_rt_handle_field_process_child_stdout(child);
        check(pipe != 0 && pipe == jet_rt_handle_field_process_child_stdout(child), "stdout field lends one cell");
        int64_t data = 0, len = 0, error = 0;
        check(jet_process_stream_line_bytes_raw(pipe, &data, &len, &error) == 1 && error == 0 && len == 2 && memcmp((const void *)(intptr_t)data, "a\n", 2) == 0, "streamed first line");
        jet_rt_free(data, len, 1);
        ProcessReceipt *r = NULL;
        check(jet_process_child_wait(child, &r, &e) == 1 && e == NULL && r != NULL, "stream wait ok");
        check(eq(receipt_output(r), "a\nb"), "receipt keeps streamed bytes");
        drop_receipt(r);
        check(jet_process_stream_line_bytes_raw(pipe, &data, &len, &error) == 1 && error == 0 && len == 0 && data == 0, "stream at EOF after wait");
        jet_rt_handle_drop_process_child(child);
        jet_rt_handle_drop_process_spec(s);
    }
    {
        Spec *s = SPEC("/bin/sh", "-c", "printf 'x\\ny\\n'");
        s = replace(s, jet_process_spec_stdout(s, 0));
        ProcessChild *child = NULL;
        IOError *e = NULL;
        check(jet_process_spec_spawn(s, &child, &e) == 1 && e == NULL && child != NULL, "pinned spawn ok");
        int64_t held = jet_rt_handle_clone_process_stdout_stream(jet_rt_handle_field_process_child_stdout(child));
        jet_rt_handle_drop_process_child(child);
        int64_t data = 0, len = 0, error = 0;
        check(jet_process_stream_line_bytes_raw(held, &data, &len, &error) == 1 && error == 0 && len == 2 && memcmp((const void *)(intptr_t)data, "x\n", 2) == 0, "clone reads after the child alias drops");
        jet_rt_free(data, len, 1);
        check(jet_process_stream_line_bytes_raw(held, &data, &len, &error) == 1 && error == 0 && len == 2 && memcmp((const void *)(intptr_t)data, "y\n", 2) == 0, "clone reads every line");
        jet_rt_free(data, len, 1);
        check(jet_process_stream_line_bytes_raw(held, &data, &len, &error) == 1 && error == 0 && len == 0, "clone reaches EOF");
        jet_rt_handle_drop_process_stdout_stream(held);
        jet_rt_handle_drop_process_spec(s);
    }

    // 7. Timeout kills the child process group and reports timed_out with SIGKILL.
    {
        Spec *s = SPEC("/bin/sh", "-c", "sleep 5");
        const int64_t timeout_ns = jet_int_owned_from_i64(INT64_C(100000000));
        s = replace(s, jet_process_spec_timeout(s, timeout_ns));
        jet_rt_int_drop(timeout_ns);
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        check(jet_process_spec_run(s, &r, &e) == 1 && e == NULL, "timeout run returns receipt");
        check(receipt_timed_out(r) == 1, "timed_out set");
        check(receipt_has_signal(r) && receipt_signal(r) == 9, "timeout killed with SIGKILL");
        check(receipt_success(r) == 0, "timeout is not success");
        drop_receipt(r);
        jet_rt_handle_drop_process_spec(s);
    }

    // 8. Output cap: captured stdout beyond the limit is ResourceLimit (Output).
    {
        Spec *s = SPEC("/bin/sh", "-c", "printf 0123456789");
        s = replace(s, jet_process_spec_stdout(s, STREAM_CAPTURE));
        s = replace(s, jet_process_spec_output_limit(s, 4));
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_run(s, &r, &e);
        expect_error(ok, e, KIND_RESOURCE_LIMIT, -1, NULL);
        jet_rt_handle_drop_process_spec(s);
    }

    // 9. Missing executable: the child's exec errno (HostSpawn's CLOEXEC
    // pipe) is the error's kind, code and cause, naming the program.
    {
        Spec *s = SPEC("/nonexistent/jet-no-such");
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_run(s, &r, &e);
        check(ok == 0 && r == NULL && e != NULL, "missing executable is error");
        check(e->kind == KIND_NOT_FOUND && e->operation == OP_RESOLVE, "missing executable is NotFound/Resolve");
        check(e->has_code == 1 && e->code == 2, "missing executable carries ENOENT");
        check(eq(e->resource, "/nonexistent/jet-no-such"), "missing executable names the program");
        check(has(e->cause, " (os error 2)"), "missing executable errno cause");
        jet_rt_handle_drop_io_error(e);
        jet_rt_handle_drop_process_spec(s);
    }

    // 10. Empty command: InvalidInput/Resolve naming the empty command (it
    // has no program to name).
    {
        Spec *s = spec_of(NULL, 0);
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_run(s, &r, &e);
        check(ok == 0 && e != NULL && eq(e->resource, "process.cmd([])"), "empty command names process.cmd([])");
        expect_error(ok, e, KIND_INVALID_INPUT, OP_RESOLVE, "process command needs at least one word");
        jet_rt_handle_drop_process_spec(s);
    }

    // 10b. Empty pipeline: InvalidInput/Resolve naming process.pipeline([]).
    {
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_std_process_pipeline(NULL, 0, &r, &e);
        check(ok == 0 && e != NULL && eq(e->resource, "process.pipeline([])"), "empty pipeline names process.pipeline([])");
        expect_error(ok, e, KIND_INVALID_INPUT, OP_RESOLVE, "process.pipeline needs at least one command");
    }

    // 11. NUL byte inside a command word: Other/Resolve.
    {
        Text *words[2] = { txt("/bin/sh"), jet_rt_string_from_static("a\0b", 3) };
        Spec *s = spec_texts(words, 2);
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_run(s, &r, &e);
        expect_error(ok, e, KIND_OTHER, OP_RESOLVE, "nul byte found in provided data");
        jet_rt_handle_drop_process_spec(s);
    }

    // 12. Invalid environment variable name: InvalidInput/Resolve.
    {
        Spec *s = SPEC("/bin/sh", "-c", "exit 0");
        Text *name = txt("BAD=NAME");
        Text *value = txt("v");
        s = replace(s, jet_process_spec_env(s, name, value));
        jet_rt_string_drop(name);
        jet_rt_string_drop(value);
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_run(s, &r, &e);
        expect_error(ok, e, KIND_INVALID_INPUT, OP_RESOLVE, "invalid environment variable name");
        jet_rt_handle_drop_process_spec(s);
    }

    // 13. Invalid environment variable value (NUL byte): InvalidInput/Resolve.
    {
        Spec *s = SPEC("/bin/sh", "-c", "exit 0");
        Text *name = txt("JET_OK");
        Text *value = jet_rt_string_from_static("a\0b", 3);
        s = replace(s, jet_process_spec_env(s, name, value));
        jet_rt_string_drop(name);
        jet_rt_string_drop(value);
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_run(s, &r, &e);
        expect_error(ok, e, KIND_INVALID_INPUT, OP_RESOLVE, "invalid environment variable value");
        jet_rt_handle_drop_process_spec(s);
    }

    // 14. Unbound plan (no .under authority): InvalidInput/Resolve.
    {
        Spec *s = SPEC("/bin/sh", "-c", "exit 0");
        void *ok_unused = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_plan(s, ok_unused, &e);
        expect_error(ok, e, KIND_INVALID_INPUT, OP_RESOLVE,
            "ProcessSpec.plan() requires an authority policy from .under(...)");
        jet_rt_handle_drop_process_spec(s);
    }

    // 15. Authority-bound spec is refused before spawn: Other/Resolve.
    {
        static const uint64_t rights[1] = { 0 };
        JetAuthority *auth = jet_authority_from_rights(rights, 0);
        Spec *s = SPEC("/bin/sh", "-c", "exit 0");
        s = replace(s, jet_std_process_spec_under(s, auth));
        jet_rt_handle_drop_jet_authority(auth);
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_run(s, &r, &e);
        expect_error(ok, e, KIND_OTHER, OP_RESOLVE,
            "authority-bound process execution requires the #398/#893 isolation backend; refusing before spawn");
        jet_rt_handle_drop_process_spec(s);
    }

    // 16. Terminal sessions cannot be detached.
    {
        Spec *s = SPEC("/bin/sh", "-c", "exit 0");
        s = replace(s, jet_process_spec_terminal(s));
        s = replace(s, jet_process_spec_detached(s));
        ProcessReceipt *r = NULL;
        IOError *e = NULL;
        int64_t ok = jet_process_spec_run(s, &r, &e);
        expect_error(ok, e, -1, -1, "terminal sessions cannot be detached");
        jet_rt_handle_drop_process_spec(s);
    }

    // 17. Controlled-world regression: native stop, no child, errsentinel untouched.
    {
        Spec *s = SPEC("/bin/sh", "-c", "exit 0");
        check(jet_rt_process_execution_controlled_fixture(s) == 1, "controlled-world refusal");
        jet_rt_handle_drop_process_spec(s);
        puts("ProcessExecution controlled-world refusal: native stop, no child, errsentinel untouched");
    }

    // 18/19. Strict capture decoding preserves Rust's invalid/incomplete
    // Utf8Error text instead of silently producing replacement characters.
    {
        const char *scripts[]={"printf '\\377'","printf '\\342'"};
        const char *causes[]={"invalid utf-8 sequence of 1 bytes from index 0",
            "incomplete utf-8 byte sequence from index 0"};
        for(unsigned i=0;i<2;++i) {
            Spec *s=SPEC("/bin/sh","-c",scripts[i]);
            ProcessReceipt *r=NULL; IOError *e=NULL;
            int64_t ok=jet_process_spec_run(s,&r,&e);
            check(!r,"invalid UTF8 produces no receipt");
            check(e && eq(e->resource,"process stdout"),"invalid UTF8 stream resource");
            expect_error(ok,e,KIND_OTHER,0,causes[i]);
            jet_rt_handle_drop_process_spec(s);
        }
    }
    // 20. Dropping the last attached child alias kills and reaps a live child.
    {
        Spec *s=SPEC("/bin/sh","-c","sleep 5");
        ProcessChild *child=NULL; IOError *e=NULL;
        check(jet_process_spec_spawn(s,&child,&e)==1 && child && !e,"drop cleanup spawn");
        int64_t owned_pid=jet_process_child_id(child);
        int64_t pid=jet_int_owned_to_i64(owned_pid);
        jet_rt_int_drop(owned_pid);
        jet_rt_handle_drop_process_child(child);
        errno=0;
        check(kill((pid_t)pid,0)==-1 && errno==ESRCH,"last alias drop reaps real child");
        jet_rt_handle_drop_process_spec(s);
    }

    puts("ProcessExecution real-child native witness OK");
    return 0;
}
