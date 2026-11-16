#include <stdint.h>
typedef void *gzFile;
gzFile gzopen(const char *path, const char *mode);
int gzread(gzFile file, char *buf, unsigned int len);
int gzclose(gzFile file);
