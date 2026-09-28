#pragma once

#include <cstdint>

namespace rules {

using Tick = std::int64_t;

// Existing native ABI.  The game call site must remain rules::on_tick(tick).
Tick on_tick(Tick tick) noexcept;

} // namespace rules
