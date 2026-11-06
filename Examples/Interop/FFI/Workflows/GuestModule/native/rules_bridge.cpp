#include "rules.hpp"
#include "rules.h"

namespace rules {

Tick on_tick(Tick tick) noexcept {
    return ::on_tick(tick);
}

} // namespace rules
