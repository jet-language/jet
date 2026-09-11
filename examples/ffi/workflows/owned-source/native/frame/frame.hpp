#pragma once

#include <cstddef>
#include <cstdint>
#if __cplusplus >= 202002L
#include <span>
#endif
#include <utility>
#include <vector>

#if __cplusplus >= 202002L
using PixelView = std::span<const std::uint8_t>;
#else
// The checked C++ binder currently emits a C++17 shim.  Keep the native
// source's C++20 std::span contract while giving that shim a non-owning,
// equivalent view type for header parsing and adapter compilation.
class PixelView {
public:
    explicit PixelView(const std::vector<std::uint8_t>& values) noexcept
        : data_(values.data()), size_(values.size()) {}

    const std::uint8_t& operator[](std::size_t index) const noexcept {
        return data_[index];
    }

    std::size_t size() const noexcept { return size_; }

private:
    const std::uint8_t* data_;
    std::size_t size_;
};
#endif

namespace frames {

struct Frame {
    std::vector<std::uint8_t> data;
    std::uint64_t generation{0};

#if __cplusplus >= 202002L
    std::span<const std::uint8_t> pixels() const noexcept { return data; }
#else
    PixelView pixels() const noexcept { return PixelView(data); }
#endif

    void replace(std::vector<std::uint8_t> next) {
        data = std::move(next);
        ++generation;
    }
};

inline Frame make_frame() {
    return Frame{{255, 0, 0, 255}, 0};
}

inline std::uint64_t generation(const Frame& frame) noexcept {
    return frame.generation;
}

} // namespace frames
