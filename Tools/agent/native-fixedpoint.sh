#!/usr/bin/env bash
# Native bootstrap: native CLI seed -> jetc1 -> jetc2 -> jetc3. No Rust
# emission, release profile, or memory cap here; the caller owns the proofq
# reservation. The builder must be a real argv CLI with jet-runtime/ beside it
# (native-feed.mjs stage1/jet or stage2/jet), never the stage-zero factory
# jetc0, which only speaks the JET_BOOTSTRAP_* protocol.
# Host.jet writes <source root>/.jet/build/<resolved entry stem>.
# O1 is not exposed by the hosted CLI yet: build uses its native O0 path.
# Every stage execs under a 1 GiB RLIMIT_STACK (the native compiler runs on
# the main thread; O0 frames never share slots) with the factory's compiler
# identity in JET_COMPILER_IDENTITY (the CLI refuses to compile without it).
set -euo pipefail
usage() {
    printf '%s\n' 'usage: native-fixedpoint.sh --builder PATH --unit FILE.jet|DIR --out DIR [--factory-receipt BOOTSTRAP_RECEIPT] [--level o0|o1] [--stages 3]' \
        'identity: compiler_source_identity= of the stage1 factory bootstrap receipt (native-feed factory-native/), else $JET_COMPILER_IDENTITY (name@version#source-id)' \
        'NATIVE_FP_TIMEOUT: per-stage seconds; 0 (default) disables the limit'
}
fail() { printf 'native-fixedpoint: %s\n' "$*" >&2; exit 2; }
builder='' unit='' out='' record='' level=o0 stages=3
while (( $# )); do
    case $1 in
        --builder|--unit|--out|--factory-receipt|--level|--stages)
            (( $# >= 2 )) || fail "missing value for $1"
            case $1 in
                --builder) builder=$2 ;; --unit) unit=$2 ;; --out) out=$2 ;;
                --factory-receipt) record=$2 ;;
                --level) level=$2 ;; --stages) stages=$2 ;;
            esac
            shift 2 ;;
        --help|-h) usage; exit 0 ;;
        *) usage >&2; fail "unknown argument: $1" ;;
    esac
done
[[ -n $builder && -n $unit && -n $out ]] || { usage >&2; fail 'builder, unit and out are required'; }
[[ $level == o0 || $level == o1 ]] || fail 'level must be o0 or o1'
[[ $level != o1 ]] || fail 'native O1 not selectable from hosted CLI yet'
[[ $stages == 3 ]] || fail 'stages must be 3'
# A whole native compile of the compiler has never been timed; no default cap.
limit=${NATIVE_FP_TIMEOUT:-0}
[[ $limit =~ ^(0|[1-9][0-9]*)$ ]] || fail 'NATIVE_FP_TIMEOUT must be 0 (no limit) or a positive integer in seconds'
for tool in realpath timeout prlimit node readelf nm objdump cmp sha256sum stat find; do
    command -v "$tool" >/dev/null || fail "missing tool: $tool"
done
if [[ -n $record ]]; then
    [[ -f $record ]] || fail "factory receipt does not exist: $record"
    JET_COMPILER_IDENTITY=$(sed -n 's/^compiler_source_identity=//p' -- "$record")
fi
[[ ${JET_COMPILER_IDENTITY:-} =~ ^[^@#[:space:]]+@[^@#[:space:]]+#[^@#[:space:]]+$ ]] || fail 'compiler identity (name@version#source-id) is required from --factory-receipt or JET_COMPILER_IDENTITY; never invented'
export JET_COMPILER_IDENTITY
# The native CLI reads Core from JET_TOOLCHAIN_ROOT (JetCli Request.jet
# jet_cli_core_sources); run under the program export's Tools/agent/jet-env so
# the inline-C toolchain matches the factory's (stage1/README.md).
[[ -n ${JET_TOOLCHAIN_ROOT:-} && -d $JET_TOOLCHAIN_ROOT/Core ]] || fail 'JET_TOOLCHAIN_ROOT must name the program source tree (the directory holding Core/)'
timer=$(type -P time) || fail 'GNU time is required to measure peak RSS'
"$timer" --version >/dev/null 2>&1 || fail 'GNU time is required'
builder=$(realpath -e -- "$builder") || fail 'builder does not exist'
unit=$(realpath -e -- "$unit") || fail 'unit does not exist'
[[ -x $builder && -f $builder ]] || fail 'builder must be an executable file'
[[ -d $unit || ( -f $unit && $unit == *.jet ) ]] || fail 'unit must be a .jet file or package directory'
mkdir -p -- "$out" || fail 'cannot create output directory'
out=$(realpath -e -- "$out")
# Refuse reused receipts: a failed stage must never leave a stale successful chain.
for receipt in jetc1 jetc2 jetc3 stages.tsv comparisons.tsv triage.txt; do
    [[ ! -e $out/$receipt ]] || fail "output already contains $receipt (use a fresh directory)"
done
if [[ -f $unit ]]; then
    root=${unit%/*}
    ancestor=$root
    while :; do
        if [[ -f $ancestor/package.jet ]]; then root=$ancestor; break; fi
        [[ $ancestor != / ]] || break
        ancestor=${ancestor%/*}; ancestor=${ancestor:-/}
    done
    stem=${unit##*/}; stem=${stem%.jet}
else
    root=$unit
    stem=
fi
# Interim runtime pack is located beside the executable. Keep every link on
# the same pack; do not copy/rebuild it and do not replace a caller's resource.
[[ -d ${builder%/*}/jet-runtime ]] || fail "builder has no jet-runtime/ beside it: ${builder%/*}"
runtime=$(realpath -e -- "${builder%/*}/jet-runtime")
if [[ -e $out/jet-runtime || -L $out/jet-runtime ]]; then
    [[ $(realpath -e -- "$out/jet-runtime") == "$runtime" ]] || fail 'output runtime differs from builder runtime'
else
    ln -s -- "$runtime" "$out/jet-runtime" || fail 'cannot expose runtime beside stages'
fi
printf 'stage\tlevel\tseconds\tpeak_rss_kb\tbytes\tsha256\n' > "$out/stages.tsv"
printf 'pair\tidentical\n' > "$out/comparisons.tsv"
for stage in 1 2 3; do
    marker=$out/stage-$stage.started
    touch -- "$marker"
    status=0
    "$timer" -f '%e\t%M' -o "$out/stage-$stage.time" \
        timeout --kill-after=30 "$limit" prlimit --stack=1073741824:1073741824 -- "$builder" build "$unit" \
        > "$out/stage-$stage.log" 2>&1 || status=$?
    # GNU time adds an exit-status line on failure; its final row is metrics.
    metrics=
    while IFS= read -r line; do metrics=$line; done < "$out/stage-$stage.time"
    if (( status != 0 )); then
        printf '%s\t%s\t%s\t-\t-\n' "$stage" "$level" "$metrics" >> "$out/stages.tsv"
        printf 'native-fixedpoint: stage %s build failed (exit %s); see %s\n' "$stage" "$status" "$out/stage-$stage.log" >&2
        exit 3
    fi
    binaries=()
    if [[ -n $stem ]]; then
        binary=$root/.jet/build/$stem
        [[ -f $binary && -x $binary && $binary -nt $marker ]] || fail "stage $stage produced no fresh executable at $binary"
        binaries=("$binary")
    else
        # Package output resolution belongs to the CLI. Accept exactly one
        # freshly written executable in the documented build directory.
        [[ -d $root/.jet/build ]] || fail "stage $stage produced no build directory"
        mapfile -d '' -t binaries < <(find "$root/.jet/build" -maxdepth 1 -type f -executable -newer "$marker" ! -name '*.o' -print0)
        (( ${#binaries[@]} == 1 )) || fail "stage $stage produced ${#binaries[@]} fresh executables; pass its entry .jet explicitly"
    fi
    cp -- "${binaries[0]}" "$out/jetc$stage" || fail 'cannot copy stage executable'
    chmod +x -- "$out/jetc$stage"
    digest=$(sha256sum "$out/jetc$stage"); digest=${digest%% *}
    printf '%s\t%s\t%s\t%s\t%s\n' "$stage" "$level" "$metrics" "$(stat -c %s "$out/jetc$stage")" "$digest" >> "$out/stages.tsv"
    builder=$out/jetc$stage
done
# Detailed triage is developer-only: section fingerprints, defined symbols,
# then bytes of the first differing symbol and both disassemblies. ELF64 LE
# is the current native target. Structural/header differences stay explicit.
triage() {
    node - "$1" "$2" <<'NODE'
const fs = require('node:fs');
const crypto = require('node:crypto');
const {execFileSync} = require('node:child_process');
const paths = process.argv.slice(2);
const hash = b => crypto.createHash('sha256').update(b).digest('hex');
function image(path) {
    const b = fs.readFileSync(path);
    if (b.subarray(0, 6).toString('hex') !== '7f454c460201') throw Error(`${path}: expected ELF64 little-endian`);
    const u64 = p => Number(b.readBigUInt64LE(p));
    const off = u64(40), width = b.readUInt16LE(58), count = b.readUInt16LE(60);
    if (!count || width < 64 || off + width * count > b.length) throw Error(`${path}: invalid/extended section table`);
    const sections = Array.from({length: count}, (_, i) => {
        const p = off + i * width;
        return {nameAt: b.readUInt32LE(p), type: b.readUInt32LE(p + 4), addr: u64(p + 16), off: u64(p + 24), size: u64(p + 32), link: b.readUInt32LE(p + 40), ent: u64(p + 56)};
    });
    const bytes = s => s.type === 8 ? Buffer.alloc(0) : b.subarray(s.off, s.off + s.size);
    const str = (s, p) => { const t = bytes(s); const end = t.indexOf(0, p); return t.subarray(p, end < 0 ? t.length : end).toString(); };
    const names = sections[b.readUInt16LE(62)];
    if (!names) throw Error(`${path}: missing section-name table`);
    for (const s of sections) s.name = str(names, s.nameAt);
    const symbols = [];
    for (const s of sections.filter(s => s.type === 2 || s.type === 11)) {
        if (s.ent < 24) throw Error(`${path}: invalid symbol entry size`);
        for (let p = s.off; p < s.off + s.size; p += s.ent) {
            const idx = b.readUInt16LE(p + 6), section = sections[idx];
            const name = str(sections[s.link], b.readUInt32LE(p));
            if (!name || !section || !idx || section.type === 8) continue;
            symbols.push({name, section, value: u64(p + 8), size: u64(p + 16)});
        }
    }
    symbols.sort((a, c) => a.section.off - c.section.off || a.value - c.value || a.name.localeCompare(c.name));
    let section = null, upper = 0, value = -1, next = 0;
    for (let i = symbols.length - 1; i >= 0; i--) {
        const s = symbols[i], start = s.value - s.section.addr;
        if (section !== s.section) {
            section = s.section; upper = section.addr + section.size; value = -1;
        } else if (s.value !== value) {
            upper = next;
        }
        const size = s.size || upper - s.value;
        s.bytes = bytes(section).subarray(start, start + size);
        value = s.value; next = s.value;
    }
    const key = s => `${s.section.name}\0${s.name}`;
    return {b, sections, symbols, bytes, bySymbol: new Map(symbols.map(s => [key(s), s])), key};
}
try {
    const [a, b] = paths.map(image);
    for (const path of paths) {
        console.log(`=== ${path}: sections ===\n${execFileSync('readelf', ['-SW', path], {encoding:'utf8'})}`);
        console.log(`=== ${path}: defined symbols ===\n${execFileSync('nm', ['--defined-only', '--size-sort', path], {encoding:'utf8', maxBuffer:64*1024*1024})}`);
    }
    console.log('section\tleft_sha256\tright_sha256');
    let firstSection = '';
    for (const name of new Set([...a.sections, ...b.sections].map(s => s.name))) {
        const x = a.sections.find(s => s.name === name), y = b.sections.find(s => s.name === name);
        const left = x ? hash(a.bytes(x)) : 'missing', right = y ? hash(b.bytes(y)) : 'missing';
        console.log(`${name}\t${left}\t${right}`);
        if (!firstSection && (left !== right || x?.size !== y?.size)) firstSection = name;
    }
    let firstSymbol = '';
    for (const s of a.symbols) {
        const other = b.bySymbol.get(a.key(s));
        if (!other || !s.bytes.equals(other.bytes)) { firstSymbol = s.name; break; }
    }
    if (!firstSymbol) firstSymbol = b.symbols.find(s => !a.bySymbol.has(b.key(s)))?.name || '';
    console.log(`FIRST_DIFFERING_SECTION: ${firstSection || '<ELF headers/layout>'}`);
    console.log(`FIRST_DIFFERING_SYMBOL: ${firstSymbol || '<no symbol: headers/layout or section data>'}`);
    if (firstSymbol) for (const path of paths) console.log(`=== ${path}: ${firstSymbol} ===\n${execFileSync('objdump', ['-d', `--disassemble=${firstSymbol}`, path], {encoding:'utf8', maxBuffer:64*1024*1024})}`);
} catch (err) { console.error(`triage failed: ${err.message}`); process.exitCode = 2; }
NODE
}
first_same=no
if cmp -s "$out/jetc1" "$out/jetc2"; then first_same=yes; fi
printf 'jetc1==jetc2\t%s\n' "$first_same" >> "$out/comparisons.tsv"
printf 'REPORT jetc1 == jetc2: %s\n' "$first_same"
fixed=no
if cmp -s "$out/jetc2" "$out/jetc3"; then fixed=yes; fi
printf 'jetc2==jetc3\t%s\n' "$fixed" >> "$out/comparisons.tsv"
if [[ $first_same == no || $fixed == no ]]; then
    : > "$out/triage.txt"
    if [[ $first_same == no ]]; then
        printf '=== REPORT jetc1 vs jetc2 ===\n' >> "$out/triage.txt"
        triage "$out/jetc1" "$out/jetc2" >> "$out/triage.txt" 2>&1 || fail "triage failed; see $out/triage.txt"
    fi
    if [[ $fixed == no ]]; then
        printf '=== GATE jetc2 vs jetc3 ===\n' >> "$out/triage.txt"
        triage "$out/jetc2" "$out/jetc3" >> "$out/triage.txt" 2>&1 || fail "triage failed; see $out/triage.txt"
        symbol='<no symbol>'
        while IFS= read -r line; do
            if [[ $line == 'FIRST_DIFFERING_SYMBOL: '* ]]; then symbol=${line#FIRST_DIFFERING_SYMBOL: }; fi
        done < "$out/triage.txt"
        printf 'NOT A FIXED POINT: %s\n' "$symbol"
        exit 1
    fi
fi
printf 'NATIVE FIXED POINT: %s\n' "$digest"
