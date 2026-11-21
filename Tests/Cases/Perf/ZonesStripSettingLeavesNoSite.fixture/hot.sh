# hot.sh JET PLAIN ZONED: whether ZONED's hot code (its first read+execute
# load segment) exceeds PLAIN's by no more than its zone sites' stubs, and
# up to 15 bytes a site of the site itself and the padding that keeps a
# grown function's successors aligned. The segments are read with
# `JET debug IMAGE --do segments`, one row a program header: its type, then
# `offset`, `address` (hexadecimal), `file` and `memory` (bytes) and
# `flags`. ZONED names its sites in its `jet.zone_sites` part note (JetBackend
# Image/StaticLink.jet link_zone_write): past the note's 16-byte head and the
# 16-byte name, the site count, then per site a rel32 from the field's end to
# the site and a zero word. A site, while zones are off, is a `test eax,
# imm32` (a9) over the rel32 of the jump to its stub; the stub ends in the
# first jump back to the instruction after the site, read from the image's
# bytes (a jmp rel32, e9, or rel8, eb).
jet=$1
shift
hot() {
    "$jet" debug "$1" --do segments | awk '$1 == "LOAD" && $11 == "r-x" { print $7; exit }'
}
plain=$(($(hot "$1")))
zoned=$(($(hot "$2")))
"$jet" debug "$2" --do segments > zoned.headers
od -An -v -tu1 "$2" > zoned.bytes
spans=$(awk '
function hex(s,    i, v) {
    sub(/^0x/, "", s)
    v = 0
    for (i = 1; i <= length(s); i++) v = v * 16 + index("0123456789abcdef", substr(s, i, 1)) - 1
    return v
}
function u32(at) { return b[at] + 256 * (b[at + 1] + 256 * (b[at + 2] + 256 * b[at + 3])) }
function s32(at,    v) {
    v = u32(at)
    return v >= 2147483648 ? v - 4294967296 : v
}
function offset(address,    k) {
    for (k = 1; k <= rows; k++) {
        if (kind[k] == "LOAD" && address >= vaddr[k] && address < vaddr[k] + size[k]) return address - vaddr[k] + off[k]
    }
    return -1
}
FILENAME == ARGV[1] {
    if ($1 == "LOAD" || $1 == "NOTE") {
        rows++
        kind[rows] = $1
        off[rows] = hex($3)
        vaddr[rows] = hex($5)
        size[rows] = $7
    }
    next
}
{
    for (i = 1; i <= NF; i++) b[n++] = $i
}
END {
    for (k = 1; k <= rows; k++) {
        if (kind[k] != "NOTE" || size[k] < 40) continue
        name = ""
        for (i = 16; i < 30; i++) name = name sprintf("%c", b[off[k] + i])
        if (name == "jet.zone_sites") note = k
    }
    if (!note) { print "-1"; exit }
    count = u32(off[note] + 32)
    sum = 0
    for (s = 0; s < count; s++) {
        address = vaddr[note] + 44 + 8 * s + s32(off[note] + 40 + 8 * s)
        site = offset(address)
        if (site < 0 || b[site] != 169) { print "-2"; exit }
        stub = address + 5 + s32(site + 1)
        at = offset(stub)
        end = 0
        for (j = 0; at >= 0 && j < 256 && !end; j++) {
            if (b[at + j] == 233 && stub + j + 5 + s32(at + j + 1) == address + 5) end = stub + j + 5
            if (b[at + j] == 235 && stub + j + 2 + (b[at + j + 1] >= 128 ? b[at + j + 1] - 256 : b[at + j + 1]) == address + 5) end = stub + j + 2
        }
        if (!end) { print "-3"; exit }
        sum += end - stub
    }
    print sum + 15 * count
}' zoned.headers zoned.bytes)
if [ "$spans" = -1 ]; then
    echo "the zoned image has no jet.zone_sites note"
elif [ "$spans" = -2 ]; then
    echo "a site the note names is not a test eax, imm32"
elif [ "$spans" = -3 ]; then
    echo "a site's stub never jumps back past the site"
elif [ $((zoned - plain)) -le "$spans" ]; then
    echo "hot code grows by the sites' stubs alone"
else
    echo "hot code grows by $((zoned - plain)) bytes; the sites' stubs and padding allow $spans"
fi
