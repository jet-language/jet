// Direct C ABI witness: validates numeric value independently of jet_int_compare
// and the runtime's formatter, so a mutually wrong parser/compare cannot pass.
#define _POSIX_C_SOURCE 200809L
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

typedef struct {
    uint64_t refs, retired_next, negative;
    uint32_t *limbs;
    uint64_t len, cap;
} IntNode;
_Static_assert(sizeof(IntNode) == 48, "exact Int C carrier size");

extern int64_t jet_rt_int_from_static(const unsigned char *, uint64_t);
extern int64_t jet_rt_int_clone(int64_t);
extern void jet_rt_int_drop(int64_t);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern int64_t jet_rt_int_clone_atomic_reload(int64_t, int64_t (*)(int64_t), int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern double jet_int_owned_to_f64(int64_t);
extern int64_t jet_int_compare(int64_t, int64_t);
extern int64_t jet_int_checked_fixed(int64_t, int64_t, const unsigned char *, uint64_t, int64_t);
#define BINARY(name) extern int64_t jet_int_##name(int64_t, int64_t);
BINARY(add) BINARY(sub) BINARY(mul) BINARY(bit_and) BINARY(bit_or) BINARY(bit_xor)
#define LOCATED(name) extern int64_t jet_int_##name(int64_t, int64_t, const unsigned char *, uint64_t, int64_t);
LOCATED(div) LOCATED(rem) LOCATED(mod) LOCATED(floor_div)
LOCATED(div_euclid) LOCATED(rem_euclid) LOCATED(pow) LOCATED(shl) LOCATED(shr)

static int failures;
static int64_t parse(const char *text) {
    return jet_rt_int_from_static((const unsigned char *)text, strlen(text));
}

static void value(const char *label, int64_t raw, const char *expected) {
    uint32_t limbs[160] = {0};
    size_t len = 0;
    int negative = expected[0] == '-';
    const char *digits = expected + negative;
    size_t end = strlen(digits);
    do {
        size_t start = end > 9 ? end - 9 : 0;
        uint32_t limb = 0;
        for (size_t i = start; i < end; ++i) limb = limb * 10 + (uint32_t)(digits[i] - '0');
        limbs[len++] = limb;
        end = start;
    } while (end);
    int ok = 1;
    if (raw >= -(INT64_C(1) << 62) && raw < (INT64_C(1) << 62)) {
        char text[32];
        snprintf(text, sizeof(text), "%" PRId64, raw);
        ok = !strcmp(text, expected);
    } else {
        IntNode *node = (IntNode *)(uintptr_t)((uint64_t)raw & ((UINT64_C(1) << 62) - 1));
        ok = node->refs != 0 && node->retired_next == 0 && node->negative == (uint64_t)negative && node->len == len && node->cap >= len;
        if (ok) ok = !memcmp(limbs, node->limbs, len * sizeof(uint32_t));
    }
    if (!ok) { fprintf(stderr, "%s: wrong value (expected %s)\n", label, expected); ++failures; }
    printf("%s\t%s\t%s\n", label, expected, ok ? "pass" : "wrong");
    jet_rt_int_drop(raw);
}

static void word_value(const char *label, int64_t word, int64_t expected) {
    int ok = word == expected;
    if (!ok) { fprintf(stderr, "%s: wrong fixed word\n", label); ++failures; }
    printf("%s\t%" PRId64 "\t%s\n", label, word, ok ? "pass" : "wrong");
}

static void float_value(const char *label, int64_t raw, uint64_t expected) {
    union { double f; uint64_t bits; } actual = { .f = jet_int_owned_to_f64(raw) };
    int ok = actual.bits == expected;
    if (!ok) { fprintf(stderr, "%s: float bits %016" PRIx64 " != %016" PRIx64 "\n", label, actual.bits, expected); ++failures; }
    printf("%s\t%016" PRIx64 "\t%s\n", label, actual.bits, ok ? "pass" : "wrong");
    jet_rt_int_drop(raw);
}

static void hazard_check(int ok, const char *what) {
    if (!ok) { fprintf(stderr, "int hazard: %s\n", what); ++failures; }
}

static IntNode *node_of(int64_t raw) { return (IntNode *)(uintptr_t)((uint64_t)raw & ((UINT64_C(1) << 62) - 1)); }

// The hazard reader: a foreign thread cloning an atomic Int slot through the
// real hazard domain (IntHazard.jet int_hazard_clone). Its second reload runs
// after the clone published its hazard: it lets the writer empty the slot and
// final-drop the node, sees the node retired but intact, and reloads the now
// empty slot, so the clone yields 0 and clears its hazard.
typedef struct {
    _Atomic int64_t slot;
    pthread_barrier_t barrier;
    IntNode *node;
    int reloads, protected_ok;
    int64_t observed;
} HazardRace;
static HazardRace race;
static int64_t hazard_reload(int64_t context) {
    (void)context;
    if (++race.reloads == 2) {
        pthread_barrier_wait(&race.barrier);
        pthread_barrier_wait(&race.barrier);
        const uint32_t digits[3] = {854775807, 223372036, 9};
        race.protected_ok = race.node->refs == 0 && race.node->len == 3 && !memcmp(race.node->limbs, digits, sizeof(digits));
    }
    return atomic_load(&race.slot);
}
static void *hazard_reader(void *initial) {
    jet_rt_thread_attach();
    race.observed = jet_rt_int_clone_atomic_reload((int64_t)(intptr_t)initial, hazard_reload, 0);
    jet_rt_thread_detach();
    return NULL;
}

// The checked heap's free observer counts frees of the one watched node.
static _Atomic uintptr_t watched;
static _Atomic int watched_frees;
static void observe_free(int64_t address, int64_t bytes, int64_t align) {
    (void)bytes; (void)align;
    if ((uintptr_t)address == atomic_load(&watched)) atomic_fetch_add(&watched_frees, 1);
}
static void watch(IntNode *node) {
    atomic_store(&watched, (uintptr_t)node);
    atomic_store(&watched_frees, 0);
}

// An unhazarded final drop frees its node at once; a node a reader has
// hazarded is retired instead, and the reader's clearing reclaims it, once.
// The checked heap (every C host that links the runtime object runs it) traps
// a double free itself. The fixture drives the hazard domain's C ABI.
static void hazard_fixture(void) {
    hazard_check(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1, "the checked heap observer was not installed");
    int64_t raw = jet_int_owned_from_i64(INT64_MAX);
    IntNode *node = node_of(raw);
    int64_t clone = jet_rt_int_clone(raw);
    hazard_check(node->refs == 2, "a clone did not count its owner");
    jet_rt_int_drop(clone);
    hazard_check(node->refs == 1, "a dropped clone did not release its owner");
    watch(node);
    jet_rt_int_drop(raw);
    hazard_check(atomic_load(&watched_frees) == 1, "unhazarded final drop did not free once");

    raw = jet_int_owned_from_i64(INT64_MAX);
    race.node = node_of(raw);
    watch(race.node);
    atomic_store(&race.slot, raw);
    pthread_barrier_init(&race.barrier, NULL, 2);
    pthread_t reader;
    hazard_check(pthread_create(&reader, NULL, hazard_reader, (void *)(intptr_t)raw) == 0, "reader thread did not start");
    pthread_barrier_wait(&race.barrier);
    atomic_store(&race.slot, 0); // consume the atomic slot's owner
    jet_rt_int_drop(raw);
    hazard_check(atomic_load(&watched_frees) == 0, "freed a hazarded node");
    pthread_barrier_wait(&race.barrier);
    pthread_join(reader, NULL);
    pthread_barrier_destroy(&race.barrier);
    hazard_check(race.protected_ok, "the hazarded node was not kept intact");
    hazard_check(race.observed == 0, "the reader did not observe the emptied slot");
    // Clearing the real read guard reclaims the real retired stack.
    hazard_check(atomic_load(&watched_frees) == 1, "cleared hazard did not reclaim once");
    jet_rt_alloc_check_observe(0, 0);
    jet_rt_int_drop(0);
    jet_rt_int_drop(-1);
}

#include "int-corpus.inc"

int main(int argc, char **argv) {
    // Independent stopped processes allow byte-for-byte Rust/native report
    // and exit-status comparisons without narrowing the success corpus.
    const unsigned char *file = (const unsigned char *)"int-cabi.jet";
    if (argc == 2) {
        if (!strcmp(argv[1], "zero")) jet_int_div(1, 0, file, 12, 7);
        else if (!strcmp(argv[1], "exponent")) jet_int_pow(2, -1, file, 12, 7);
        else if (!strcmp(argv[1], "shift-negative")) jet_int_shl(1, -1, file, 12, 7);
        else if (!strcmp(argv[1], "shift-wide")) jet_int_shr(1, parse("9223372036854775808"), file, 12, 7);
        else if (!strcmp(argv[1], "fixed")) jet_int_checked_fixed(128, 0, file, 12, 7);
        else if (!strcmp(argv[1], "native")) jet_int_owned_to_i64(parse("9223372036854775808"));
        else return 64;
        return 65; // Every named case must stop before returning.
    }
    int_corpus();
    hazard_fixture();
    return failures ? 1 : 0;
}
