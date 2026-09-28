/* Minimal HarfBuzz ABI surface used by the opaque-handle example. */
typedef struct hb_blob_t hb_blob_t;
typedef struct hb_face_t hb_face_t;
typedef struct hb_font_t hb_font_t;
typedef struct hb_buffer_t hb_buffer_t;

typedef unsigned int hb_codepoint_t;

typedef struct hb_feature_t {
    unsigned int tag;
    unsigned int value;
    unsigned int start;
    unsigned int end;
} hb_feature_t;

typedef struct hb_glyph_info_t {
    unsigned int codepoint;
    unsigned int mask;
    unsigned int cluster;
    unsigned int var1;
    unsigned int var2;
} hb_glyph_info_t;

typedef struct hb_glyph_position_t {
    int x_advance;
    int y_advance;
    int x_offset;
    int y_offset;
    unsigned int var;
} hb_glyph_position_t;

hb_blob_t *hb_blob_create_from_file(const char *file_name);
hb_face_t *hb_face_create(hb_blob_t *blob, unsigned int index);
hb_font_t *hb_font_create(hb_face_t *face);
void hb_font_set_scale(hb_font_t *font, int x_scale, int y_scale);

hb_buffer_t *hb_buffer_create(void);
void hb_buffer_add_codepoints(
    hb_buffer_t *buffer,
    const hb_codepoint_t *text,
    int text_length,
    unsigned int item_offset,
    int item_length
);
void hb_buffer_guess_segment_properties(hb_buffer_t *buffer);
void hb_shape(
    hb_font_t *font,
    hb_buffer_t *buffer,
    const hb_feature_t *features,
    unsigned int num_features
);
unsigned int hb_buffer_get_length(hb_buffer_t *buffer);
const hb_glyph_info_t *hb_buffer_get_glyph_infos(
    hb_buffer_t *buffer,
    unsigned int *length
);
const hb_glyph_position_t *hb_buffer_get_glyph_positions(
    hb_buffer_t *buffer,
    unsigned int *length
);

void hb_font_destroy(hb_font_t *font);
void hb_face_destroy(hb_face_t *face);
void hb_blob_destroy(hb_blob_t *blob);
void hb_buffer_destroy(hb_buffer_t *buffer);
