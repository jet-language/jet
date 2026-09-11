#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <zlib.h>

typedef struct {
    gzFile file;
    char buffer[256];
} JetZReader;

int64_t jet_z_open(const char *path) {
    JetZReader *reader = (JetZReader *)calloc(1, sizeof(JetZReader));
    if (reader == NULL) return 0;
    reader->file = gzopen(path, "rb");
    if (reader->file == NULL) {
        free(reader);
        return 0;
    }
    return (int64_t)(intptr_t)reader;
}

int64_t jet_z_read_count(int64_t handle) {
    JetZReader *reader = (JetZReader *)(intptr_t)handle;
    if (reader == NULL || reader->file == NULL) return -1;
    if (gzgets(reader->file, reader->buffer, (int)sizeof(reader->buffer)) == NULL) return -1;
    return (int64_t)strlen(reader->buffer);
}

void jet_z_close(int64_t handle) {
    JetZReader *reader = (JetZReader *)(intptr_t)handle;
    if (reader == NULL) return;
    if (reader->file != NULL) gzclose(reader->file);
    free(reader);
}
