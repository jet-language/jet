struct F { int fd; }; static struct F out1 = {1}; struct F *stdout = &out1; long __stdio_write(struct F *f, const char *s) { return f->fd + *s; }
