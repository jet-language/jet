long long strlib_slen(const char *s) { const char *p = s; while (*p) ++p; return (long long)(p - s); }
