#include <stddef.h>
long long jetc436_strlen(const char *s) { const char *p = s; while (*p) ++p; return (long long)(p - s); }
