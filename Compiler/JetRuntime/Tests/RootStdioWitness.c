// #4881: link only the freshly composed full Jet runtime pack. These entries
// are non-unwinding machine/C callers, not Rust panic or scheduler-test shims.
// A shared pipe observes the required buffered-stdout-before-report ordering.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

typedef struct { const unsigned char *ptr; int64_t len, cap; } Text;
_Static_assert(sizeof(Text) == 24, "E4 text header");
extern void jet_rt_host_init(void);
extern void jet_rt_stdout_flush(void);
extern int32_t jet_rt_main(void (*entry)(void));
extern void jet_term_write_stdout_line(const Text *, uint8_t);
extern void jet_rt_panic_message(const unsigned char *, int64_t);
extern void jet_rt_entry_error_exit(const Text *);
extern bool jet_rt_root_publish_rendered(const unsigned char *, int64_t, int64_t);
extern int64_t jet_scheduler_native_propagate(void);
extern int64_t jet_int_owned_from_i64(int64_t);
extern void jet_std_process_exit(int64_t);

static int mode;
static void entry(void) {
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
        // Entryless hosts may call terminal flushing before their init array.
        jet_rt_stdout_flush();
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
    assert(strncmp(output, "body\n", 5) == 0);
    if (report == NULL) {
        assert(strcmp(output, "body\n") == 0);
    } else {
        assert(strncmp(output + 5, report, strlen(report)) == 0);
        assert(strstr(output + 5 + strlen(report), report) == NULL);
        assert(strstr(output, "panicked at") == NULL);
    }
}

int main(void) {
    check(0, 0, NULL);
    check(1, 70, "Stop [E3001]: ");
    check(2, 1, "Error:");
    check(3, 0, NULL);
    check(4, 0, NULL);
    puts("root stdio: ok");
    return 0;
}
