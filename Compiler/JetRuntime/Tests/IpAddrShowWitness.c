// Link real Jet exports; IP24 clone/drop/eq come from canonical18eb2f82f.
// Link -Wl,--wrap=jet_rt_free; no native List<U8> geometry is used here.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { int64_t family; unsigned char octets[16]; } IP;
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
_Static_assert(sizeof(IP) == 24 && offsetof(IP, octets) == 8, "canonical IP24");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern IP *jet_rt_handle_clone_jet_ip_addr(IP *);
extern void jet_rt_handle_drop_jet_ip_addr(IP *);
extern bool jet_rt_handle_eq_jet_ip_addr(IP *, IP *);
extern Text *jet_rt_handle_show_jet_ip_addr(IP *);
extern void jet_rt_string_drop(Text *);
static struct { void *pointer; int64_t size, align; unsigned frees; } watched[4];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *pointer, int64_t size, int64_t align) {
    ++intercepted;
    for (size_t i = 0; i < count; ++i) if (pointer == watched[i].pointer) {
        assert(size == watched[i].size && align == watched[i].align);
        assert(++watched[i].frees == 1);
    }
    __real_jet_rt_free(pointer, size, align);
}
static void watch(void *pointer, int64_t size, int64_t align) {
    assert(count < 4);
    for (size_t i = 0; i < count; ++i) assert(pointer != watched[i].pointer);
    watched[count].pointer = pointer;
    watched[count].size = size;
    watched[count].align = align;
    watched[count++].frees = 0;
}
static void done(unsigned before) {
    assert(intercepted == before + count);
    for (size_t i = 0; i < count; ++i) assert(watched[i].frees == 1);
    count = 0;
}
static void expect(IP *ip, const char *expected) {
    IP saved = *ip;
    Text *text = jet_rt_handle_show_jet_ip_addr(ip);
    assert(text->length == (int64_t)strlen(expected));
    assert(memcmp(text->data, expected, (size_t)text->length) == 0);
    assert(memcmp(ip, &saved, sizeof saved) == 0);
    watch((char *)(text)-8, 32, 8);
    if (text->capacity) watch(text->data, text->capacity, 1);
    unsigned before = intercepted;
    jet_rt_string_drop(text);
    done(before);
}
static void address_case(int64_t family, const unsigned char octets[16], const char *expected) {
    IP *ip = jet_rt_alloc(24, 8);
    ip->family = family;
    memcpy(ip->octets, octets, 16);
    IP *copy = jet_rt_handle_clone_jet_ip_addr(ip);
    assert(ip != copy && jet_rt_handle_eq_jet_ip_addr(ip, copy));
    expect(ip, expected); expect(copy, expected);
    watch((char *)(ip)-8, 32, 8); watch((char *)(copy)-8, 32, 8);
    unsigned before = intercepted;
    jet_rt_handle_drop_jet_ip_addr(ip); jet_rt_handle_drop_jet_ip_addr(copy);
    done(before);
}
static void v6_case(const uint16_t segments[8], const char *expected) {
    unsigned char octets[16];
    for (size_t i = 0; i < 8; ++i) {
        octets[i * 2] = (unsigned char)(segments[i] >> 8);
        octets[i * 2 + 1] = (unsigned char)segments[i];
    }
    address_case(6, octets, expected);
}
int main(void) {
    jet_rt_alloc_init();
    static const struct { uint16_t segments[8]; const char *expected; } cases[] = {
        {{0,0,0,0,0,0,0,0}, "::"},
        {{0,0,0,0,0,0,0,1}, "::1"},
        {{1,0,0,0,0,0,0,0}, "1::"},
        {{0,0,0,0,0,0xffff,0x0102,0x0304}, "::ffff:1.2.3.4"},
        {{0,0,0,0,0,0,0x0102,0x0304}, "::102:304"},
        {{1,0,2,3,4,5,6,7}, "1:0:2:3:4:5:6:7"},
        {{1,0,0,2,0,0,3,4}, "1::2:0:0:3:4"},
        {{1,0,0,2,0,0,0,3}, "1:0:0:2::3"},
        {{0xffff,0xffff,0xffff,0xffff,0xffff,0xffff,0xffff,0xffff}, "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"},
        {{0xabcd,1,0x0010,0x0100,0x1000,0x000f,0x00ff,0x0fff}, "abcd:1:10:100:1000:f:ff:fff"},
        {{0,0,1,2,3,4,5,6}, "::1:2:3:4:5:6"},
        {{1,2,3,4,5,6,0,0}, "1:2:3:4:5:6::"},
        {{0,1,2,3,4,5,6,0}, "0:1:2:3:4:5:6:0"},
        {{1,2,0,0,0,0,0,3}, "1:2::3"},
        {{0,0,0,0,0,0xffff,0,0}, "::ffff:0.0.0.0"},
        {{0,0,0,0,0,0xffff,0xffff,0xffff}, "::ffff:255.255.255.255"},
        {{0,0,0,0,1,0xffff,0x0102,0x0304}, "::1:ffff:102:304"}
    };
    for (size_t i = 0; i < sizeof cases / sizeof cases[0]; ++i) v6_case(cases[i].segments, cases[i].expected);
    for (unsigned byte = 0; byte < 256; ++byte) {
        unsigned char octets[16] = {0};
        for (size_t i = 0; i < 4; ++i) octets[i] = (unsigned char)byte;
        char expected[16];
        int length = snprintf(expected, sizeof expected, "%u.%u.%u.%u", byte, byte, byte, byte);
        assert(length > 0 && length < (int)sizeof expected);
        address_case(4, octets, expected);
    }
    assert(intercepted != 0);
    puts("IPAddr native Show OK");
    return 0;
}
