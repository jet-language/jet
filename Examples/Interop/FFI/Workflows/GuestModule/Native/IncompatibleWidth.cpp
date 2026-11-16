#include <cstdint>

// Deliberately not linked.  The workflow driver feeds this contract to the
// binder and expects an explicit width rejection, not a truncating cast.
extern "C" std::int32_t on_tick(std::int32_t tick) noexcept {
    return tick + 1;
}
