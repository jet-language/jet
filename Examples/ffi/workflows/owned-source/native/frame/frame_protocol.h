#pragma once
/* Mirrors the generated Jet Library owner/view protocol (resource_protocol). */

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FrameDocument {
    uint64_t slot;
    uint64_t generation;
} FrameDocument;

typedef struct FrameView {
    uint64_t slot;
    uint64_t generation;
} FrameView;

typedef enum frameError {
    FRAME_OK = 0,
    FRAME_EXPIRED_VIEW = 1,
    FRAME_INVALID_HANDLE = 2,
    FRAME_OVERFLOW = 3,
    FRAME_ENCODING = 4,
    FRAME_CLOSED = 5
} frameError;

int frame_document_open(const uint8_t *input, size_t len, FrameDocument *out);
int frame_document_bytes(const FrameDocument *owner, FrameView *out);
int frame_view_at(const FrameView *view, size_t index, uint8_t *out);
int frame_document_replace(FrameDocument *owner, const uint8_t *input, size_t len);
int frame_document_close(FrameDocument *owner);

#ifdef __cplusplus
}
#endif
