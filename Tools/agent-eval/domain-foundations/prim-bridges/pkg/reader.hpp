#pragma once
#include <cstdint>

namespace bridge {
class Reader {
public:
    Reader();
    int64_t next_line_length();
private:
    int64_t fd_;
};
}
