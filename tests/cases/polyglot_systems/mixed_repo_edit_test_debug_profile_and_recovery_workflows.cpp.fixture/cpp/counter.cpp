#include "counter.hpp"
#include <stdexcept>
#include <thread>

namespace acme {
Counter::Counter(int64_t start) : value(start) {}
int64_t Counter::add(int64_t amount) { value += amount; return value; }
int64_t Counter::add(double factor) { value += static_cast<int64_t>(factor); return value; }
int64_t Counter::fail_if_negative(int64_t input) {
    if (input < 0) throw std::runtime_error("hidden C++ detail");
    return input;
}
int64_t apply(int64_t (*callback)(int64_t), int64_t input) {
    auto result = callback(input);
    if (result < 0) throw std::runtime_error("hidden C++ callback detail");
    return result;
}
int64_t threaded(int64_t input) {
    int64_t output = 0;
    std::thread worker([&] { output = input + 1; });
    worker.join();
    return output;
}
}
