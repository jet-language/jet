// #1414 C++20 peer: direct USTAR inspection using the standard library only.
// No libarchive code or linkage is claimed; this reader owns the same frozen
// byte-level contract as the Jet adapter.
#include <algorithm>
#include <charconv>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <iterator>
#include <limits>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <system_error>
#include <vector>

namespace {

using Byte = unsigned char;
using Wide = unsigned __int128;

struct Options {
    std::string format = "tar";
    std::string filter = "*.md";
    long long max_size = 64;
    long long max_entries = 10000;
    std::string input;
    std::string output;
};

struct Number {
    bool valid = false;
    Wide value = 0;
};

struct Report {
    std::vector<std::string> rows;
    std::set<std::string> seen;
    std::size_t entries = 0;
    std::size_t accepted = 0;
    Wide bytes = 0;
    std::size_t rejected = 0;
    std::size_t traversal = 0;
    std::size_t duplicates = 0;
    std::size_t malformed = 0;
    std::size_t oversize = 0;
    std::size_t limited = 0;
};

bool utf8_valid(std::string_view text) {
    std::size_t i = 0;
    while (i < text.size()) {
        const auto first = static_cast<Byte>(text[i]);
        std::size_t width = 0;
        Wide code = 0;
        if (first <= 0x7f) {
            width = 1;
            code = first;
        } else if (first >= 0xc2 && first <= 0xdf) {
            width = 2;
            code = first & 0x1f;
        } else if (first >= 0xe0 && first <= 0xef) {
            width = 3;
            code = first & 0x0f;
        } else if (first >= 0xf0 && first <= 0xf4) {
            width = 4;
            code = first & 0x07;
        } else {
            return false;
        }
        if (i + width > text.size()) return false;
        for (std::size_t j = 1; j < width; ++j) {
            const auto next = static_cast<Byte>(text[i + j]);
            if ((next & 0xc0) != 0x80) return false;
            code = (code << 6) | (next & 0x3f);
        }
        if ((width == 3 && code < 0x800) || (width == 4 && code < 0x10000) ||
            code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) {
            return false;
        }
        i += width;
    }
    return true;
}

std::optional<std::string> field_text(
    const std::vector<Byte> &bytes,
    std::size_t start,
    std::size_t width
) {
    if (start > bytes.size() || width > bytes.size() - start) return std::nullopt;
    std::size_t length = 0;
    while (length < width && bytes[start + length] != 0) ++length;
    std::string text;
    text.reserve(length);
    for (std::size_t i = 0; i < length; ++i) {
        text.push_back(static_cast<char>(bytes[start + i]));
    }
    if (!utf8_valid(text)) return std::nullopt;
    return text;
}

Number number_field(const std::vector<Byte> &bytes, std::size_t start, std::size_t width) {
    if (start > bytes.size() || width > bytes.size() - start || width == 0) return {};
    const Byte first = bytes[start];
    if ((first & 0x80) != 0) {
        Wide value = first & 0x7f;
        for (std::size_t i = 1; i < width; ++i) {
            if (value > (static_cast<Wide>(-1) >> 8)) return {};
            value = (value << 8) | bytes[start + i];
        }
        return {true, value};
    }
    Wide value = 0;
    bool seen = false;
    for (std::size_t i = 0; i < width; ++i) {
        const Byte byte = bytes[start + i];
        if (byte == 0 || byte == ' ') {
            if (seen) break;
            continue;
        }
        if (byte < '0' || byte > '7') return {};
        seen = true;
        value = value * 8 + (byte - '0');
    }
    return {true, value};
}

bool checksum_ok(const std::vector<Byte> &bytes, std::size_t offset) {
    const Number stored = number_field(bytes, offset + 148, 8);
    if (!stored.valid) return false;
    Wide sum = 0;
    for (std::size_t i = 0; i < 512; ++i) {
        sum += (i >= 148 && i < 156) ? 32 : bytes[offset + i];
    }
    return stored.value == sum;
}

bool zero_block(const std::vector<Byte> &bytes, std::size_t offset) {
    for (std::size_t i = 0; i < 512; ++i) {
        if (bytes[offset + i] != 0) return false;
    }
    return true;
}

bool zero_tail(const std::vector<Byte> &bytes, std::size_t offset) {
    for (std::size_t i = offset; i < bytes.size(); ++i) {
        if (bytes[i] != 0) return false;
    }
    return true;
}

bool safe_path(std::string_view path) {
    if (path.empty() || path.front() == '/' || path.front() == '\\') return false;
    if (path.size() > 1 && path[1] == ':') return false;
    std::string normalized(path);
    for (char &byte : normalized) {
        if (byte == '\\') byte = '/';
    }
    std::size_t start = 0;
    while (start <= normalized.size()) {
        const std::size_t end = normalized.find('/', start);
        const std::size_t length = end == std::string::npos ? normalized.size() - start : end - start;
        if (normalized.compare(start, length, "..") == 0) return false;
        if (end == std::string::npos) break;
        start = end + 1;
    }
    return true;
}

bool glob_match(std::string_view pattern, std::string_view path) {
    std::size_t pattern_index = 0;
    std::size_t path_index = 0;
    std::size_t star = std::string::npos;
    std::size_t star_match = 0;
    while (path_index < path.size()) {
        if (pattern_index < pattern.size()) {
            const unsigned char pattern_byte = static_cast<Byte>(pattern[pattern_index]);
            if (pattern_byte == '?' || pattern[pattern_index] == path[path_index]) {
                ++pattern_index;
                ++path_index;
                continue;
            }
            if (pattern_byte == '*') {
                star = pattern_index++;
                star_match = path_index;
                continue;
            }
        }
        if (star != std::string::npos) {
            pattern_index = star + 1;
            path_index = ++star_match;
            continue;
        }
        return false;
    }
    while (pattern_index < pattern.size() && pattern[pattern_index] == '*') ++pattern_index;
    return pattern_index == pattern.size();
}

std::string entry_kind(Byte byte) {
    if (byte == 0 || byte == '0') return "file";
    if (byte == '1') return "hardlink";
    if (byte == '2') return "symlink";
    if (byte == '3') return "char";
    if (byte == '4') return "block";
    if (byte == '5') return "directory";
    if (byte == '6') return "fifo";
    if (byte == '7') return "reserved";
    return {};
}

std::string decimal(Wide value) {
    if (value == 0) return "0";
    std::string result;
    while (value != 0) {
        result.push_back(static_cast<char>('0' + value % 10));
        value /= 10;
    }
    std::reverse(result.begin(), result.end());
    return result;
}

bool parse_signed(std::string_view text, long long &value) {
    if (text.empty()) return false;
    const auto *first = text.data();
    const auto *last = first + text.size();
    const auto parsed = std::from_chars(first, last, value);
    return parsed.ec == std::errc{} && parsed.ptr == last;
}

bool option_value(
    int argc,
    char **argv,
    int &index,
    std::string_view name,
    std::string &value
) {
    const std::string_view arg(argv[index]);
    if (arg == name) {
        if (index + 1 >= argc) return false;
        value = argv[++index];
        return true;
    }
    const std::string prefix = std::string(name) + "=";
    if (arg.starts_with(prefix)) {
        value = std::string(arg.substr(prefix.size()));
        return !value.empty();
    }
    return false;
}

bool parse_options(int argc, char **argv, Options &options) {
    bool after_separator = false;
    for (int index = 1; index < argc; ++index) {
        const std::string_view arg(argv[index]);
        if (!after_separator && arg == "--") {
            after_separator = true;
            continue;
        }
        if (!after_separator && arg == "--help") {
            std::cout << "usage: cli-archive-filter INPUT [--format tar|ustar] [--filter GLOB]"
                         " [--max-size N] [--max-entries N] [--output FILE]\n";
            return false;
        }
        std::string value;
        if (!after_separator && option_value(argc, argv, index, "--format", value)) {
            options.format = value;
            continue;
        }
        if (!after_separator && option_value(argc, argv, index, "--filter", value)) {
            options.filter = value;
            continue;
        }
        if (!after_separator && option_value(argc, argv, index, "--output", value)) {
            options.output = value;
            continue;
        }
        if (!after_separator && (arg == "--format" || arg.starts_with("--format=") ||
                                 arg == "--filter" || arg.starts_with("--filter=") ||
                                 arg == "--output" || arg.starts_with("--output="))) {
            return false;
        }
        if (!after_separator && (arg == "--max-size" || arg.starts_with("--max-size="))) {
            if (!option_value(argc, argv, index, "--max-size", value) ||
                !parse_signed(value, options.max_size)) {
                return false;
            }
            continue;
        }
        if (!after_separator && (arg == "--max-entries" || arg.starts_with("--max-entries="))) {
            if (!option_value(argc, argv, index, "--max-entries", value) ||
                !parse_signed(value, options.max_entries)) {
                return false;
            }
            continue;
        }
        if (!options.input.empty() || (!after_separator && !arg.empty() && arg.front() == '-')) return false;
        options.input = std::string(arg);
    }
    return !options.input.empty();
}

bool append_entry(Report &report, const Options &options, const std::string &path, const std::string &kind, Wide size) {
    if (!safe_path(path)) {
        ++report.traversal;
        ++report.rejected;
        return false;
    }
    if (!report.seen.insert(path).second) {
        ++report.duplicates;
        ++report.rejected;
        return false;
    }
    if (report.entries > static_cast<std::size_t>(options.max_entries)) {
        ++report.limited;
        ++report.rejected;
        return false;
    }
    if (!glob_match(options.filter, path)) return false;
    report.rows.push_back("path=" + path + "|type=" + kind + "|size=" + decimal(size));
    ++report.accepted;
    report.bytes += size;
    return true;
}

Report inspect(const std::vector<Byte> &raw, const Options &options) {
    Report report;
    std::size_t offset = 0;
    bool terminated = false;
    while (offset < raw.size()) {
        if (raw.size() - offset < 512) {
            ++report.malformed;
            ++report.rejected;
            break;
        }
        if (zero_block(raw, offset)) {
            if (!zero_tail(raw, offset)) {
                ++report.malformed;
                ++report.rejected;
            } else {
                terminated = true;
            }
            break;
        }
        ++report.entries;
        if (!checksum_ok(raw, offset)) {
            ++report.malformed;
            ++report.rejected;
            offset += 512;
            continue;
        }
        const auto magic = field_text(raw, offset + 257, 6);
        if (!magic || ((options.format == "ustar" && !magic->starts_with("ustar")) ||
                       (!magic->empty() && !magic->starts_with("ustar")))) {
            ++report.malformed;
            ++report.rejected;
            offset += 512;
            continue;
        }
        const Number size_number = number_field(raw, offset + 124, 12);
        if (!size_number.valid) {
            ++report.malformed;
            ++report.rejected;
            offset += 512;
            continue;
        }
        const Wide max_offset = static_cast<Wide>(std::numeric_limits<std::size_t>::max());
        if (size_number.value > max_offset - 1023) {
            if (size_number.value > static_cast<Wide>(options.max_size)) {
                ++report.oversize;
            } else {
                ++report.malformed;
            }
            ++report.rejected;
            break;
        }
        const Wide padded = ((size_number.value + 511) / 512) * 512;
        const Wide next_wide = static_cast<Wide>(offset) + 512 + padded;
        if (next_wide > raw.size()) {
            if (size_number.value > static_cast<Wide>(options.max_size)) {
                ++report.oversize;
            } else {
                ++report.malformed;
            }
            ++report.rejected;
            break;
        }
        const auto next_offset = static_cast<std::size_t>(next_wide);
        if (size_number.value > static_cast<Wide>(options.max_size)) {
            ++report.oversize;
            ++report.rejected;
            offset = next_offset;
            continue;
        }
        const auto name = field_text(raw, offset, 100);
        const auto prefix = field_text(raw, offset + 345, 155);
        if (!name || !prefix || name->empty() || raw[offset] == 0) {
            ++report.malformed;
            ++report.rejected;
            offset = next_offset;
            continue;
        }
        const std::string path = prefix->empty() ? *name : *prefix + "/" + *name;
        const std::string kind = entry_kind(raw[offset + 156]);
        if (kind.empty()) {
            ++report.malformed;
            ++report.rejected;
            offset = next_offset;
            continue;
        }
        append_entry(report, options, path, kind, size_number.value);
        offset = next_offset;
    }
    if (!terminated && offset >= raw.size()) {
        ++report.malformed;
        ++report.rejected;
    }
    return report;
}

std::vector<std::string> report_lines(const Report &report, const Options &options) {
    std::vector<std::string> lines{
        "format=" + options.format,
        "entries=" + std::to_string(report.entries),
        "accepted=" + std::to_string(report.accepted),
        "bytes=" + decimal(report.bytes),
        "rejected=" + std::to_string(report.rejected),
        "traversal=" + std::to_string(report.traversal),
        "duplicates=" + std::to_string(report.duplicates),
        "malformed=" + std::to_string(report.malformed),
        "oversize=" + std::to_string(report.oversize),
        "limited=" + std::to_string(report.limited),
    };
    lines.insert(lines.end(), report.rows.begin(), report.rows.end());
    return lines;
}

} // namespace

int main(int argc, char **argv) {
    Options options;
    if (!parse_options(argc, argv, options)) return argc > 1 && std::string_view(argv[1]) == "--help" ? 0 : 2;
    if (options.format != "tar" && options.format != "ustar") {
        std::cout << "error=unsupported-format\n";
        return 0;
    }
    if (options.max_size < 0 || options.max_entries < 0) {
        std::cout << "error=invalid-limit\n";
        return 0;
    }
    std::ifstream input(options.input, std::ios::binary);
    if (!input) return 2;
    std::vector<Byte> raw(
        (std::istreambuf_iterator<char>(input)),
        std::istreambuf_iterator<char>()
    );
    Report report = inspect(raw, options);
    std::sort(report.rows.begin(), report.rows.end());
    const auto lines = report_lines(report, options);
    std::ostream *stream = &std::cout;
    std::ofstream output;
    if (!options.output.empty()) {
        output.open(options.output, std::ios::binary | std::ios::trunc);
        if (!output) return 2;
        stream = &output;
    }
    for (const std::string &line : lines) *stream << line << '\n';
}
