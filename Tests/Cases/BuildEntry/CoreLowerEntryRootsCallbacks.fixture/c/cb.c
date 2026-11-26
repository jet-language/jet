#include <stdint.h>
typedef int32_t (*cb_t)(int32_t);
int32_t call_twice(cb_t cb, int32_t x) { return cb(cb(x)); }
