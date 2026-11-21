# The last two records of a flight ring's samples file, from its bytes as
# `od -A n -v -t u2 -w8` lists them: one line a word, whose fourth 16-bit
# field holds the head's bits 48-63, the record's size in words (head
# included) in its low 11 bits and its kind above them (PerfSamples.jet
# perf_samples_head). The file holds whole records, oldest first, so each
# starts where the one before it ended. Prints the next-to-last record's
# kind, then the last record's kind and size, then the words past the last
# whole record (0 when the file ends on one).
{ top[count++] = $4 }
END {
    at = 0
    before = -1
    kind = -1
    size = 0
    while (at < count) {
        if (top[at] % 2048 == 0 || at + top[at] % 2048 > count) break
        before = kind
        kind = int(top[at] / 2048)
        size = top[at] % 2048
        at += size
    }
    print before, kind, size, count - at
}
