#include "reader.hpp"

namespace {
long syscall3(long number, long a, long b, long c) {
    long result;
    asm volatile("syscall"
                 : "=a"(result)
                 : "a"(number), "D"(a), "S"(b), "d"(c)
                 : "rcx", "r11", "memory");
    return result;
}
}

namespace bridge {
Reader::Reader() : fd_(syscall3(2, reinterpret_cast<long>("sample.txt"), 0, 0)) {}

int64_t Reader::next_line_length() {
    char buffer[256];
    const long count = syscall3(0, fd_, reinterpret_cast<long>(buffer), 255);
    if (count <= 0) return -1;
    long length = 0;
    while (length < count && buffer[length] != '\n') ++length;
    return static_cast<int64_t>(length);
}
}
