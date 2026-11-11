#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* The Jet struct `Mixed` of main.jet, which has no #Layout, as the compiler
   places it: fields by descending alignment (declaration order among equals),
   then padding to a multiple of 8. main.jet checks every fact below against
   Mixed.reflect().layout before it passes a Mixed. */
typedef struct {
    int64_t b;
    int64_t d;
    uint8_t a;
    uint8_t c;
    uint8_t e;
    uint8_t padding[5];
} Mixed;

int64_t jet_mixed_layout_fact(int64_t which) {
    switch (which) {
    case 0: return (int64_t)sizeof(Mixed);
    case 1: return (int64_t)_Alignof(Mixed);
    case 2: return (int64_t)offsetof(Mixed, a);
    case 3: return (int64_t)offsetof(Mixed, b);
    case 4: return (int64_t)offsetof(Mixed, c);
    case 5: return (int64_t)offsetof(Mixed, d);
    case 6: return (int64_t)offsetof(Mixed, e);
    default: return -1;
    }
}

void jet_mixed_bump(Mixed *value) {
    value->a += 1;
    value->b *= 2;
    value->c += 2;
    value->d -= 5;
    value->e += 3;
}

int64_t jet_mixed_sum(Mixed *value) {
    return (int64_t)value->a + value->b + (int64_t)value->c + value->d + (int64_t)value->e;
}

static Mixed *remembered;

void jet_mixed_remember(Mixed *value) { remembered = value; }

bool jet_mixed_same(Mixed *value) { return value == remembered; }
