extern void *__memcpy_fwd(void *, const void *, unsigned long); void *memcpy(void *d, const void *s, unsigned long n) { return __memcpy_fwd(d, s, n); }
