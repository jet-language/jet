// Link the real composed Jet runtime with -Wl,--wrap=jet_rt_free.
// Packed native byte-list stride must match canonical RT_HOST_U8_STRIDE.
#include <assert.h>
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#define EMAIL_TRUST_U8_STRIDE 1
_Static_assert(EMAIL_TRUST_U8_STRIDE == 1, "packed List<U8> stride");
typedef struct { int64_t length, capacity; unsigned char *data; } Sequence;
typedef struct { int64_t kind; Sequence *pem; } Trust;
_Static_assert(sizeof(Sequence) == 24 && offsetof(Sequence, data) == 16, "List24");
_Static_assert(sizeof(Trust) == 16 && offsetof(Trust, pem) == 8, "EmailTrustABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern Trust *jet_rt_handle_clone_tls_trust(Trust *);
extern void jet_rt_handle_drop_tls_trust(Trust *);
extern bool jet_rt_handle_eq_tls_trust(Trust *, Trust *);
static struct { void *pointer; int64_t size; unsigned frees; } watched[12];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *pointer, int64_t size, int64_t align) {
    ++intercepted;
    for (size_t i = 0; i < count; ++i) if (pointer == watched[i].pointer) {
        assert(size == watched[i].size && align == 8);
        assert(++watched[i].frees == 1);
    }
    __real_jet_rt_free(pointer, size, align);
}
static void watch(void *pointer, int64_t size) {
    assert(count < 12);
    for (size_t i = 0; i < count; ++i) assert(pointer != watched[i].pointer);
    watched[count].pointer = pointer;
    watched[count].size = size;
    watched[count++].frees = 0;
}
static void watch_trust(Trust *trust) {
    watch(trust, 16);
    if (trust->kind == 1) {
        watch(trust->pem, 24);
        if (trust->pem->capacity) watch(trust->pem->data, trust->pem->capacity * EMAIL_TRUST_U8_STRIDE);
    }
}
static void done(unsigned before) {
    assert(intercepted == before + count);
    for (size_t i = 0; i < count; ++i) assert(watched[i].frees == 1);
    count = 0;
}
static Trust *new_trust(int64_t kind, int64_t length, int64_t capacity) {
    Trust *trust = jet_rt_alloc(16, 8);
    trust->kind = kind;
    trust->pem = (Sequence *)(uintptr_t)1; // Inactive System payload must not be read.
    if (kind == 1) {
        Sequence *pem = jet_rt_alloc(24, 8);
        pem->length = length;
        pem->capacity = capacity;
        pem->data = capacity ? jet_rt_alloc(capacity * EMAIL_TRUST_U8_STRIDE, 8) : (unsigned char *)(uintptr_t)8;
        for (int64_t i = 0; i < capacity; ++i) pem->data[i * EMAIL_TRUST_U8_STRIDE] = (unsigned char)i;
        trust->pem = pem;
    }
    return trust;
}
static void system_case(void) {
    Trust *a = new_trust(0, 0, 0);
    Trust *b = jet_rt_handle_clone_tls_trust(a);
    assert(a != b && b->kind == 0);
    assert(jet_rt_handle_eq_tls_trust(a, a) && jet_rt_handle_eq_tls_trust(a, b));
    b->pem = NULL;
    assert(jet_rt_handle_eq_tls_trust(a, b));
    watch_trust(a); watch_trust(b);
    unsigned before = intercepted;
    jet_rt_handle_drop_tls_trust(a); jet_rt_handle_drop_tls_trust(b);
    done(before);
}
static void ca_case(int64_t length, int64_t capacity) {
    Trust *a = new_trust(1, length, capacity);
    Trust *b = jet_rt_handle_clone_tls_trust(a);
    Trust *c = jet_rt_handle_clone_tls_trust(b);
    Trust *system = new_trust(0, 0, 0);
    assert(a != b && b != c && a != c);
    assert(a->pem != b->pem && b->pem != c->pem && a->pem != c->pem);
    assert(b->pem->length == length && b->pem->capacity == length);
    assert(c->pem->length == length && c->pem->capacity == length);
    assert(jet_rt_handle_eq_tls_trust(a, a) && jet_rt_handle_eq_tls_trust(a, b));
    assert(jet_rt_handle_eq_tls_trust(b, c));
    assert(!jet_rt_handle_eq_tls_trust(a, system) && !jet_rt_handle_eq_tls_trust(system, a));
    if (length) {
        assert(a->pem->data != b->pem->data && b->pem->data != c->pem->data);
        for (int64_t i = 0; i < length; ++i) assert(b->pem->data[i * EMAIL_TRUST_U8_STRIDE] == (unsigned char)i);
        b->pem->length = length - 1;
        assert(!jet_rt_handle_eq_tls_trust(a, b));
        b->pem->length = length;
        b->pem->data[(length - 1) * EMAIL_TRUST_U8_STRIDE] ^= 1;
        assert(!jet_rt_handle_eq_tls_trust(a, b) && jet_rt_handle_eq_tls_trust(a, c));
        b->pem->data[(length - 1) * EMAIL_TRUST_U8_STRIDE] ^= 1;
        if (length > 1) {
            unsigned char byte = b->pem->data[0];
            b->pem->data[0] = b->pem->data[EMAIL_TRUST_U8_STRIDE];
            b->pem->data[EMAIL_TRUST_U8_STRIDE] = byte;
            assert(!jet_rt_handle_eq_tls_trust(a, b));
            b->pem->data[EMAIL_TRUST_U8_STRIDE] = b->pem->data[0];
            b->pem->data[0] = byte;
        }
    }
    watch_trust(a); watch_trust(b); watch_trust(c); watch_trust(system);
    unsigned before = intercepted;
    jet_rt_handle_drop_tls_trust(a);
    assert(jet_rt_handle_eq_tls_trust(b, c));
    jet_rt_handle_drop_tls_trust(b); jet_rt_handle_drop_tls_trust(c); jet_rt_handle_drop_tls_trust(system);
    done(before);
}
int main(void) {
    jet_rt_alloc_init();
    system_case();
    ca_case(0, 0); ca_case(0, 4); ca_case(1, 4); ca_case(256, 300);
    unsigned before = intercepted;
    jet_rt_handle_drop_tls_trust(NULL);
    assert(intercepted == before && intercepted != 0);
    puts("Email TLSTrust ownership/equality OK");
    return 0;
}
