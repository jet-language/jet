#include <hb-ot.h>
#include <hb.h>

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

/*
 * Jet's browser bridge keeps the shaping policy in HostServices.rs.  This
 * module exposes only the native HarfBuzz object API over a numeric Wasm ABI.
 * Ui.js copies app-memory bytes into this module and treats returned pointers
 * as opaque HarfBuzz-owned handles or arrays.
 */

enum {
  JET_HB_ABI_VERSION = 2,
};

uint32_t jet_hb_abi_version(void) { return JET_HB_ABI_VERSION; }

uint32_t jet_hb_alloc(uint32_t size) {
  if (size == 0) return 0;
  return (uint32_t)(uintptr_t)malloc(size);
}

void jet_hb_free(uint32_t pointer) {
  if (pointer != 0) free((void *)(uintptr_t)pointer);
}

static void jet_hb_blob_release(void *data) { free(data); }

/*
 * The Rust declaration mirrors hb_blob_create's five-argument shape.  The
 * browser adapter supplies a copied buffer and the bridge owns its release;
 * user_data/destroy are intentionally ignored because the shared kernel
 * passes null for both when creating a vetted bundled-font blob.
 */
uint32_t jet_hb_blob_create(uint32_t data_pointer, uint32_t length,
                            uint32_t memory_mode, uint32_t user_data,
                            uint32_t destroy) {
  (void)user_data;
  (void)destroy;
  if (data_pointer == 0 || length == 0) return 0;
  uint8_t *copy = (uint8_t *)malloc(length);
  if (copy == NULL) return 0;
  memcpy(copy, (const void *)(uintptr_t)data_pointer, length);
  hb_blob_t *blob = hb_blob_create(
      (const char *)copy, length, (hb_memory_mode_t)memory_mode, copy,
      jet_hb_blob_release);
  if (blob == NULL) free(copy);
  return (uint32_t)(uintptr_t)blob;
}

void jet_hb_blob_destroy(uint32_t blob) {
  if (blob != 0) hb_blob_destroy((hb_blob_t *)(uintptr_t)blob);
}

uint32_t jet_hb_face_create(uint32_t blob, uint32_t index) {
  if (blob == 0) return 0;
  return (uint32_t)(uintptr_t)hb_face_create((hb_blob_t *)(uintptr_t)blob, index);
}

uint32_t jet_hb_face_get_glyph_count(uint32_t face) {
  if (face == 0) return 0;
  return hb_face_get_glyph_count((hb_face_t *)(uintptr_t)face);
}

void jet_hb_face_destroy(uint32_t face) {
  if (face != 0) hb_face_destroy((hb_face_t *)(uintptr_t)face);
}

uint32_t jet_hb_font_create(uint32_t face) {
  if (face == 0) return 0;
  return (uint32_t)(uintptr_t)hb_font_create((hb_face_t *)(uintptr_t)face);
}

void jet_hb_font_destroy(uint32_t font) {
  if (font != 0) hb_font_destroy((hb_font_t *)(uintptr_t)font);
}

void jet_hb_font_set_scale(uint32_t font, int32_t x_scale, int32_t y_scale) {
  if (font != 0) hb_font_set_scale((hb_font_t *)(uintptr_t)font, x_scale, y_scale);
}

void jet_hb_ot_font_set_funcs(uint32_t font) {
  if (font != 0) hb_ot_font_set_funcs((hb_font_t *)(uintptr_t)font);
}

uint32_t jet_hb_buffer_create(void) {
  return (uint32_t)(uintptr_t)hb_buffer_create();
}

void jet_hb_buffer_destroy(uint32_t buffer) {
  if (buffer != 0) hb_buffer_destroy((hb_buffer_t *)(uintptr_t)buffer);
}

void jet_hb_buffer_add_utf8(uint32_t buffer, uint32_t text_pointer,
                            int32_t text_length, uint32_t item_offset,
                            int32_t item_length) {
  if (buffer == 0 || text_pointer == 0 || text_length < 0) return;
  hb_buffer_add_utf8((hb_buffer_t *)(uintptr_t)buffer,
                     (const char *)(uintptr_t)text_pointer, text_length,
                     item_offset, item_length);
}

void jet_hb_buffer_guess_segment_properties(uint32_t buffer) {
  if (buffer != 0) {
    hb_buffer_guess_segment_properties((hb_buffer_t *)(uintptr_t)buffer);
  }
}

void jet_hb_shape(uint32_t font, uint32_t buffer, uint32_t features_pointer,
                  uint32_t feature_count) {
  (void)features_pointer;
  if (font != 0 && buffer != 0) {
    hb_shape((hb_font_t *)(uintptr_t)font, (hb_buffer_t *)(uintptr_t)buffer,
             NULL, feature_count);
  }
}

uint32_t jet_hb_buffer_get_length(uint32_t buffer) {
  if (buffer == 0) return 0;
  return hb_buffer_get_length((hb_buffer_t *)(uintptr_t)buffer);
}

uint32_t jet_hb_buffer_get_glyph_infos(uint32_t buffer, uint32_t length_pointer) {
  if (buffer == 0 || length_pointer == 0) return 0;
  unsigned int length = 0;
  const hb_glyph_info_t *infos = hb_buffer_get_glyph_infos(
      (hb_buffer_t *)(uintptr_t)buffer, &length);
  *(unsigned int *)(uintptr_t)length_pointer = length;
  return (uint32_t)(uintptr_t)infos;
}

uint32_t jet_hb_buffer_get_glyph_positions(uint32_t buffer,
                                           uint32_t length_pointer) {
  if (buffer == 0 || length_pointer == 0) return 0;
  unsigned int length = 0;
  const hb_glyph_position_t *positions = hb_buffer_get_glyph_positions(
      (hb_buffer_t *)(uintptr_t)buffer, &length);
  *(unsigned int *)(uintptr_t)length_pointer = length;
  return (uint32_t)(uintptr_t)positions;
}
