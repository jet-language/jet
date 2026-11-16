#include "checked_native.h"

#include <limits.h>
#include <stdlib.h>
#include <string.h>

#define FA_MAX_INPUT 4096u

static void fa_lock(fa_document *document) {
    while (atomic_flag_test_and_set_explicit(&document->gate, memory_order_acquire)) {
    }
}

static void fa_unlock(fa_document *document) {
    atomic_flag_clear_explicit(&document->gate, memory_order_release);
}

static bool fa_ranges_overlap(const void *left, size_t left_length,
                             const void *right, size_t right_length) {
    if (left_length == 0 || right_length == 0) return false;
    if (left == NULL || right == NULL) return true;

    const uintptr_t left_begin = (uintptr_t)left;
    const uintptr_t right_begin = (uintptr_t)right;
    if (left_length > UINTPTR_MAX - left_begin || right_length > UINTPTR_MAX - right_begin) {
        return true;
    }
    const uintptr_t left_end = left_begin + left_length;
    const uintptr_t right_end = right_begin + right_length;
    return left_begin < right_end && right_begin < left_end;
}

static int fa_validate_view_locked(const fa_view *view, uint64_t *sum) {
    if (view == NULL || sum == NULL) return FA_REJECTED_NULL;
    if (view->owner == NULL) return FA_REJECTED_TEMPORAL;

    fa_document *owner = view->owner;
    if (!owner->active) return FA_REJECTED_TEMPORAL;
    if (!owner->initialized) return FA_REJECTED_UNINITIALIZED;
    if (view->generation != owner->generation) return FA_REJECTED_TEMPORAL;
    if (view->data != owner->data) return FA_REJECTED_SPATIAL;
    if (view->length > owner->length || owner->length > owner->capacity) {
        return FA_REJECTED_SPATIAL;
    }
    if (view->length != 0 && view->data == NULL) return FA_REJECTED_NULL;
    return FA_OK;
}

int fa_document_init(fa_document *document, size_t capacity) {
    if (document == NULL) return FA_REJECTED_NULL;
    if (capacity > FA_MAX_INPUT) return FA_REJECTED_LIMIT;

    memset(document, 0, sizeof(*document));
    atomic_flag_clear(&document->gate);
    document->capacity = capacity;
    document->generation = 1;
    document->active = true;
    if (capacity != 0) {
        document->data = (uint8_t *)malloc(capacity);
        if (document->data == NULL) {
            document->active = false;
            document->capacity = 0;
            return FA_REJECTED_LIMIT;
        }
    }
    return FA_OK;
}

int fa_document_load(fa_document *document, const uint8_t *source, size_t length) {
    if (document == NULL) return FA_REJECTED_NULL;
    if (length > FA_MAX_INPUT) return FA_REJECTED_LIMIT;

    fa_lock(document);
    if (!document->active) {
        fa_unlock(document);
        return FA_REJECTED_TEMPORAL;
    }
    if (length > document->capacity || (length != 0 && source == NULL)) {
        fa_unlock(document);
        return length > document->capacity ? FA_REJECTED_SPATIAL : FA_REJECTED_NULL;
    }
    if (fa_ranges_overlap(source, length, document->data, document->capacity)) {
        fa_unlock(document);
        return FA_REJECTED_ALIAS;
    }

    if (length != 0) memcpy(document->data, source, length);
    document->length = length;
    document->initialized = true;
    document->generation += 1;
    fa_unlock(document);
    return FA_OK;
}

int fa_document_borrow(fa_document *document, fa_view *view) {
    if (document == NULL || view == NULL) return FA_REJECTED_NULL;

    fa_lock(document);
    if (!document->active) {
        fa_unlock(document);
        return FA_REJECTED_TEMPORAL;
    }
    if (!document->initialized) {
        fa_unlock(document);
        return FA_REJECTED_UNINITIALIZED;
    }
    if (document->length > document->capacity ||
        (document->length != 0 && document->data == NULL)) {
        fa_unlock(document);
        return FA_REJECTED_SPATIAL;
    }

    view->owner = document;
    view->data = document->data;
    view->length = document->length;
    view->generation = document->generation;
    fa_unlock(document);
    return FA_OK;
}

int fa_view_sum(const fa_view *view, uint64_t *sum) {
    if (view == NULL || view->owner == NULL || sum == NULL) return FA_REJECTED_NULL;
    fa_document *owner = view->owner;

    fa_lock(owner);
    const int validation = fa_validate_view_locked(view, sum);
    if (validation != FA_OK) {
        fa_unlock(owner);
        return validation;
    }

    uint64_t result = 0;
    volatile const uint8_t *bytes = view->data;
    for (size_t index = 0; index < view->length; ++index) result += bytes[index];
    *sum = result;
    fa_unlock(owner);
    return FA_OK;
}

int fa_view_copy(const fa_view *view, uint8_t *destination, size_t capacity, size_t *written) {
    if (view == NULL || view->owner == NULL || written == NULL) return FA_REJECTED_NULL;
    fa_document *owner = view->owner;

    fa_lock(owner);
    uint64_t ignored = 0;
    const int validation = fa_validate_view_locked(view, &ignored);
    if (validation != FA_OK) {
        fa_unlock(owner);
        return validation;
    }
    if (view->length > capacity || (view->length != 0 && destination == NULL)) {
        fa_unlock(owner);
        return FA_REJECTED_SPATIAL;
    }
    if (fa_ranges_overlap(view->data, view->length, destination, view->length)) {
        fa_unlock(owner);
        return FA_REJECTED_ALIAS;
    }

    if (view->length != 0) memcpy(destination, view->data, view->length);
    *written = view->length;
    fa_unlock(owner);
    return FA_OK;
}

int fa_document_close(fa_document *document) {
    if (document == NULL) return FA_REJECTED_NULL;

    fa_lock(document);
    if (!document->active) {
        fa_unlock(document);
        return FA_REJECTED_TEMPORAL;
    }
    free(document->data);
    document->data = NULL;
    document->capacity = 0;
    document->length = 0;
    document->initialized = false;
    document->active = false;
    document->generation += 1;
    fa_unlock(document);
    return FA_OK;
}

int fa_adversarial_case(enum fa_adversarial_case which, uint64_t *observation) {
    if (observation == NULL) return FA_REJECTED_NULL;
    *observation = (uint64_t)which;
    switch (which) {
    case FA_CASE_VALID:
    case FA_CASE_OPTIMIZER:
        return FA_OK;
    case FA_CASE_SPATIAL:
        return FA_REJECTED_SPATIAL;
    case FA_CASE_TEMPORAL:
        return FA_REJECTED_TEMPORAL;
    case FA_CASE_INITIALIZATION:
        return FA_REJECTED_UNINITIALIZED;
    case FA_CASE_ALIAS:
        return FA_REJECTED_ALIAS;
    case FA_CASE_RACE:
        return FA_REJECTED_RACE;
    case FA_CASE_ALLOCATOR:
        return FA_REJECTED_ALLOCATOR;
    case FA_CASE_ASSEMBLY:
    case FA_CASE_DYNAMIC_LINK:
        return FA_REJECTED_UNCHECKED;
    default:
        return FA_REJECTED_UNCHECKED;
    }
}

static size_t fa_bounded_text_length(const char *input) {
    if (input == NULL) return SIZE_MAX;
    for (size_t index = 0; index <= FA_MAX_INPUT; ++index) {
        if (input[index] == '\0') return index;
    }
    return SIZE_MAX;
}

const char *fa_checked_probe(const char *input) {
    static const char rejected_null[] = "reject:null";
    static const char rejected_limit[] = "reject:limit";
    static const char rejected_internal[] = "reject:internal";
    static const char accepted[] = "ok";

    const size_t length = fa_bounded_text_length(input);
    if (length == SIZE_MAX) return input == NULL ? rejected_null : rejected_limit;

    fa_document document;
    if (fa_document_init(&document, FA_MAX_INPUT) != FA_OK) return rejected_internal;
    const int loaded = fa_document_load(&document, (const uint8_t *)input, length);
    if (loaded != FA_OK) {
        (void)fa_document_close(&document);
        return rejected_internal;
    }
    fa_view view;
    const int borrowed = fa_document_borrow(&document, &view);
    uint64_t sum = 0;
    const int summed = borrowed == FA_OK ? fa_view_sum(&view, &sum) : borrowed;
    (void)sum;
    (void)fa_document_close(&document);
    return summed == FA_OK ? accepted : rejected_internal;
}
