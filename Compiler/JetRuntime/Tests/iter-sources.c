// Raw producer ABI witnesses; source-policy behavior is tested by Core goldens.
// No Rust owner layout and no simulated stop: use the actual comptime rail.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef intptr_t Word;
typedef struct { Word stride, clone, drop, show, equal, compare, kind, child0, child1; } Meta;
typedef struct { Word payload, next, release, clone; } Adapter;
typedef struct { int at, releases, clones, stop_at, moved_stop_at; } Source;
static int drops;
extern Word jet_rt_native_iter_file_lines(Word, Word, Word);
extern Word jet_rt_native_iter_stdin_lines(Word, Word, Word);
extern Word jet_rt_native_iter_process_lines(Word, Word, Word);
extern Word jet_rt_native_iter_encoding_reader(Word, Word, Word);
extern Word jet_rt_native_iter_next(Word);
extern void jet_rt_native_iter_drop(Word);
extern Word jet_rt_string_from_static(Word, Word);
extern Word jet_rt_string_clone(Word);
extern void jet_rt_string_drop(Word);
extern void jet_rt_free(Word, Word, Word);
extern bool jet_rt_comptime_begin(uint64_t, uint64_t);
extern void jet_rt_comptime_work(uint64_t);
extern void jet_rt_comptime_end(void);
extern Word jet_rt_session_propagate(void);
static Word text_clone(Word from, Word to) {
    *(Word *)to = jet_rt_string_clone(*(Word *)from);
    return 0;
}
static Word text_drop(Word slot) {
    ++drops;
    jet_rt_string_drop(*(Word *)slot);
    return 0;
}
static Word next(Word raw, Word out) {
    Source *source = (Source *)raw;
    if (++source->at == source->stop_at) {
        jet_rt_comptime_work(UINT64_C(1000001));
        assert(jet_rt_session_propagate());
        return -1; // Deliberately do not initialize out.
    }
    // The source owns CRLF policy; preserve exactly its produced slot bytes.
    static const char *const lines[] = {"CRLF-stripped", "tail-without-newline"};
    if (source->at > 2) return 0;
    const char *line = lines[source->at - 1];
    *(Word *)out = jet_rt_string_from_static((Word)line, (Word)strlen(line));
    if (source->at == source->moved_stop_at) {
        jet_rt_comptime_work(UINT64_C(1000001));
        assert(jet_rt_session_propagate());
    }
    return 1;
}
static Word release(Word raw) { ++((Source *)raw)->releases; return 0; }
static Word clone(Word raw) { ++((Source *)raw)->clones; return raw; }
static void value(Word iter, const char *expected) {
    Word slot = jet_rt_native_iter_next(iter);
    assert(slot);
    // E4 Text24 is [compact bytes, byte length, capacity].
    Word *text = (Word *)*(Word *)slot;
    assert(text[1] == (Word)strlen(expected));
    assert(memcmp((void *)text[0], expected, (size_t)text[1]) == 0);
    text_drop(slot);
    jet_rt_free(slot, 8, 8);
}
int main(void) {
    Meta meta = {8, (Word)text_clone, (Word)text_drop, 0, 0, 0, 5, 0, 0};
    Word (*providers[])(Word, Word, Word) = {
        jet_rt_native_iter_file_lines, jet_rt_native_iter_stdin_lines,
        jet_rt_native_iter_process_lines, jet_rt_native_iter_encoding_reader
    };
    for (size_t family = 0; family < 4; ++family) {
        Source source = {0};
        Adapter adapter = {(Word)&source, (Word)next, (Word)release, (Word)clone};
        Word iter = providers[family]((Word)&adapter, 1, (Word)&meta);
        assert(source.at == 0); // Owned construction is lazy.
        value(iter, "CRLF-stripped");
        value(iter, "tail-without-newline");
        assert(!jet_rt_native_iter_next(iter));
        assert(!jet_rt_native_iter_next(iter)); // Fused EOF: no callback again.
        assert(source.at == 3);
        jet_rt_native_iter_drop(iter);
        assert(source.releases == 1);
        source = (Source){0};
        iter = providers[family]((Word)&adapter, 1, (Word)&meta);
        jet_rt_native_iter_drop(iter); // No pull, exactly one release.
        assert(source.at == 0 && source.releases == 1);
        source = (Source){.stop_at = 2};
        assert(jet_rt_comptime_begin(UINT64_C(1000000), UINT64_C(1000)));
        iter = providers[family]((Word)&adapter, 1, (Word)&meta);
        value(iter, "CRLF-stripped");
        assert(!jet_rt_native_iter_next(iter));
        assert(jet_rt_session_propagate());
        jet_rt_native_iter_drop(iter);
        assert(source.releases == 1);
        jet_rt_comptime_end();
        source = (Source){.moved_stop_at = 1};
        drops = 0;
        assert(jet_rt_comptime_begin(UINT64_C(1000000), UINT64_C(1000)));
        iter = providers[family]((Word)&adapter, 1, (Word)&meta);
        assert(!jet_rt_native_iter_next(iter)); // iter_pull drops moved output.
        assert(jet_rt_session_propagate() && drops == 1);
        jet_rt_native_iter_drop(iter);
        assert(source.releases == 1 && drops == 1);
        jet_rt_comptime_end();
    }
    for (size_t family = 0; family < 2; ++family) {
        Source source = {0};
        Adapter adapter = {(Word)&source, (Word)next, (Word)release, (Word)clone};
        Word iter = providers[family]((Word)&adapter, 0, (Word)&meta);
        assert(source.at == 3 && source.releases == 0); // Borrowed eager snapshot.
        value(iter, "CRLF-stripped");
        jet_rt_native_iter_drop(iter);
        assert(source.releases == 0);
        source = (Source){.stop_at = 2};
        assert(jet_rt_comptime_begin(UINT64_C(1000000), UINT64_C(1000)));
        assert(!providers[family]((Word)&adapter, 0, (Word)&meta));
        assert(source.at == 2 && source.releases == 0);
        jet_rt_comptime_end();
    }
    Source source = {0};
    Adapter adapter = {(Word)&source, (Word)next, (Word)release, (Word)clone};
    Word iter = jet_rt_native_iter_process_lines((Word)&adapter, 0, (Word)&meta);
    assert(source.clones == 1 && source.at == 0);
    jet_rt_native_iter_drop(iter);
    assert(source.releases == 1);
    puts("iter sources: ownership, EOF, snapshot and real stop rail passed");
    return 0;
}
