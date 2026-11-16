#pragma once
#include <cstdint>

namespace acme {
class Counter {
public:
    explicit Counter(int64_t start);
    int64_t add(int64_t amount);
    int64_t add(double factor);
    int64_t fail_if_negative(int64_t value);
private:
    int64_t value;
};

int64_t apply(int64_t (*callback)(int64_t), int64_t value);
int64_t threaded(int64_t value);
}
