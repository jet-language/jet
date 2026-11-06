#include "rules.hpp"

namespace rules {

Tick on_tick(Tick tick) noexcept {
    return tick + 1;
}

} // namespace rules
