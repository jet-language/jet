#include <unistd.h>

int main(void) {
    static const char text[] = "hello\n";
    return write(1, text, sizeof text - 1) == (ssize_t)(sizeof text - 1) ? 0 : 1;
}
