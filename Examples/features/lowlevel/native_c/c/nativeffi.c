#include <stdint.h>
#include <stdbool.h>

int32_t jet_fixture_negate(int32_t value) { return -value; }
uint32_t jet_fixture_high_bits(void) { return UINT32_C(0xfedcba98); }
double jet_fixture_scale(double value) { return value * 2.0; }
void jet_fixture_write(int32_t *value) { *value = -1234567; }
void jet_fixture_float_write(double *value) { *value = 3.5; }
bool jet_fixture_bool(void) { return true; }
const char *jet_fixture_text(const char *value) { return value; }
