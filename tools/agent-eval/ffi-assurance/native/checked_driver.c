#include "checked_native.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <threads.h>

struct race_context {
    fa_document *document;
    const uint8_t *source;
    size_t length;
    atomic_int failures;
};

static void record_failure(struct race_context *context) {
    (void)atomic_fetch_add_explicit(&context->failures, 1, memory_order_relaxed);
}

static int failure_count(const struct race_context *context) {
    return atomic_load_explicit(&context->failures, memory_order_relaxed);
}

static const char *status_name(int status) {
    switch (status) {
    case FA_OK: return "ok";
    case FA_REJECTED_NULL: return "rejected-null";
    case FA_REJECTED_SPATIAL: return "rejected-spatial";
    case FA_REJECTED_TEMPORAL: return "rejected-temporal";
    case FA_REJECTED_UNINITIALIZED: return "rejected-uninitialized";
    case FA_REJECTED_ALIAS: return "rejected-alias";
    case FA_REJECTED_RACE: return "rejected-race";
    case FA_REJECTED_ALLOCATOR: return "rejected-allocator";
    case FA_REJECTED_UNCHECKED: return "rejected-unchecked";
    case FA_REJECTED_PROTOCOL: return "rejected-protocol";
    case FA_REJECTED_LIMIT: return "rejected-limit";
    default: return "unknown";
    }
}

static int mutator(void *raw) {
    struct race_context *context = (struct race_context *)raw;
    for (int iteration = 0; iteration < 256; ++iteration) {
        if (fa_document_load(context->document, context->source, context->length) != FA_OK) {
            record_failure(context);
        }
    }
    return 0;
}

static int run_valid(void) {
    static const uint8_t payload[] = {1, 2, 3, 4, 5};
    fa_document document;
    fa_view view;
    uint64_t sum = 0;
    int status = fa_document_init(&document, sizeof(payload));
    if (status == FA_OK) status = fa_document_load(&document, payload, sizeof(payload));
    if (status == FA_OK) status = fa_document_borrow(&document, &view);
    if (status == FA_OK) status = fa_view_sum(&view, &sum);
    (void)fa_document_close(&document);
    if (status != FA_OK || sum != 15) {
        printf("case=valid status=%s sum=%llu\n", status_name(status), (unsigned long long)sum);
        return 1;
    }
    printf("case=valid status=ok sum=%llu\n", (unsigned long long)sum);
    return 0;
}

static int run_spatial(void) {
    static const uint8_t payload[] = {9, 8, 7};
    fa_document document;
    fa_view view;
    uint64_t sum = 0;
    int status = fa_document_init(&document, sizeof(payload));
    if (status == FA_OK) status = fa_document_load(&document, payload, sizeof(payload));
    if (status == FA_OK) status = fa_document_borrow(&document, &view);
    if (status == FA_OK) {
        view.length = document.capacity + 1;
        status = fa_view_sum(&view, &sum);
    }
    (void)fa_document_close(&document);
    printf("case=spatial status=%s\n", status_name(status));
    return status == FA_REJECTED_SPATIAL ? 0 : 1;
}

static int run_temporal(void) {
    static const uint8_t before[] = {1, 1, 2, 3};
    static const uint8_t after[] = {8, 13};
    fa_document document;
    fa_view stale;
    uint64_t sum = 0;
    int status = fa_document_init(&document, 8);
    if (status == FA_OK) status = fa_document_load(&document, before, sizeof(before));
    if (status == FA_OK) status = fa_document_borrow(&document, &stale);
    if (status == FA_OK) status = fa_document_load(&document, after, sizeof(after));
    if (status == FA_OK) status = fa_view_sum(&stale, &sum);
    (void)fa_document_close(&document);
    printf("case=temporal status=%s\n", status_name(status));
    return status == FA_REJECTED_TEMPORAL ? 0 : 1;
}

static int run_initialization(void) {
    fa_document document;
    fa_view view;
    int status = fa_document_init(&document, 8);
    if (status == FA_OK) status = fa_document_borrow(&document, &view);
    (void)fa_document_close(&document);
    printf("case=initialization status=%s\n", status_name(status));
    return status == FA_REJECTED_UNINITIALIZED ? 0 : 1;
}

static int run_alias(void) {
    static const uint8_t payload[] = {2, 4, 6, 8};
    fa_document document;
    fa_view view;
    size_t written = 0;
    int status = fa_document_init(&document, sizeof(payload));
    if (status == FA_OK) status = fa_document_load(&document, payload, sizeof(payload));
    if (status == FA_OK) status = fa_document_borrow(&document, &view);
    if (status == FA_OK) status = fa_view_copy(&view, document.data, document.capacity, &written);
    (void)fa_document_close(&document);
    printf("case=alias status=%s written=%llu\n", status_name(status), (unsigned long long)written);
    return status == FA_REJECTED_ALIAS ? 0 : 1;
}

static int run_race(void) {
    static const uint8_t payload[] = {3, 1, 4, 1, 5, 9};
    fa_document document;
    struct race_context context = {0};
    thrd_t worker;
    int status = fa_document_init(&document, sizeof(payload));
    if (status == FA_OK) status = fa_document_load(&document, payload, sizeof(payload));
    context.document = &document;
    context.source = payload;
    context.length = sizeof(payload);
    if (status == FA_OK && thrd_create(&worker, mutator, &context) != thrd_success) status = FA_REJECTED_RACE;
    if (status == FA_OK) {
        for (int iteration = 0; iteration < 256; ++iteration) {
            fa_view view;
            uint64_t sum = 0;
            const int borrowed = fa_document_borrow(&document, &view);
            if (borrowed == FA_OK) {
                const int summed = fa_view_sum(&view, &sum);
                if (summed != FA_OK && summed != FA_REJECTED_TEMPORAL) record_failure(&context);
            } else if (borrowed != FA_OK) {
                record_failure(&context);
            }
        }
        (void)thrd_join(worker, NULL);
    }
    (void)fa_document_close(&document);
    const int failures = failure_count(&context);
    printf("case=race status=%s rejected=%d\n", status_name(status), failures);
    return status == FA_OK && failures == 0 ? 0 : 1;
}

static int run_typed_rejection(enum fa_adversarial_case which, const char *name, int expected) {
    uint64_t observation = 0;
    const int status = fa_adversarial_case(which, &observation);
    printf("case=%s status=%s observation=%llu\n", name, status_name(status), (unsigned long long)observation);
    return status == expected ? 0 : 1;
}

static int run_case(const char *name) {
    if (strcmp(name, "valid") == 0) return run_valid();
    if (strcmp(name, "spatial") == 0) return run_spatial();
    if (strcmp(name, "temporal") == 0) return run_temporal();
    if (strcmp(name, "initialization") == 0) return run_initialization();
    if (strcmp(name, "alias") == 0) return run_alias();
    if (strcmp(name, "race") == 0) return run_race();
    if (strcmp(name, "optimizer") == 0) return run_typed_rejection(FA_CASE_OPTIMIZER, name, FA_OK);
    if (strcmp(name, "assembly") == 0) return run_typed_rejection(FA_CASE_ASSEMBLY, name, FA_REJECTED_UNCHECKED);
    if (strcmp(name, "dynamic-link") == 0) return run_typed_rejection(FA_CASE_DYNAMIC_LINK, name, FA_REJECTED_UNCHECKED);
    if (strcmp(name, "allocator") == 0) return run_typed_rejection(FA_CASE_ALLOCATOR, name, FA_REJECTED_ALLOCATOR);
    fprintf(stderr, "unknown checked-native case: %s\n", name);
    return 64;
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: checked-driver CASE\n");
        return 64;
    }
    return run_case(argv[1]);
}
