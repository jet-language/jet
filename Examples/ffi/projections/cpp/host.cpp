#include "projection_cpp.hpp"
#include <cstdint>

int main() {
    const uint8_t initial[] = {10, 20, 30};
    const uint8_t replacement[] = {40, 50, 60};
    jet::ResourceDocument document;
    jet::ResourceView stale{};
    int64_t value = 0;
    if (document.open(initial, sizeof(initial)) != PROJECTION_CPP_OK) return 1;
    if (document.bytes(&stale) != PROJECTION_CPP_OK) return 1;
    if (document.at(stale, 1, &value) != PROJECTION_CPP_OK || value != 20) return 1;
    if (document.replace(replacement, sizeof(replacement)) != PROJECTION_CPP_OK) return 1;
    if (document.at(stale, 1, &value) != PROJECTION_CPP_EXPIRED_VIEW) return 1;
    jet::ResourceView fresh{};
    if (document.bytes(&fresh) != PROJECTION_CPP_OK) return 1;
    if (document.at(fresh, 1, &value) != PROJECTION_CPP_OK || value != 50) return 1;
    if (document.close() != PROJECTION_CPP_OK) return 1;
    if (document.close() != PROJECTION_CPP_CLOSED) return 1;
    return jet::add(41) == 42 ? 0 : 1;
}
