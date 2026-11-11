/* The census's first object: strong references that the archives resolve
   (memcpy, puts), one the second object defines (shared_def), one nothing
   defines (missing_sym), a weak reference (weak_thing) and a local helper. */
extern void *memcpy(void *, const void *, unsigned long);
extern int puts(const char *);
extern int shared_def(int);
extern int missing_sym(void);
extern void weak_thing(void) __attribute__((weak));

static int helper_local(int x) { return x * 3; }

int jet_main(char *out, const char *in, unsigned long n) {
    memcpy(out, in, n);
    if (weak_thing) weak_thing();
    puts(out);
    return shared_def(helper_local(missing_sym()));
}
