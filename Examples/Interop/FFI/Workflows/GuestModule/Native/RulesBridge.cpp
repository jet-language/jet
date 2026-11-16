#include "Rules.hpp"
#include "Rules.h"

namespace rules {

Tick on_tick(Tick tick) noexcept {
    return ::on_tick(tick);
}

} // namespace rules
