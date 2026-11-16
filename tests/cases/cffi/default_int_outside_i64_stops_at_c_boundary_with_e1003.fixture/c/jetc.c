
#include <stdint.h>
static long long jetc_value = 0;
void jetc_reset(void) { jetc_value = 7; }
long long jetc_value_get(void) { return jetc_value; }
void jetc_store(long long value) { jetc_value = value; }
long long jetc_twice(long long value) { return value * 2; }
long long jetc_add_ints(long long a, long long b) { return a + b; }
double jetc_half(double value) { return value / 2.0; }
double jetc_sum6(double a, double b, double c, double d, double e, double f) {
    return a + b + c + d + e + f;
}
const char *jetc_echo(const char *value) { return value; }
const char *jetc_greeting(void) { return "hi from C"; }
