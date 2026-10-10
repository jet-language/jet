// Descriptor ABI witness linked against the actual planner-emitted musl archive.
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef struct { uint64_t value; unsigned char *ptr; size_t len; } JetFfiSlot;
_Static_assert(sizeof(JetFfiSlot) == 24, "native descriptor slot size");
_Static_assert(offsetof(JetFfiSlot, ptr) == 8, "native descriptor pointer offset");
_Static_assert(offsetof(JetFfiSlot, len) == 16, "native descriptor length offset");
#define DESCRIPTOR(name) extern int32_t jet_ffi_##name##_cabi(const JetFfiSlot *, size_t, JetFfiSlot *)
DESCRIPTOR(b64encode); DESCRIPTOR(identity_bool); DESCRIPTOR(identity_char);
DESCRIPTOR(identity_u32); DESCRIPTOR(identity_f64); DESCRIPTOR(identity_f32);
DESCRIPTOR(maximum); DESCRIPTOR(spin); DESCRIPTOR(panic_foreign);
extern void jet_ffi_cabi_free(unsigned char *, size_t);
extern int8_t jet_ffi_take_failure(void);
extern void jet_ffi_set_reporter(void (*)(const unsigned char *, size_t));
extern void jet_ffi_clear_panic_hook(void);
static unsigned reports;
static void report(const unsigned char *text, size_t len) {
    const char expected[] = "panic: a foreign function panicked";
    assert(len == sizeof(expected) - 1 && memcmp(text, expected, len) == 0);
    reports++;
}
static void string_call(const unsigned char *text, size_t len, const char *expected) {
    JetFfiSlot input = {0, (unsigned char *)text, len}, out = {0};
    assert(jet_ffi_b64encode_cabi(&input, 1, &out) == 0);
    assert(jet_ffi_take_failure() == 0);
    assert(out.len == strlen(expected) && memcmp(out.ptr, expected, out.len) == 0);
    jet_ffi_cabi_free(out.ptr, out.len);
}
int main(void) {
    string_call((const unsigned char *)"hi", 2, "aGk=");
    const unsigned char embedded[] = {'h', 0, 'i'};
    string_call(embedded, sizeof(embedded), "aABp");
    const unsigned char utf8[] = {'h', 0xc3, 0xa9, 'l', 'l', 'o'};
    string_call(utf8, sizeof(utf8), "aMOpbGxv");
    JetFfiSlot input = {2, NULL, 0}, out = {0};
    assert(jet_ffi_identity_bool_cabi(&input, 1, &out) == 0 && out.value == 1);
    input.value = 0xe9;
    assert(jet_ffi_identity_char_cabi(&input, 1, &out) == 0 && out.value == 0xe9);
    input.value = 0xfedcba98;
    assert(jet_ffi_identity_u32_cabi(&input, 1, &out) == 0 && out.value == input.value);
    double f64 = -2.5; memcpy(&input.value, &f64, sizeof(f64));
    assert(jet_ffi_identity_f64_cabi(&input, 1, &out) == 0 && out.value == input.value);
    float f32 = 1.25f; uint32_t f32_bits; memcpy(&f32_bits, &f32, sizeof(f32));
    input.value = f32_bits;
    assert(jet_ffi_identity_f32_cabi(&input, 1, &out) == 0 && out.value == f32_bits);
    JetFfiSlot pair[] = {{(uint64_t)-7, NULL, 0}, {(uint64_t)-3, NULL, 0}};
    assert(jet_ffi_maximum_cabi(pair, 2, &out) == 0 && (int64_t)out.value == -3);
    assert(jet_ffi_spin_cabi(&input, 0, &out) == 0);
    assert(jet_ffi_take_failure() == 0);
    assert(jet_ffi_b64encode_cabi(&input, 0, &out) == 1);
    assert(jet_ffi_take_failure() == 2 && jet_ffi_take_failure() == 0);
    jet_ffi_set_reporter(report);
    input.ptr = (unsigned char *)"foreign payload"; input.len = 15;
    assert(jet_ffi_panic_foreign_cabi(&input, 1, &out) == 1);
    assert(reports == 1 && jet_ffi_take_failure() == 1 && jet_ffi_take_failure() == 0);
    jet_ffi_clear_panic_hook();
    puts("native Rust helper ABI passed");
    return 0;
}
