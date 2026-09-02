#include <array>
#include <charconv>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

constexpr std::size_t capacity = 4;
constexpr std::int64_t modulus = 1'000'003;

bool parseInteger(std::string_view text, std::int64_t &value) {
    if (text.empty()) return false;
    const auto result = std::from_chars(text.data(), text.data() + text.size(), value);
    return result.ec == std::errc{} && result.ptr == text.data() + text.size();
}

std::int64_t positiveMod(std::int64_t value) {
    value %= modulus;
    return value < 0 ? value + modulus : value;
}

struct Ring {
    std::array<std::int64_t, capacity> slots{};
    std::size_t head = 0;
    std::size_t tail = 0;
    std::size_t count = 0;
    bool enqueue(std::int64_t value) {
        if (count == capacity) return false;
        slots[tail] = value;
        tail = (tail + 1) % capacity;
        ++count;
        return true;
    }
    bool dequeue(std::int64_t &value) {
        if (count == 0) return false;
        value = slots[head];
        head = (head + 1) % capacity;
        --count;
        return true;
    }
};

std::int64_t fold(
    std::int64_t digest,
    std::int64_t step,
    std::int64_t code,
    std::int64_t value,
    const Ring &ring
) {
    return positiveMod(
        digest * 131 + step * 17 + code * 257 + positiveMod(value) * 3 +
        static_cast<std::int64_t>(ring.head) * 5 +
        static_cast<std::int64_t>(ring.tail) * 7 +
        static_cast<std::int64_t>(ring.count) * 11
    );
}

void printState(
    std::vector<std::string> &output,
    std::int64_t step,
    std::string_view operation,
    std::string_view result,
    const Ring &ring,
    const std::int64_t *value = nullptr
) {
    std::ostringstream row;
    row << "step=" << step << "|op=" << operation;
    if (value != nullptr) row << "|value=" << *value;
    row << "|result=" << result << "|head=" << ring.head << "|tail=" << ring.tail
        << "|count=" << ring.count;
    output.push_back(row.str());
}

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    Ring ring;
    std::int64_t step = 0, acceptedEnqueues = 0, acceptedDequeues = 0, rejects = 0, digest = 17;
    std::vector<std::string> output{"capacity=4"};
    std::string line;
    while (std::getline(input, line)) {
        if (line.empty() || line.front() == '#') continue;
        ++step;
        std::vector<std::string_view> fields;
        std::size_t start = 0;
        while (true) {
            const auto separator = line.find('|', start);
            fields.emplace_back(line.data() + start, separator == std::string::npos ? line.size() - start : separator - start);
            if (separator == std::string::npos) break;
            start = separator + 1;
        }
        if (fields.size() == 2 && fields[0] == "ENQ") {
            std::int64_t value = 0;
            if (!parseInteger(fields[1], value)) {
                ++rejects;
                digest = fold(digest, step, 5, 0, ring);
                printState(output, step, "malformed", "reject-malformed", ring);
            } else if (ring.enqueue(value)) {
                ++acceptedEnqueues;
                digest = fold(digest, step, 1, value, ring);
                printState(output, step, "enqueue", "accepted", ring, &value);
            } else {
                ++rejects;
                digest = fold(digest, step, 3, value, ring);
                printState(output, step, "enqueue", "reject-overflow", ring, &value);
            }
        } else if (fields.size() == 1 && fields[0] == "DEQ") {
            std::int64_t value = 0;
            if (ring.dequeue(value)) {
                ++acceptedDequeues;
                digest = fold(digest, step, 2, value, ring);
                printState(output, step, "dequeue", "accepted", ring, &value);
            } else {
                ++rejects;
                digest = fold(digest, step, 4, 0, ring);
                printState(output, step, "dequeue", "reject-underflow", ring);
            }
        } else {
            ++rejects;
            digest = fold(digest, step, 5, 0, ring);
            printState(output, step, "malformed", "reject-malformed", ring);
        }
    }
    std::ostringstream summary;
    summary << "summary|accepted_enqueues=" << acceptedEnqueues
        << "|accepted_dequeues=" << acceptedDequeues << "|rejects=" << rejects
        << "|head=" << ring.head << "|tail=" << ring.tail << "|count=" << ring.count;
    output.push_back(summary.str());
    output.push_back("digest=" + std::to_string(digest));
    for (const auto &row : output) std::cout << row << '\n';
}
