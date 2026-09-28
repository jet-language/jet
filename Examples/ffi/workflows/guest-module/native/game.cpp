#include "rules.hpp"

#include <cstdint>
#include <iostream>

int main() {
    const std::int64_t next = rules::on_tick(41);
    std::cout << next << '\n';
    return next == 42 ? 0 : 1;
}
