/* The second object: defines what the first references, references strlen. */
extern unsigned long strlen(const char *);

int shared_def(const char *s) { return (int)strlen(s); }
