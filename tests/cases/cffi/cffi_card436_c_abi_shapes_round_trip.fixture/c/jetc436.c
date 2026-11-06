
#include <stdint.h>

/* D-SG9: fixed-width integers cross the C boundary as their exact C type. */
uint8_t jetc436_add_u8(uint8_t a, uint8_t b) { return (uint8_t)(a + b); }
int32_t jetc436_add_i32(int32_t a, int32_t b) { return a + b; }
float jetc436_add_f32(float a, float b) { return a + b; }

/* D-REPRC1: a `#Layout(c)` struct is `#[repr(C)]` — same field order/size as
 * this C struct, so it crosses by value with no bridging code needed. */
typedef struct {
    long long x;
    long long y;
} CPoint;

CPoint jetc436_make_point(long long x, long long y) {
    CPoint p;
    p.x = x;
    p.y = y;
    return p;
}

long long jetc436_point_sum(CPoint p) { return p.x + p.y; }

/* D-DIST1: a distinct type is `#[repr(transparent)]` over its base — same
 * ABI as the base scalar, so it crosses the boundary directly. */
long long jetc436_scale_meters(long long m) { return m * 2; }
