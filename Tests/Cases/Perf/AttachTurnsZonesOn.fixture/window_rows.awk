# The zones windows a spans file closed, from its bytes as `od -A n -v -t u1
# -w1` lists them: each row is four little-endian words (start, end, task,
# the name's byte length with the row's kind from bit 32), then the name,
# unpadded, so a row starts wherever the one before it ended. Prints the
# rows of kind 6 (a window) holding five words.
{ byte[count++] = $1 }
END {
    at = 0
    windows = 0
    while (at + 32 <= count) {
        length_ = byte[at + 24] + 256 * byte[at + 25] + 65536 * byte[at + 26] + 16777216 * byte[at + 27]
        kind = byte[at + 28] + 256 * byte[at + 29] + 65536 * byte[at + 30] + 16777216 * byte[at + 31]
        if (kind == 6 && length_ == 40) windows++
        at += 32 + length_
    }
    print windows
}
