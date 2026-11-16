#include <algorithm>
#include <array>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

namespace fs = std::filesystem;

struct Row {
    std::string path;
    std::uintmax_t bytes;
    std::string digest;
};

std::uint32_t rotateRight(std::uint32_t value, unsigned amount) {
    return (value >> amount) | (value << (32 - amount));
}

std::string sha256(std::string_view data) {
    constexpr std::array<std::uint32_t, 64> roundConstants = {
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
        0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    };
    std::array<std::uint32_t, 8> state = {
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    };
    std::vector<std::uint8_t> message(data.begin(), data.end());
    message.push_back(0x80);
    while ((message.size() + 8) % 64 != 0) message.push_back(0);
    const std::uint64_t bitLength = static_cast<std::uint64_t>(data.size()) * 8;
    for (int shift = 56; shift >= 0; shift -= 8) {
        message.push_back(static_cast<std::uint8_t>(bitLength >> shift));
    }
    for (std::size_t offset = 0; offset < message.size(); offset += 64) {
        std::array<std::uint32_t, 64> schedule{};
        for (std::size_t index = 0; index < 16; ++index) {
            const std::size_t position = offset + index * 4;
            schedule[index] = (static_cast<std::uint32_t>(message[position]) << 24) |
                (static_cast<std::uint32_t>(message[position + 1]) << 16) |
                (static_cast<std::uint32_t>(message[position + 2]) << 8) |
                static_cast<std::uint32_t>(message[position + 3]);
        }
        for (std::size_t index = 16; index < 64; ++index) {
            const auto first = schedule[index - 15];
            const auto second = schedule[index - 2];
            const auto sigma0 = rotateRight(first, 7) ^ rotateRight(first, 18) ^ (first >> 3);
            const auto sigma1 = rotateRight(second, 17) ^ rotateRight(second, 19) ^ (second >> 10);
            schedule[index] = schedule[index - 16] + sigma0 + schedule[index - 7] + sigma1;
        }
        auto a = state[0], b = state[1], c = state[2], d = state[3];
        auto e = state[4], f = state[5], g = state[6], h = state[7];
        for (std::size_t index = 0; index < 64; ++index) {
            const auto sigma1 = rotateRight(e, 6) ^ rotateRight(e, 11) ^ rotateRight(e, 25);
            const auto choice = (e & f) ^ ((~e) & g);
            const auto first = h + sigma1 + choice + roundConstants[index] + schedule[index];
            const auto sigma0 = rotateRight(a, 2) ^ rotateRight(a, 13) ^ rotateRight(a, 22);
            const auto majority = (a & b) ^ (a & c) ^ (b & c);
            const auto second = sigma0 + majority;
            h = g; g = f; f = e; e = d + first;
            d = c; c = b; b = a; a = first + second;
        }
        state[0] += a; state[1] += b; state[2] += c; state[3] += d;
        state[4] += e; state[5] += f; state[6] += g; state[7] += h;
    }
    std::ostringstream output;
    output << std::hex << std::setfill('0');
    for (const auto value : state) output << std::setw(8) << value;
    return output.str();
}

bool readFile(const fs::path &file, std::string &data) {
    std::ifstream input(file, std::ios::binary);
    if (!input) return false;
    data.assign(std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>());
    return input.good() || input.eof();
}

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    const fs::path root = argv[1];
    std::vector<Row> rows;
    std::uintmax_t bytes = 0;
    std::size_t links = 0, rejects = 0;
    std::error_code ec;
    if (!fs::is_directory(root, ec)) return 2;
    fs::recursive_directory_iterator it(root, fs::directory_options::skip_permission_denied, ec), end;
    while (it != end) {
        const fs::directory_entry entry = *it;
        const fs::path relative = fs::relative(entry.path(), root, ec);
        if (ec) { ec.clear(); ++rejects; it.increment(ec); continue; }
        const std::string rel = relative.generic_string();
        if (rel == ".fixture-modes.tsv") { it.increment(ec); continue; }
        std::error_code typeEc;
        const auto status = entry.symlink_status(typeEc);
        if (typeEc) { ++rejects; it.increment(ec); continue; }
        if (fs::is_symlink(status)) {
            ++links;
        } else if (fs::is_regular_file(status)) {
            std::string data;
            if (!readFile(entry.path(), data)) {
                ++rejects;
            } else {
                rows.push_back({rel, static_cast<std::uintmax_t>(data.size()), sha256(data)});
                bytes += data.size();
            }
        }
        it.increment(ec);
        if (ec) { ec.clear(); ++rejects; }
    }
    std::sort(rows.begin(), rows.end(), [](const Row &a, const Row &b) { return a.path < b.path; });
    std::ostringstream canonical;
    for (const auto &row : rows) canonical << "file|" << row.path << "|" << row.bytes << "|" << row.digest << "\n";
    std::cout << "files=" << rows.size() << "\nbytes=" << bytes << "\nlinks=" << links
        << "\nrejects=" << rejects << "\nindex_sha256=" << sha256(canonical.str()) << "\n";
    for (const auto &row : rows) std::cout << "file|" << row.path << "|" << row.bytes << "|" << row.digest << "\n";
}
