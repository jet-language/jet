// Foreign caller of the actual checked MIR .Global export fixture.
#include <assert.h>
#include <stdint.h>
extern void native_global_store(int64_t value);
extern int64_t native_global_load(void);
extern double runtime_float_identity(double value);
int main(void) {
    assert(native_global_load() == 0);
    const int64_t values[] = {-7, INT64_MAX, INT64_MIN, 0};
    for (unsigned i = 0; i < sizeof values / sizeof values[0]; ++i) {
        native_global_store(values[i]);
        assert(native_global_load() == values[i]);
    }
    const uint64_t float_bits[] = {
        UINT64_C(0), UINT64_C(0x8000000000000000), UINT64_C(0x3ff8000000000000),
        UINT64_C(0x7ff0000000000000), UINT64_C(0x7ff8000000000042)
    };
    for (unsigned i = 0; i < sizeof float_bits / sizeof float_bits[0]; ++i) {
        union { uint64_t bits; double value; } input = {.bits = float_bits[i]}, result;
        result.value = runtime_float_identity(input.value);
        assert(result.bits == input.bits);
    }
    return 0;
}
