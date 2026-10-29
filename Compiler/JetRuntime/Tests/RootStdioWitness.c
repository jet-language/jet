// #4881: link only the freshly composed full Jet runtime pack. These entries
// are non-unwinding machine/C callers, not Rust panic or scheduler-test shims.
// A shared pipe observes the required buffered-stdout-before-report ordering.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

typedef struct { const unsigned char *ptr; int64_t len, cap; } Text;
_Static_assert(sizeof(Text) == 24, "E4 text header");
typedef struct {
    int64_t refs;
    int64_t (*body)(int64_t);
    int64_t (*release)(int64_t);
} ExitClosure;
_Static_assert(sizeof(ExitClosure) == 24, "canonical three-word native closure");
_Static_assert(offsetof(ExitClosure, body) == 8 && offsetof(ExitClosure, release) == 16,
               "canonical closure callback slots");
extern void jet_rt_host_init(void);
extern int32_t jet_rt_main(void (*entry)(void));
extern void jet_term_write_stdout_line(const Text *, uint8_t);
extern void jet_rt_panic_message(const unsigned char *, int64_t);
extern void jet_rt_entry_error_exit(const Text *);
extern bool jet_rt_root_publish_rendered(const unsigned char *, int64_t, int64_t);
extern int64_t jet_scheduler_native_propagate(void);
extern int64_t jet_int_owned_from_i64(int64_t);
extern void jet_std_process_exit(int64_t);
extern void *jet_rt_alloc(size_t, size_t);
extern void jet_rt_free(void *, size_t, size_t);
extern void jet_rt_native_atexit(int64_t);
extern bool jet_stdio_stdout_tty_raw(void);

// A real closure in the canonical Jet exit queue; no manually invoked cleanup
// and no Rust unwind across this C callback. The production queue owns release.
static int64_t cleanup_body(int64_t closure) {
    (void)closure;
    static const unsigned char bytes[] = "cleanup";
    Text text = { bytes, 7, 0 };
    jet_term_write_stdout_line(&text, 0);
    return 0;
}

static int64_t cleanup_release(int64_t closure) {
    jet_rt_free((void *)(uintptr_t)closure, sizeof(ExitClosure), _Alignof(ExitClosure));
    return 0;
}

static void register_cleanup(void) {
    ExitClosure *closure = jet_rt_alloc(sizeof(ExitClosure), _Alignof(ExitClosure));
    closure->refs = 1;
    closure->body = cleanup_body;
    closure->release = cleanup_release;
    jet_rt_native_atexit((int64_t)(uintptr_t)closure);
}

static int mode;
// Canonical TrapMessages.jet header_panic + prelude_end + footer_e3001.
// The direct machine panic captures header/reason/prelude/footer as a form1
// E4 Root report; the Rust boundary prints those bytes without re-rendering.
static const char root_stop_report[] =
    "Stop [E3001]: `panic: stdio-root-stop`\n"
    "  --> <core.prelude>:0\n"
    " Why: The program hit a `panic`, `assert`, or `assert_eq` call that failed, "
    "a bounds/key check triggered at runtime, or another program-side stop. "
    "The shared report boundary owns the program-side stop; an unhandled entry "
    "error is a returned report, not E3001. Jet file and line are shown in Jet "
    "terms (I2).\n"
    " Fix: Fix the logic that led to the failure. Program-side stops exit 70 "
    "through the shared boundary; unhandled entry errors print their report "
    "and exit 1; exit 101 is reserved for Jet defects.\n"
    "More: jet-lang.dev/e/E3001\n";

static void entry(void) {
    if (mode != 4) register_cleanup();
    static const unsigned char body[] = "body";
    Text text = { body, 4, 0 };
    jet_term_write_stdout_line(&text, 0); // Below the pipe's 8192-byte buffer.
    if (mode == 0) return;
    if (mode == 1) {
        static const unsigned char reason[] = "stdio-root-stop";
        jet_rt_panic_message(reason, (int64_t)sizeof(reason) - 1);
    } else if (mode == 2) {
        static const unsigned char reason[] = "stdio-entry-error";
        Text error = { reason, (int64_t)sizeof(reason) - 1, 0 };
        jet_rt_entry_error_exit(&error);
    } else if (mode == 4) {
        jet_std_process_exit(jet_int_owned_from_i64(0));
        _exit(91); // The direct terminal provider must not return.
    } else {
        // A real nonzero Root record distinguishes explicit exit0 from a
        // successful return, even though there are no report bytes to borrow.
        static const unsigned char empty[] = "";
        assert(jet_rt_root_publish_rendered(empty, 0, 0));
    }
    // Consume the existing native propagation fact before any further body
    // work. No Rust unwind may cross this entry or a foreign callback frame.
    assert(jet_scheduler_native_propagate() != 0);
}

static void check(int selected, int expected_status, const char *report) {
    int stream[2];
    assert(pipe(stream) == 0);
    pid_t child = fork();
    assert(child >= 0);
    if (child == 0) {
        assert(close(stream[0]) == 0);
        assert(dup2(stream[1], STDOUT_FILENO) == STDOUT_FILENO);
        assert(dup2(stream[1], STDERR_FILENO) == STDERR_FILENO);
        assert(close(stream[1]) == 0);
        // The fixture runner pipes stdout before loader init arrays, so the
        // canonical cached TTY fact is false. Duplication alone cannot prove it.
        assert(!jet_stdio_stdout_tty_raw());
        jet_rt_host_init();
        mode = selected;
        if (selected == 4) {
            entry(); // No Root: exercise the provider's direct terminal edge.
            _exit(91);
        }
        int32_t status = jet_rt_main(entry);
        // Deliberately bypass libc and runtime fini arrays: ordinary Root
        // return must already have drained the canonical Jet stdout buffer.
        _exit(status);
    }
    assert(close(stream[1]) == 0);
    char output[16384];
    size_t used = 0;
    for (;;) {
        assert(used < sizeof(output) - 1);
        ssize_t count = read(stream[0], output + used, sizeof(output) - 1 - used);
        assert(count >= 0);
        if (count == 0) break;
        used += (size_t)count;
    }
    output[used] = '\0';
    assert(close(stream[0]) == 0);
    int status = 0;
    assert(waitpid(child, &status, 0) == child);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == expected_status);
    const char *prefix = selected == 4 ? "body\n" : "body\ncleanup\n";
    size_t prefix_length = strlen(prefix);
    assert(strncmp(output, prefix, prefix_length) == 0);
    if (report == NULL) {
        assert(strcmp(output, prefix) == 0);
    } else if (selected == 1) {
        assert(used == prefix_length + strlen(report));
        assert(memcmp(output + prefix_length, report, strlen(report)) == 0);
    } else {
        assert(strncmp(output + prefix_length, report, strlen(report)) == 0);
        assert(strstr(output + prefix_length + strlen(report), report) == NULL);
        assert(strstr(output, "panicked at") == NULL);
    }
}

int main(int argc, char **argv) {
    const bool established = argc == 2 && strcmp(argv[1], "--established") == 0;
    if (argc != 1 && !established) {
        fputs("usage: RootStdioWitness [--established]\n", stderr);
        return 2;
    }
    check(0, 0, NULL);
    check(1, 70, root_stop_report);
    // #4902 holds entry-Err atexit eligibility and cross-group report order.
    // The default full witness retains that disputed case unchanged. This
    // explicit preparation selector neither exercises nor claims that gate.
    if (!established) check(2, 1, "Error:");
    check(3, 0, NULL);
    check(4, 0, NULL);
    puts(established ? "root stdio established (mode2 held): ok" : "root stdio: ok");
    return 0;
}
