#include <stdint.h>

int64_t game_probe_add(int64_t left, int64_t right) {
    return left + right;
}

int32_t game_probe_key_down(int32_t key) {
    return key == 65;
}

void game_probe_draw_rect(int32_t x, int32_t y, int32_t width, int32_t height) {
    (void)x;
    (void)y;
    (void)width;
    (void)height;
}
