/* Link with the native object exporting Fixtures/U32Import.jet's three
 * callers. This is an ABI boundary witness, not a replacement runtime.
 */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>

extern int64_t fixture_u32_zero_word(void);
extern int64_t fixture_u32_max_word(void);
extern int64_t fixture_u32_status_word(void);

/* SysV U32 return values define eax, NOT the upper half of rax. */
__asm__(
    ".text\n"
    ".globl fixture_dirty_u32_zero\n"
    ".type fixture_dirty_u32_zero,@function\n"
    "fixture_dirty_u32_zero:\n"
    "movabs $0xdeadbeef00000000,%rax\n"
    "ret\n"
    ".size fixture_dirty_u32_zero,.-fixture_dirty_u32_zero\n"
    ".globl fixture_dirty_u32_max\n"
    ".type fixture_dirty_u32_max,@function\n"
    "fixture_dirty_u32_max:\n"
    "movabs $0xdeadbeefffffffff,%rax\n"
    "ret\n"
    ".size fixture_dirty_u32_max,.-fixture_dirty_u32_max\n"
    ".globl fixture_dirty_u32_status\n"
    ".type fixture_dirty_u32_status,@function\n"
    "fixture_dirty_u32_status:\n"
    "movabs $0xdeadbeef00000003,%rax\n"
    "ret\n"
    ".size fixture_dirty_u32_status,.-fixture_dirty_u32_status\n"
);

int main(void) {
    assert(fixture_u32_zero_word() == INT64_C(0));
    assert(fixture_u32_max_word() == INT64_C(4294967295));
    assert(fixture_u32_status_word() == INT64_C(3));
    puts("CHECK OK — native U32 imports zero-extend dirty return bits: zero, max, comptime status");
    return 0;
}
