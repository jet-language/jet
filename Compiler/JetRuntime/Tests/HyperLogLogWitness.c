// Link the real providers with -pthread -Wl,--wrap=jet_rt_free; no substitute implementation.
#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { const char *data; int64_t len, capacity; } Text;
typedef struct { int64_t refs, mutex; uint8_t registers[256]; } HLL;
_Static_assert(sizeof(HLL) == 272, "canonical HLL272 packed U8 stride1");
extern void jet_rt_alloc_init(void);
extern HLL *jet_hyper_log_log_new(void);
extern HLL *jet_rt_handle_clone_jet_hyper_log_log(HLL *);
extern void jet_rt_handle_drop_jet_hyper_log_log(HLL *);
extern Text *jet_rt_handle_show_jet_hyper_log_log(const HLL *);
extern void jet_rt_native_hll_add(HLL *, const Text *);
extern int64_t jet_rt_native_hll_count(HLL *);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern void jet_rt_string_drop(Text *);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
static HLL *watched;
static unsigned frees;
void __wrap_jet_rt_free(void *pointer, int64_t bytes, int64_t align) {
    if (pointer == watched) { assert(bytes == 272 && align == 8); assert(++frees == 1); }
    __real_jet_rt_free(pointer, bytes, align);
}
static void count(HLL *value, int64_t expected) {
    int64_t result = jet_rt_native_hll_count(value);
    assert(jet_int_owned_to_i64(result) == expected); jet_rt_int_drop(result);
}
static void show(HLL *value, const char *expected) {
    Text *text = jet_rt_handle_show_jet_hyper_log_log(value);
    assert(text->len == (int64_t)strlen(expected));
    assert(memcmp(text->data, expected, (size_t)text->len) == 0); jet_rt_string_drop(text);
}
static void *adds(void *pointer) {
    Text shared = {"shared",6,6};
    for (unsigned i = 0; i < 1000; ++i) jet_rt_native_hll_add(pointer, &shared);
    return NULL;
}
int main(void) {
    // Frozen ACTUAL Sketch.rs offline oracle; sparse C initializers describe full 256-byte vectors.
    static const uint8_t four[256] = {[37]=9,[43]=9,[167]=10,[184]=9};
    static const uint8_t shared[256] = {[37]=9,[43]=9,[167]=10,[184]=9,[244]=13};
    static const int64_t linear[256] = {
        1420,1242,1138,1065,1008,961,921,887,857,830,806,783,763,744,726,710,
        694,680,666,653,640,628,617,606,596,585,576,567,558,549,540,532,
        524,517,509,502,495,488,482,475,469,463,457,451,445,439,434,429,
        423,418,413,408,403,398,394,389,385,380,376,371,367,363,359,355,
        351,347,343,339,336,332,328,325,321,318,314,311,308,304,301,298,
        295,291,288,285,282,279,276,273,270,268,265,262,259,256,254,251,
        248,246,243,241,238,236,233,231,228,226,223,221,219,216,214,212,
        209,207,205,203,200,198,196,194,192,190,188,186,184,181,179,177,
        175,173,172,170,168,166,164,162,160,158,156,155,153,151,149,147,
        146,144,142,140,139,137,135,133,132,130,128,127,125,124,122,120,
        119,117,116,114,112,111,109,108,106,105,103,102,100,99,97,96,
        94,93,92,90,89,87,86,85,83,82,80,79,78,76,75,74,
        72,71,70,68,67,66,64,63,62,61,59,58,57,56,54,53,
        52,51,49,48,47,46,45,43,42,41,40,39,38,36,35,34,
        33,32,31,30,29,27,26,25,24,23,22,21,20,19,18,17,
        15,14,13,12,11,10,9,8,7,6,5,4,3,2,1,0
    };
    static const int64_t uniform[55] = {
        368,736,1471,2942,5884,11768,23536,47073,94145,188291,376582,753163,
        1506327,3012654,6025307,12050614,24101229,48202458,96404916,192809831,
        385619662,771239324,1542478649,3084957298,6169914595,12339829190,
        24679658381,49359316762,98718633524,197437267048,394874534095,
        789749068191,1579498136381,3158996272763,6317992545526,12635985091051,
        25271970182102,50543940364205,101087880728409,202175761456818,
        404351522913637,808703045827274,1617406091654547,3234812183309095,
        6469624366618189,12939248733236378,25878497466472756,51756994932945512,
        103513989865891024,207027979731782048,414055959463564096,
        828111918927128192,1656223837854256384,3312447675708512768,6624895351417025536
    };
    static const char snow[] = "snow\0\xe9\x9b\xaa";
    static const Text keys[] = {{"",0,0},{"alpha",5,5},{"beta",4,4},{snow,sizeof(snow)-1,sizeof(snow)-1}};
    jet_rt_alloc_init();
    HLL *owner = jet_hyper_log_log_new(); watched = owner;
    assert(owner->refs == 1 && owner->mutex == 0);
    for (unsigned i = 0; i < 256; ++i) assert(owner->registers[i] == 0);
    count(owner,0); show(owner,"HyperLogLog(count=0)");
    HLL *a = jet_rt_handle_clone_jet_hyper_log_log(owner), *b = jet_rt_handle_clone_jet_hyper_log_log(a);
    assert(owner == a && a == b && owner->refs == 3);
    for (unsigned i = 0; i < 4; ++i) {jet_rt_native_hll_add(a,&keys[i]); count(b,i+1);}
    assert(memcmp(owner->registers,four,256) == 0); show(b,"HyperLogLog(count=4)");
    jet_rt_handle_drop_jet_hyper_log_log(owner); assert(frees == 0 && a->refs == 2);
    pthread_t threads[3];
    for (unsigned i = 0; i < 3; ++i) assert(pthread_create(&threads[i],NULL,adds,a) == 0);
    for (unsigned i = 0; i < 3; ++i) assert(pthread_join(threads[i],NULL) == 0);
    assert(memcmp(b->registers,shared,256) == 0); count(a,5); show(b,"HyperLogLog(count=5)");
    // No concurrent users: initialize real state to all seed estimator boundary vectors.
    for (unsigned empty = 1; empty <= 256; ++empty) {
        memset(a->registers,9,256); memset(a->registers,0,empty); count(b,linear[empty-1]);
    }
    for (unsigned rank = 1; rank <= 255; ++rank) {
        memset(a->registers,(int)rank,256); count(b,rank <= 55 ? uniform[rank-1] : INT64_MAX);
    }
    show(a,"HyperLogLog(count=9223372036854775807)");
    for (unsigned i = 0; i < 256; ++i) a->registers[i] = (uint8_t)(1 + (i*37)%255);
    count(b,31382); show(a,"HyperLogLog(count=31382)");
    jet_rt_handle_drop_jet_hyper_log_log(a); assert(frees == 0 && b->refs == 1);
    jet_rt_handle_drop_jet_hyper_log_log(NULL); assert(frees == 0);
    jet_rt_handle_drop_jet_hyper_log_log(b); assert(frees == 1); watched = NULL;
    puts("HLL shared registers/estimators/show/drop OK");
}
