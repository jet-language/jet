#pragma once

#include "frame.hpp"
#include "frame_protocol.h"

#include <array>
#include <limits>
#include <mutex>
#include <new>

namespace frames::protocol_detail {

struct Slot {
    Frame frame;
};

static std::array<Slot *, 64> slots{};
static std::mutex slots_mutex;

static Slot *lookup(uint64_t slot) noexcept {
    if (slot == 0 || slot > slots.size()) {
        return nullptr;
    }
    return slots[slot - 1];
}

static Slot *lookup_owner(const FrameDocument *owner) noexcept {
    if (owner == nullptr) {
        return nullptr;
    }
    Slot *slot = lookup(owner->slot);
    if (slot == nullptr || slot->frame.generation != owner->generation) {
        return nullptr;
    }
    return slot;
}

} // namespace frames::protocol_detail

extern "C" int frame_document_open(
    const uint8_t *input,
    size_t len,
    FrameDocument *out) {
    if (out == nullptr || (input == nullptr && len != 0)) {
        return FRAME_INVALID_HANDLE;
    }
    std::lock_guard<std::mutex> guard(frames::protocol_detail::slots_mutex);
    for (size_t index = 0; index < frames::protocol_detail::slots.size(); ++index) {
        if (frames::protocol_detail::slots[index] != nullptr) {
            continue;
        }
        try {
            auto *slot = new frames::protocol_detail::Slot{};
            if (len != 0) {
                slot->frame.data.assign(input, input + len);
            }
            frames::protocol_detail::slots[index] = slot;
            out->slot = static_cast<uint64_t>(index + 1);
            out->generation = slot->frame.generation;
            return FRAME_OK;
        } catch (const std::bad_alloc &) {
            return FRAME_OVERFLOW;
        }
    }
    return FRAME_OVERFLOW;
}

extern "C" int frame_document_bytes(
    const FrameDocument *owner,
    FrameView *out) {
    if (out == nullptr) {
        return FRAME_INVALID_HANDLE;
    }
    std::lock_guard<std::mutex> guard(frames::protocol_detail::slots_mutex);
    auto *slot = frames::protocol_detail::lookup_owner(owner);
    if (slot == nullptr) {
        return owner != nullptr && frames::protocol_detail::lookup(owner->slot) != nullptr
            ? FRAME_EXPIRED_VIEW
            : FRAME_INVALID_HANDLE;
    }
    const auto pixels = slot->frame.pixels();
    if (pixels.size() > std::numeric_limits<uint64_t>::max()) {
        return FRAME_OVERFLOW;
    }
    out->slot = owner->slot;
    out->generation = owner->generation;
    return FRAME_OK;
}

extern "C" int frame_view_at(
    const FrameView *view,
    size_t index,
    uint8_t *out) {
    if (view == nullptr || out == nullptr) {
        return FRAME_INVALID_HANDLE;
    }
    std::lock_guard<std::mutex> guard(frames::protocol_detail::slots_mutex);
    auto *slot = frames::protocol_detail::lookup(view->slot);
    if (slot == nullptr) {
        return FRAME_INVALID_HANDLE;
    }
    if (slot->frame.generation != view->generation) {
        return FRAME_EXPIRED_VIEW;
    }
    const auto pixels = slot->frame.pixels();
    if (index >= pixels.size()) {
        return FRAME_OVERFLOW;
    }
    *out = pixels[index];
    return FRAME_OK;
}

extern "C" int frame_document_replace(
    FrameDocument *owner,
    const uint8_t *input,
    size_t len) {
    if (owner == nullptr || (input == nullptr && len != 0)) {
        return FRAME_INVALID_HANDLE;
    }
    std::lock_guard<std::mutex> guard(frames::protocol_detail::slots_mutex);
    auto *slot = frames::protocol_detail::lookup_owner(owner);
    if (slot == nullptr) {
        return FRAME_INVALID_HANDLE;
    }
    if (slot->frame.generation == std::numeric_limits<uint64_t>::max()) {
        return FRAME_OVERFLOW;
    }
    try {
        std::vector<uint8_t> next;
        if (len != 0) {
            next.assign(input, input + len);
        }
        slot->frame.replace(std::move(next));
        owner->generation = slot->frame.generation;
        return FRAME_OK;
    } catch (const std::bad_alloc &) {
        return FRAME_OVERFLOW;
    }
}

extern "C" int frame_document_close(FrameDocument *owner) {
    if (owner == nullptr) {
        return FRAME_INVALID_HANDLE;
    }
    if (owner->slot == 0) {
        return FRAME_CLOSED;
    }
    std::lock_guard<std::mutex> guard(frames::protocol_detail::slots_mutex);
    auto *slot = frames::protocol_detail::lookup(owner->slot);
    if (slot == nullptr || slot->frame.generation != owner->generation) {
        return FRAME_INVALID_HANDLE;
    }
    frames::protocol_detail::slots[owner->slot - 1] = nullptr;
    delete slot;
    owner->slot = 0;
    owner->generation = 0;
    return FRAME_OK;
}
