# hot.sh PLAIN ZONED: whether ZONED's hot code (its first read+execute load
# segment) exceeds PLAIN's by no more than its zone sites' stubs, and up to
# 15 bytes a site of the site itself and the padding that keeps a grown
# function's successors aligned. ZONED names its sites in its
# `jet.zone_sites` part note (JetBackend Image/StaticLink.jet
# link_zone_write): past the note's 16-byte head and the 16-byte name, the
# site count, then per site a rel32 from the field's end to the site and a
# zero word. A site, while zones are off, is a `test eax, imm32` (a9) over
# the rel32 of the jump to its stub; the stub ends in the first jump back
# to the instruction after the site.
hot() {
    objdump -p "$1" | awk '/LOAD/ { getline; if ($6 == "r-x") { print $2; exit } }'
}
plain=$(($(hot "$1")))
zoned=$(($(hot "$2")))
objdump -p "$2" > zoned.headers
od -An -v -tu1 "$2" > zoned.bytes
objdump -d "$2" > zoned.asm
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
        getline
        size[rows] = hex($2)
    }
    next
}
FILENAME == ARGV[2] {
    for (i = 1; i <= NF; i++) b[n++] = $i
    next
}
{
    # Each jmp of the disassembly: its address, length and target.
    if (split($0, f, "\t") < 3 || f[3] !~ /^jmp /) next
    sub(/:$/, "", f[1])
    gsub(/ /, "", f[1])
    split(f[3], w, " ")
    jumps++
    jat[jumps] = hex(f[1])
    jlen[jumps] = split(f[2], x, " ")
    jto[jumps] = hex(w[2])
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
        end = 0
        for (j = 1; j <= jumps && !end; j++) {
            if (jat[j] >= stub && jto[j] == address + 5) end = jat[j] + jlen[j]
        }
        if (!end) { print "-3"; exit }
        sum += end - stub
    }
    print sum + 15 * count
}' zoned.headers zoned.bytes zoned.asm)
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
