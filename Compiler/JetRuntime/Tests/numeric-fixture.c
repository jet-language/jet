/* Link with the composed native runtime. Tests the native LowerRoute533 ABI. */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern double jet_numeric_checked_widen_at(int64_t, bool, bool, int64_t, int64_t);
extern int64_t jet_rt_string_from_static(int64_t, int64_t);
extern void jet_rt_string_drop(int64_t);

static void rejected(uint64_t raw, bool sign, bool f32, int64_t file) {
    int output[2];
    assert(pipe(output) == 0);
    pid_t child = fork();
    assert(child >= 0);
    if (child == 0) {
        assert(close(output[0]) == 0);
        assert(dup2(output[1], STDERR_FILENO) == STDERR_FILENO);
        assert(close(output[1]) == 0);
        (void)jet_numeric_checked_widen_at((int64_t)raw, sign, f32, file, 27);
        _exit(99);
    }
    assert(close(output[1]) == 0);
    char text[4096] = {0};
    size_t at = 0;
    for (;;) {
        ssize_t count = read(output[0], text + at, sizeof text - 1 - at);
        assert(count >= 0);
        if (count == 0) break;
        at += (size_t)count;
        assert(at < sizeof text - 1);
    }
    assert(close(output[0]) == 0);
    int status;
    assert(waitpid(child, &status, 0) == child);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 70);
    assert(strstr(text, "whole number cannot cross into the decimal without losing precision"));
    assert(strstr(text, "numeric-witness.jet:27"));
}

int main(void) {
    static const char name[] = "numeric-witness.jet";
    int64_t file = jet_rt_string_from_static((int64_t)(intptr_t)name, sizeof name - 1);
    for (unsigned target = 0; target < 2; ++target) {
        bool f32 = target != 0;
        uint64_t boundary = UINT64_C(1) << (f32 ? 24 : 53);
        for (unsigned signedness = 0; signedness < 2; ++signedness) {
            bool sign = signedness != 0;
            assert(jet_numeric_checked_widen_at((int64_t)(boundary - 1), sign, f32, file, 27) == (double)(boundary - 1));
            assert(jet_numeric_checked_widen_at((int64_t)boundary, sign, f32, file, 27) == (double)boundary);
            assert(jet_numeric_checked_widen_at((int64_t)(boundary + 2), sign, f32, file, 27) == (double)(boundary + 2));
            rejected(boundary + 1, sign, f32, file);
            if (sign) {
                assert(jet_numeric_checked_widen_at(-(int64_t)(boundary - 1), true, f32, file, 27) == -(double)(boundary - 1));
                assert(jet_numeric_checked_widen_at(-(int64_t)boundary, true, f32, file, 27) == -(double)boundary);
                rejected((uint64_t)(-(int64_t)(boundary + 1)), true, f32, file);
            }
        }
    }
    assert(jet_numeric_checked_widen_at(INT64_MIN, true, false, file, 27) == -9223372036854775808.0);
    assert(jet_numeric_checked_widen_at(INT64_MIN, false, false, file, 27) == 9223372036854775808.0);
    rejected(UINT64_MAX, false, false, file);
    jet_rt_string_drop(file);
    puts("CHECK OK — native widening: signed/unsigned precision24/53 boundaries, file:line rejection, full-width signs");
    return 0;
}
