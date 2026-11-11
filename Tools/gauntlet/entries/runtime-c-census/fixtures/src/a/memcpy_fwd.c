void *__memcpy_fwd(void *d, const void *s, unsigned long n) { char *o = d; const char *i = s; while (n--) *o++ = *i++; return d; }
