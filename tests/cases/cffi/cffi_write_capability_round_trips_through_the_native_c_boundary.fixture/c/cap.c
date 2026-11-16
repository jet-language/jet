#include <stdint.h>
typedef struct { int64_t x; int64_t y; } CapPair;
void jet_cap_increment(int64_t *value) { *value += 1; }
CapPair jet_cap_make(void) { return (CapPair){20, 22}; }
int64_t jet_cap_consume(CapPair value) { return value.x + value.y; }
