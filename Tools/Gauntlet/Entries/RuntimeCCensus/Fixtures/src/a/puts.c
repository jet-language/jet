struct F; extern struct F *stdout; extern int fputs(const char *, struct F *); int puts(const char *s) { return fputs(s, stdout); }
