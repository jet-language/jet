# hot.sh PLAIN ZONED: whether ZONED's hot code (its first read+execute load
# segment) exceeds PLAIN's by no more than the bytes its zone sites skip,
# and up to 15 bytes a site of the padding that keeps a grown function's
# successors aligned. ZONED names its sites in its `jet.zone_sites` part
# note (JetBackend Image/StaticLink.jet link_zone_write): past the note's
# 16-byte head and the 16-byte name, the site count, then per site a rel32
# from the field's end to the site and a zero word. A site, while zones are
# off, is a `jmp rel32` (e9) whose displacement is the span it skips.
hot() {
    objdump -p "$1" | awk '/LOAD/ { getline; if ($6 == "r-x") { print $2; exit } }'
}
plain=$(($(hot "$1")))
zoned=$(($(hot "$2")))
objdump -p "$2" > zoned.headers
od -An -v -tu1 "$2" > zoned.bytes
spans=$(awk '
function hex(s,    i, v) {
    sub(/^0x/, "", s)
    v = 0
    for (i = 1; i <= length(s); i++) v = v * 16 + index("0123456789abcdef", substr(s, i, 1)) - 1
    return v
}
function u32(at) { return b[at] + 256 * (b[at + 1] + 256 * (b[at + 2] + 256 * b[at + 3])) }
function offset(address,    k) {
    for (k = 1; k <= rows; k++) {
        if (kind[k] == "LOAD" && address >= vaddr[k] && address < vaddr[k] + size[k]) return address - vaddr[k] + off[k]
    }
    return -1
}
NR == FNR {
    if ($1 == "LOAD" || $1 == "NOTE") {
        rows++
        kind[rows] = $1
        off[rows] = hex($3)
        vaddr[rows] = hex($5)
        getline
        size[rows] = hex($2)
    }
    next
}
{ for (i = 1; i <= NF; i++) b[n++] = $i }
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
        rel = u32(off[note] + 40 + 8 * s)
        if (rel >= 2147483648) rel -= 4294967296
        site = offset(vaddr[note] + 44 + 8 * s + rel)
        if (site < 0 || b[site] != 233) { print "-2"; exit }
        sum += u32(site + 1)
    }
    print sum + 15 * count
}' zoned.headers zoned.bytes)
if [ "$spans" = -1 ]; then
    echo "the zoned image has no jet.zone_sites note"
elif [ "$spans" = -2 ]; then
    echo "a site the note names is not a jmp rel32"
elif [ $((zoned - plain)) -le "$spans" ]; then
    echo "hot code grows by the sites' spans alone"
else
    echo "hot code grows by $((zoned - plain)) bytes; the sites' spans and padding allow $spans"
fi
