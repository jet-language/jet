#ifndef JET_FFI_ASSURANCE_CHECKED_NATIVE_H
#define JET_FFI_ASSURANCE_CHECKED_NATIVE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdatomic.h>

#ifdef __cplusplus
extern "C" {
#endif

enum fa_status {
    FA_OK = 0,
    FA_REJECTED_NULL = 1,
    FA_REJECTED_SPATIAL = 2,
    FA_REJECTED_TEMPORAL = 3,
    FA_REJECTED_UNINITIALIZED = 4,
    FA_REJECTED_ALIAS = 5,
    FA_REJECTED_RACE = 6,
    FA_REJECTED_ALLOCATOR = 7,
    FA_REJECTED_UNCHECKED = 8,
    FA_REJECTED_PROTOCOL = 9,
    FA_REJECTED_LIMIT = 10,
};

enum fa_adversarial_case {
    FA_CASE_VALID = 0,
    FA_CASE_SPATIAL = 1,
    FA_CASE_TEMPORAL = 2,
    FA_CASE_INITIALIZATION = 3,
    FA_CASE_ALIAS = 4,
    FA_CASE_RACE = 5,
    FA_CASE_OPTIMIZER = 6,
    FA_CASE_ASSEMBLY = 7,
    FA_CASE_DYNAMIC_LINK = 8,
    FA_CASE_ALLOCATOR = 9,
};

typedef struct fa_document {
    uint8_t *data;
    size_t capacity;
    size_t length;
    uint64_t generation;
    bool initialized;
    bool active;
    atomic_flag gate;
} fa_document;

typedef struct fa_view {
    fa_document *owner;
    const uint8_t *data;
    size_t length;
    uint64_t generation;
} fa_view;

/* The C implementation is foreign to Jet. The caller must use these checked
 * entry points; raw data pointers are never an ordinary-facade contract. */
int fa_document_init(fa_document *document, size_t capacity);
int fa_document_load(fa_document *document, const uint8_t *source, size_t length);
int fa_document_borrow(fa_document *document, fa_view *view);
int fa_view_sum(const fa_view *view, uint64_t *sum);
int fa_view_copy(const fa_view *view, uint8_t *destination, size_t capacity, size_t *written);
int fa_document_close(fa_document *document);

/* Deliberately non-admitted edges return a typed rejection without touching a
 * foreign pointer. This keeps raw assembly/allocator/linker paths observable
 * instead of silently labelling them safe. */
int fa_adversarial_case(enum fa_adversarial_case which, uint64_t *observation);

/* Stable scalar/string bridge used by the safe Jet caller fixture. */
const char *fa_checked_probe(const char *input);

#ifdef __cplusplus
}
#endif

#endif
