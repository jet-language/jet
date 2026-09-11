#pragma once

// CppBind compiles this translation unit as the native implementation carrier.
// The canonical C ABI definitions are included here; the binder recognizes
// their local C-linkage declarations and materializes the archive without a
// second, unrelated C++ facade.
#include "frame_protocol_impl.hpp"
