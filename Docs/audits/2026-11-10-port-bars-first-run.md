# Port bars, first run: every Jet tool twin against its original

Date: 2026-11-10. Card #5072 c14. Judge: Tools/gauntlet/PortBars.jet (SUB G6LeadB-7, not yet landed), run on dev c460ba91e5 plus that SUB, with the dev-built jet (seed-t9d), `--runs 5`. The machine was loaded (loadavg about 50 on 32 CPUs) while it ran; the rounds were interleaved, so both sides shared the load, and any overlap between the two sides' rounds counts as inconclusive, which fails the bar.

This is a dated finding, not live status. The card holds the live state.

## The four bars

Each entry is judged only against its own original. A port may replace its original only when all four bars hold.

1. **Parity.** On every fixture case, both programs exit with the expected code and print the expected file byte for byte.
2. **Speed.** 5 interleaved rounds of every case per side. Jet wins a measure (wall time, peak resident memory) only when every Jet round beats every original round. Overlap is inconclusive and fails. Jet must win both. The ratio shown is Jet/original of the medians.
3. **Tokens.** Jet source tokens must be at most the original's. Comments, the `#!` line and Jet `#Test` blocks are not counted; a string literal is one token; a word is one token; each operator is one token.
4. **Reasoning.** A reviewer record (`review.json`) must score Jet at least the original on the fixed checklist (types visible at use, every failure path visible at the call, no hidden global or mutable state, data flows top to bottom, one obvious way per operation, effects visible), and the measured proxies (branch count, control nesting depth) must be no worse for Jet. **No entry has a review record yet**, so this bar fails everywhere today; the proxies are still measured.

## Summary

| Entry | What it does | Parity | Wall Jet/orig | Peak mem Jet/orig | Tokens Jet / orig (ratio) | Branches Jet / orig | Nesting Jet / orig | Verdict |
|---|---|---|---|---|---|---|---|---|
| batch10 | The Lander's batch10.sh and its Jet twin Batch10.jet | PASS | 0.58 | 1.21 (loses) | 2548 / 587 (4.34x) | 127 / 30 | 4 / 3 | FAIL |
| census-snap2 | Main's precommit census census-snap2.sh and its Jet twin CensusSnap2.jet | PASS | 0.64 | 1.00 (overlap) | 3260 / 856 (3.81x) | 109 / 29 | 3 / 3 | FAIL |
| integ-apply6 | Main's integration applier integ-apply6.sh and its Jet twin IntegApply6.jet | PASS | 0.71 | 1.00 (overlap) | 4250 / 1149 (3.70x) | 148 / 58 | 4 / 4 | FAIL |
| integ-land4 | The Lander's integ-land4.sh and its Jet twin IntegLand4.jet | PASS | 0.61 (overlap) | 0.29 | 2453 / 670 (3.66x) | 98 / 39 | 3 / 2 | FAIL |
| integrate10 | Main's integration pass integrate10.sh and its Jet twin Integrate10.jet | PASS | 0.83 | 2.21 (loses) | 5843 / 1616 (3.62x) | 185 / 64 | 4 / 5 | FAIL |
| jetc-trace | The compiler trace summarizer summarize.mjs and its Jet twin Summarize.jet | PASS | 0.31 | 0.25 | 1579 / 1396 (1.13x) | 66 / 54 | 3 / 4 | FAIL |
| lane-commit | Main's lane lander lane-commit.sh and its Jet twin LaneCommit.jet | PASS | 0.41 | 0.31 | 3801 / 1133 (3.35x) | 130 / 57 | 2 / 3 | FAIL |
| streamb10 | The Lander's streamB10.sh and its Jet twin StreamB10.jet | PASS | 1.28 (loses) | 1.71 (loses) | 873 / 166 (5.26x) | 37 / 4 | 2 / 2 | FAIL |
| tower-spec-apply | Main's Tower spec apply tower-spec-apply.mjs and its Jet twin TowerSpecApply.jet | PASS | 0.80 (overlap) | 0.87 | 3314 / 1273 (2.60x) | 144 / 53 | 4 / 4 | FAIL |
| verify-rows | Main orchestrator helper | PASS | 0.49 (overlap) | 1.25 (loses) | 1325 / 181 (7.32x) | 59 / 3 | 2 / 1 | FAIL |

Reading the table: parity holds on all 10. In wall time, Jet's median is lower on 9 of 10, but it wins cleanly (every Jet round faster) only on 6; 3 overlap, and streamb10 loses. In peak memory, Jet wins cleanly on 4 (integ-land4, jetc-trace, lane-commit, tower-spec-apply), loses on 4 and overlaps on 2. Only jetc-trace and lane-commit pass the speed bar. Every Jet twin is longer, at 1.13x to 7.32x the original's tokens, and has more branches in all 10. So the token bar is what blocks these ports. The reasoning bar also needs a review record for every entry.

## Per entry

### batch10

The Lander's batch10.sh and its Jet twin Batch10.jet (#5072 c15): hold the SUBs that wait for a seed, run integrate10.sh, and when the census or a generator fails, quarantine the SUBs it names (with every later SUB of the same lead) and run the rest once more. The fixtures hold six batches, their SUBs and a stand-in integrate10.sh that prints its arguments and exits with the code in rc/BATCH.

- Original: `Tools/agent/orchestrator/batch10.sh` (shell (bash), 72 lines)
- Jet twin: `Tools/agent/orchestrator/Batch10.jet` (334 lines)
- Fixture cases: 6 (args per case: `b1 subs/A-1 subs/B-1/ subs/C-1`; `b2 subs/A-1` exit 2; `b3 subs/L-1 subs/L-2 subs/M-1 subs/N-1`; `b4 subs/P-1 subs/Q-1 subs/P-2 subs/R-1 subs/S-1` exit 3; `b5 subs/T-1` exit 1; `b6 subs/U-1` exit 1)

| Bar | Result | Measurement |
|---|---|---|
| parity | PASS | 6/6 cases identical |
| speed | FAIL | wall Jet/original 0.58 (209.7 ms against 361.1 ms; every Jet round below every original round); peak memory Jet/original 1.21 (13.8 MB against 11.3 MB; lost: every original round below every Jet round); medians of 5 interleaved rounds |
| tokens | FAIL | Jet 2548, original 587 (+1961, Jet/original 4.34) |
| reasoning | FAIL | no review record `Tools/gauntlet/entries/batch10/review.json`; branches Jet 127, original 30 (+97); control nesting Jet 4, original 3 (+1) |
| verdict | FAIL | loses speed, tokens, reasoning; the original keeps running |

#### Original: Tools/agent/orchestrator/batch10.sh

```sh
#!/usr/bin/env bash
# batch2.sh BATCH SUB... : one integration cycle that does not let one bad submission hold back the rest.
#   1. SUBs whose meta says `seed:` something other than none wait for that seed and are listed as HELD.
#   2. integrate3.sh BATCH with the rest.
#   3. If the census failed, a SUB is QUARANTINED when
#      - a census row names one of its touched files (Core/X/Y.jet, <corelib>/Core/X/Y.jet, or module path Core/X/Y), or
#      - an error's quoted source line (`NNN | text` under an `Error [` header in census/*.err) is a line its patch adds
#        (this attributes failures in concatenated units such as the runtime pack's unit.jet, which have no rows);
#      with every later SUB of the same lead. integrate3.sh BATCH-r then runs once more with the others.
# integ-land3.sh lands from whichever pass is all OK.
# The original of gauntlet tool entry batch10; Batch10.jet beside it is its Jet twin. The batches live in
# $JET_SCRATCH_ROOT/lanes/Main/integ (Tools/agent/roots.sh); integrate10.sh is the one in $JET_ORCHESTRATOR_DIR,
# this directory unless set.
set -u
. "$(dirname "$0")/../roots.sh"
M=$JET_SCRATCH_ROOT/lanes/Main; G=${JET_ORCHESTRATOR_DIR:-$(dirname "$0")}
# One batch at a time: two runs of the same BATCH would write into one integ dir.
exec 9>"$M/integ/.batch.lock"; flock -n 9 || { echo "batch: another batch holds the lock"; exit 75; }
BATCH=${1:?batch}; shift
run=()
for s in "$@"; do
  s=${s%/}; sd=$(awk -F': *' '$1=="seed"{print $2}' "$s/meta" 2>/dev/null)
  if [ -n "$sd" ] && [ "$sd" != none ]; then echo "HELD $(basename "$s") (seed: $sd)"; else run+=("$s"); fi
done
"$G/integrate10.sh" "$BATCH" "${run[@]}"; irc=$?
# rc 2: the export or the apply step stopped (integrate10 printed why); there is no census to attribute.
[ $irc = 2 ] && { echo "batch: integrate stopped before the census (rc 2)"; exit 2; }
I=$M/integ/$BATCH
if [ -f "$I/REGENFAIL" ]; then
  # Blame the SUBs whose added lines carry a name the failing generator quotes (`core.math.__from_bits_f32` gives
  # __from_bits_f32), with every later SUB of the same lead.
  names=$(cat "$I"/regen.*.out | awk '{while (match($0, /`[^`]+`/)) {t=substr($0, RSTART+1, RLENGTH-2); n=split(t, p, "."); if (length(p[n]) > 3) print p[n]; $0=substr($0, RSTART+RLENGTH)}}' | sort -u)
  keep=(); blamed=()
  for s in "${run[@]}"; do
    n=$(basename "$s"); hit=0
    [ "$(cat "$I/$n.status" 2>/dev/null)" = NOAPPLY ] && continue
    [ -n "$names" ] && awk '/^\+[^+]/' "$s/patch" | grep -qwF -f <(printf '%s\n' "$names") && hit=1
    lead=${n%-*}; for x in "${blamed[@]}"; do [ "${x%-*}" = "$lead" ] && hit=1; done
    if [ $hit = 1 ]; then blamed+=("$n"); else keep+=("$s"); fi
  done
  [ ${#blamed[@]} -gt 0 ] || { echo "batch: a generator failed but no submission adds a name it quotes; see $I/REPORT.md"; exit 1; }
  echo "QUARANTINED (regen) ${blamed[*]}"
  "$G/integrate10.sh" "$BATCH-r" "${keep[@]}"
  exit $?
fi
grep -q '^Census rc=0 ' "$I/REPORT.md" && exit 0
rows=$(cat "$I/census/"*keep-going.tsv "$I/census.out" 2>/dev/null | awk -F'\t' 'NF>=2{print $2}' | sed -e 's#<corelib>/##' -e 's#:[0-9.]*$##' -e 's#\.jet$##' | sort -u)
# Source lines quoted under Error headers (not warnings), trimmed.
errlines=$(cat "$I/census/"*.err 2>/dev/null | awk '/^Error \[/{e=1; next} /^(Warning|Note) \[/{e=0} e && /^ *[0-9]+ \| /{sub(/^ *[0-9]+ \| */,""); sub(/[ \t]+$/,""); if (length($0)>8) print}' | sort -u)
keep=(); blamed=()
for s in "${run[@]}"; do
  n=$(basename "$s"); hit=0
  [ "$(cat "$I/$n.status" 2>/dev/null)" = NOAPPLY ] && continue
  if [ -n "$rows" ]; then
    while IFS= read -r f; do
      [ -n "$f" ] || continue; m=${f%.jet}
      printf '%s\n' "$rows" | grep -qxF -e "$m" -e "$f" && { hit=1; break; }
    done < "$I/$n.files"
  fi
  if [ $hit = 0 ] && [ -n "$errlines" ]; then
    P="$s/patch"; [ -s "$I/$n.rebased" ] && P="$I/$n.rebased"
    awk '/^\+[^+]/{sub(/^\+[ \t]*/,""); sub(/[ \t]+$/,""); print}' "$P" | grep -qxF -f <(printf '%s\n' "$errlines") && hit=1
  fi
  # A quarantined SUB takes every later SUB of the same lead with it (they may stack on it).
  lead=${n%-*}; for b in "${blamed[@]}"; do [ "${b%-*}" = "$lead" ] && hit=1; done
  if [ $hit = 1 ]; then blamed+=("$n"); else keep+=("$s"); fi
done
[ ${#blamed[@]} -gt 0 ] || { echo "batch: the census failed but no row or error line names a submission; see $I/REPORT.md"; exit 1; }
echo "QUARANTINED ${blamed[*]}"
[ ${#keep[@]} -gt 0 ] || exit 1
"$G/integrate10.sh" "$BATCH-r" "${keep[@]}"
```

#### Jet twin: Tools/agent/orchestrator/Batch10.jet

```jet
#!/usr/bin/env jet
// Batch10.jet BATCH SUB...: one integration cycle that does not let one bad
// submission hold back the rest.
//   1. SUBs whose meta says `seed:` something other than none wait for that
//      seed and are listed as HELD.
//   2. integrate10.sh BATCH with the rest.
//   3. If the census failed, a SUB is QUARANTINED when a census row names
//      one of its touched files (Core/X/Y.jet, <corelib>/Core/X/Y.jet, or
//      module path Core/X/Y), or an error's quoted source line (`NNN | text`
//      under an `Error [` header in census/*.err) is a line its patch adds,
//      with every later SUB of the same lead. integrate10.sh BATCH-r then
//      runs once more with the others.
//
// The Jet twin of batch10.sh (gauntlet tool entry batch10): the batches live
// in $JET_SCRATCH_ROOT/lanes/Main/integ, as Tools/agent/roots.sh sets it, and
// integrate10.sh is the one in $JET_ORCHESTRATOR_DIR, this program's
// directory unless set.
use {
    files: core.files
    process: core.process
    sys: core.sys
    term: core.term
}

env_or: fn(name: String, fallback: String) -[Env]> String Never! {
    value: process.env_get(name) ?? return fallback.copy()
    if value == "" -> fallback.copy() else -> value
}

// What the machine's jet/env file sets `name` to (the default of its
// `${NAME:-default}` form), None when it sets nothing.
env_file_value: fn(name: String) -[Env, FS]> String? Never! {
    home: env_or("HOME", "")
    text: files.read("{env_or("XDG_CONFIG_HOME", "{home}/.config")}/jet/env") ?? return None
    defaulted: "${{{name}:-"
    found := ""
    seen := false
    loop line in text.lines() {
        parts: line.trim().removeprefix("export ").partition("=")
        if parts.sep == "" || parts.head != name -> next
        value := parts.tail.removeprefix("\"").removesuffix("\"").removeprefix("'").removesuffix("'")
        if value.starts_with(defaulted) && value.ends_with("}}") -> value = value.removeprefix(defaulted).removesuffix("}}")
        found = value.replace("$HOME", home)
        seen = true
    }
    if seen -> Val(found) else -> None
}

// A root as Tools/agent/roots.sh sets it: the environment, else the jet/env
// file, else `fallback`.
root: fn(name: String, fallback: String) -[Env, FS]> String Never! {
    found: env_or(name, "")
    if found != "" -> return found
    env_file_value(name) ?? fallback.copy()
}

scratch_root: fn() -[Env, FS]> String Never! {
    root("JET_SCRATCH_ROOT", root("JET_STATE_ROOT", "{env_or("XDG_CACHE_HOME", "{env_or("HOME", "")}/.cache")}/jet-dev"))
}

// `text` without its trailing newlines, as `$(...)` gives it.
chomp: fn(text: String) -> String Never! {
    out := text.copy()
    loop out.ends_with("\n") -> out = out.removesuffix("\n")
    out
}

is_word: fn(byte: U8) -> Bool Never! {
    code: Int.from_u8(byte)
    (code >= 48 && code <= 57) || (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || code == 95
}

is_blank: fn(byte: U8) -> Bool Never! { byte == U8{32} || byte == U8{9} }

// `text` without the spaces and tabs at its start (`front`) or its end.
trim_blanks: fn(text: String, front: Bool) -> String Never! {
    raw: text.bytes()
    start := 0
    end := raw.len()
    if front {
        loop start < end && is_blank(raw[start]) -> start += 1
    } else {
        loop end > start && is_blank(raw[end - 1]) -> end -= 1
    }
    String.from_bytes(raw.slice(start, end)) ?? text.copy()
}

// What `awk -F': *' '$1=="seed"{print $2}' META` prints, as `$(...)` keeps it.
seed_of: fn(meta: String) -> String Never! {
    values := [String]{}
    loop line in meta.lines() {
        parts: line.partition(":")
        if parts.head != "seed" -> next
        rest := parts.tail.copy()
        loop rest.starts_with(" ") -> rest = rest.removeprefix(" ")
        values.push(rest.partition(":").head)
    }
    chomp(values.join("\n"))
}

// The text of every file in `dir` whose name ends with `suffix` (and starts
// with `prefix`), joined as `cat` joins them.
cat_matching: fn(dir: String, prefix: String, suffix: String) -[FS]> String Never! {
    names := [String]{}
    loop entry in (files.list_dir(dir) ?? return "") {
        if !entry.name.starts_with(".") && entry.name.starts_with(prefix) && entry.name.ends_with(suffix) -> names.push(entry.name.copy())
    }
    names.sort()
    out := ""
    loop name in names -> out = "{out}{files.read("{dir}/{name}") ?? ""}"
    out
}

// The names a generator quotes: the last dot-part, longer than 3, of each
// `quoted` span.
quoted_names: fn(text: String) -> [String] Never! {
    out := [String]{}
    loop line in text.lines() {
        raw: line.bytes()
        at := 0
        loop at < raw.len() {
            if raw[at] != 96 {
                at += 1
                next
            }
            close := at + 1
            loop close < raw.len() && raw[close] != 96 -> close += 1
            if close >= raw.len() -> break
            if close == at + 1 {
                at += 1
                next
            }
            quoted: String.from_bytes(raw.slice(at + 1, close)) ?? ""
            last: quoted.rpartition(".").tail
            if last.len() > 3 && !out.contains(last) -> out.push(last)
            at = close + 1
        }
    }
    out
}

// The lines a patch adds (`^\+[^+]`), without the `+`.
added_lines: fn(patch: String) -> [String] Never! {
    out := [String]{}
    loop line in patch.lines() {
        raw: line.bytes()
        if raw.len() >= 2 && raw[0] == 43 && raw[1] != 43 -> out.push(line.removeprefix("+"))
    }
    out
}

// Whether `line` holds `name` as a whole word, as `grep -w` matches it.
has_word: fn(line: String, name: String) -> Bool Never! {
    parts: line.split(name).to_list()
    loop index in 1..<parts.len() {
        before: parts[index - 1].bytes()
        after: parts[index].bytes()
        if before.len() > 0 && is_word(before[before.len() - 1]) -> next
        if after.len() > 0 && is_word(after[0]) -> next
        return true
    }
    false
}

// The file a census row names: column 2 without `<corelib>/`, its final
// `:line.col` and `.jet`.
row_path: fn(field: String) -> String Never! {
    parts: field.partition("<corelib>/")
    out := "{parts.head}{parts.tail}"
    if parts.sep == "" -> out = field.copy()
    colon: out.rfind(":")
    if colon >= 0 {
        rest: out.bytes().slice(colon + 1, out.bytes().len())
        numeric := true
        loop byte in rest {
            code: Int.from_u8(byte)
            if code != 46 && (code < 48 || code > 57) -> numeric = false
        }
        if numeric -> out = String.from_bytes(out.bytes().slice(0, colon)) ?? out
    }
    out.removesuffix(".jet")
}

census_rows: fn(integ: String) -[FS]> [String] Never! {
    text: "{cat_matching("{integ}/census", "", "keep-going.tsv")}{files.read("{integ}/census.out") ?? ""}"
    out := [String]{}
    loop line in text.lines() {
        fields: line.split("\t").to_list()
        if fields.len() < 2 -> next
        path: row_path(fields[1])
        if !out.contains(path) -> out.push(path)
    }
    out
}

// Whether `line` is a quoted source line `  NNN | text`; gives the text.
quoted_source: fn(line: String) -> String? Never! {
    raw: line.bytes()
    at := 0
    loop at < raw.len() && raw[at] == 32 -> at += 1
    digits: at
    loop at < raw.len() && Int.from_u8(raw[at]) >= 48 && Int.from_u8(raw[at]) <= 57 -> at += 1
    if at == digits || at + 3 > raw.len() || raw[at] != 32 || raw[at + 1] != 124 || raw[at + 2] != 32 -> return None
    at += 3
    loop at < raw.len() && raw[at] == 32 -> at += 1
    Val(String.from_bytes(raw.slice(at, raw.len())) ?? "")
}

// Source lines quoted under Error headers (not warnings), trimmed.
error_lines: fn(integ: String) -[FS]> [String] Never! {
    out := [String]{}
    in_error := false
    loop line in cat_matching("{integ}/census", "", ".err").lines() {
        if line.starts_with("Error [") {
            in_error = true
            next
        }
        if line.starts_with("Warning [") || line.starts_with("Note [") -> in_error = false
        if !in_error -> next
        text: trim_blanks(quoted_source(line) ?? next, false)
        if text.len() > 8 && !out.contains(text) -> out.push(text)
    }
    out
}

lead_of: fn(name: String) -> String Never! {
    if name.contains("-") -> name.slice(0, name.rfind("-")) else -> name.copy()
}

// Whether an earlier blamed SUB has the same lead.
same_lead: fn(blamed: [String], name: String) -> Bool Never! {
    lead: lead_of(name)
    loop other in blamed {
        if lead_of(other) == lead -> return true
    }
    false
}

no_apply: fn(integ: String, name: String) -[FS]> Bool Never! {
    chomp(files.read("{integ}/{name}.status") ?? "") == "NOAPPLY"
}

size_of: fn(path: String) -[FS]> Int Never! {
    if files.stat(path) == {
        .Ok(info) -> info.size
        .Err(_) -> -1
    }
}

// Runs integrate10.sh with inherited stdio and gives its exit code.
integrate: fn(orchestrator: String, batch: String, subs: [String]) -[Exec, IO, Time.Wait]> Int Never! {
    argv := [String]{"{orchestrator}/integrate10.sh", batch.copy()}
    argv.extend(subs.copy())
    receipt: process.cmd(argv).stdin(.Inherit).stdout(.Inherit).stderr(.Inherit).run() ?? {
        term.eprint("batch: {argv[0]}: {err}")
        return 127
    }
    if receipt.signal != None -> 128 + (receipt.signal ?? 0) else -> receipt.code
}

fail: fn(message: String, code: Int) -[Exec, IO]> Never {
    print(message)
    process.exit(code)
}

run: fn() {
    main: "{scratch_root()}/lanes/Main"
    orchestrator: env_or("JET_ORCHESTRATOR_DIR", files.dirname(sys.executable()))
    // One batch at a time: two runs of the same BATCH would write into one integ dir.
    _held: files.try_lock("{main}/integ/.batch.lock") ?? fail("batch: another batch holds the lock", 75)
    args: process.args()
    if args.len() == 0 {
        term.eprint("usage: jet Tools/agent/orchestrator/Batch10.jet BATCH SUB...")
        process.exit(1)
    }
    batch: args[0]
    run := [String]{}
    loop given in args.slice(1, args.len()) {
        sub: given.removesuffix("/")
        seed: seed_of(files.read("{sub}/meta") ?? "")
        if seed != "" && seed != "none" -> print("HELD {files.basename(sub)} (seed: {seed})") else -> run.push(sub)
    }
    // rc 2: the export or the apply step stopped (integrate10 printed why); there is no census to attribute.
    if integrate(orchestrator, batch, run) == 2 -> fail("batch: integrate stopped before the census (rc 2)", 2)
    integ: "{main}/integ/{batch}"
    if files.is_file("{integ}/REGENFAIL") {
        // Blame the SUBs whose added lines carry a name the failing generator quotes (`core.math.__from_bits_f32` gives
        // __from_bits_f32), with every later SUB of the same lead.
        names: quoted_names(cat_matching(integ, "regen.", ".out"))
        keep := [String]{}
        blamed := [String]{}
        loop sub in run {
            name: files.basename(sub)
            if no_apply(integ, name) -> next
            added: added_lines(files.read("{sub}/patch") ?? "")
            hit: added.any(line -> names.any(quoted -> has_word(line, quoted))) || same_lead(blamed, name)
            if hit -> blamed.push(name) else -> keep.push(sub.copy())
        }
        if blamed.len() == 0 -> fail("batch: a generator failed but no submission adds a name it quotes; see {integ}/REPORT.md", 1)
        print("QUARANTINED (regen) {blamed.join(" ")}")
        process.exit(integrate(orchestrator, "{batch}-r", keep))
    }
    loop line in (files.read("{integ}/REPORT.md") ?? "").lines() {
        if line.starts_with("Census rc=0 ") -> process.exit(0)
    }
    rows: census_rows(integ)
    errors: error_lines(integ)
    keep := [String]{}
    blamed := [String]{}
    loop sub in run {
        name: files.basename(sub)
        if no_apply(integ, name) -> next
        hit := false
        if rows.len() > 0 {
            loop path in (files.read("{integ}/{name}.files") ?? "").lines() {
                if path != "" && (rows.contains(path.removesuffix(".jet")) || rows.contains(path)) {
                    hit = true
                    break
                }
            }
        }
        if !hit && errors.len() > 0 {
            patch: if size_of("{integ}/{name}.rebased") > 0 -> "{integ}/{name}.rebased" else -> "{sub}/patch"
            hit = added_lines(files.read(patch) ?? "").any(line -> errors.contains(trim_blanks(trim_blanks(line, true), false)))
        }
        // A quarantined SUB takes every later SUB of the same lead with it (they may stack on it).
        if hit || same_lead(blamed, name) -> blamed.push(name) else -> keep.push(sub.copy())
    }
    if blamed.len() == 0 -> fail("batch: the census failed but no row or error line names a submission; see {integ}/REPORT.md", 1)
    print("QUARANTINED {blamed.join(" ")}")
    if keep.len() == 0 -> process.exit(1)
    process.exit(integrate(orchestrator, "{batch}-r", keep))
}
```

### census-snap2

Main's precommit census census-snap2.sh and its Jet twin CensusSnap2.jet (#5072 c15): snapshot HEAD of the dev worktree plus the named files (or take a prepared tree), then the seed check, the keep-going build, the guards gate, the runtime pass, the self-check of the compiler just built and, for a named Jetpack file, the hosted Jetpack pass, each reported in one line with its rows. The fixtures hold a stand-in jet (seed/jet) that writes the keep-going report each case's STANDIN_* variables ask for, stand-ins for Main's helpers (scratch/Main) and a bare repository (work.git) whose HEAD differs from the worktree beside it.

- Original: `Tools/agent/orchestrator/census-snap2.sh` (shell (bash), 113 lines)
- Jet twin: `Tools/agent/orchestrator/CensusSnap2.jet` (401 lines)
- Fixture cases: 15 (args per case: `(none)`; `(none)` exit 1; `(none)`; `(none)` exit 1; `(none)` exit 1; `(none)` exit 1; `(none)` exit 1; `(none)` exit 1; `(none)` exit 1; `docs/x.md Jetpack/Store/Source/A.jet`; `Jetpack/Store/Source/A.jet` exit 1; `Jetpack/Store/Source/A.jet`; `(none)`; `(none)` exit 2; `docs/a.txt docs/gone.txt notes/new.txt` exit 1)

| Bar | Result | Measurement |
|---|---|---|
| parity | PASS | 15/15 cases identical |
| speed | FAIL | wall Jet/original 0.64 (2030.1 ms against 3185.2 ms; every Jet round below every original round); peak memory Jet/original 1.00 (47.5 MB against 47.6 MB; inconclusive: the rounds overlap); medians of 5 interleaved rounds |
| tokens | FAIL | Jet 3260, original 856 (+2404, Jet/original 3.81) |
| reasoning | FAIL | no review record `Tools/gauntlet/entries/census-snap2/review.json`; branches Jet 109, original 29 (+80); control nesting Jet 3, original 3 (0) |
| verdict | FAIL | loses speed, tokens, reasoning; the original keeps running |

#### Original: Tools/agent/orchestrator/census-snap2.sh

```sh
#!/usr/bin/env bash
# census-snap.sh <file>...: collect-all census of a SNAPSHOT: HEAD of the dev worktree plus the named
# worktree files (the orchestrator's own uncommitted edits), so other lanes' uncommitted work in the
# shared tree never enters it. Output: $JET_SCRATCH_ROOT/discovery/r14p/census/{check.*,build.*,selfcheck.*,runtime/}.
# Passes, all in package form in the snapshot: `jet check Compiler/JetCli` on the seed (sema of the
# CLI and every package it depends on), then `jet build Compiler` with keep-going, so optimize,
# lowering, object and link rows are caught before a commit lands (a check-only gate let two build
# breaks through), then the runtime pack and the self-check with the compiler just built.
# CENSUS_BUILD=0 skips everything after the check. CENSUS_JET_DIR names another compiler install
# (default: the seed). Exit: 0 clean; 1 rows/failure; 2 setup failure.
# Never edit this file in place while it runs (bash reads scripts incrementally): write a new file and mv.
# The original of gauntlet tool entry census-snap2; CensusSnap2.jet beside it is its Jet twin. The dev
# worktree is $JET_DEV_WORKTREE, the seed $JET_SEED, the lock is in $JET_RUN_ROOT, and jetenv.sh,
# census-errors.mjs, guards-gate.mjs, guards-known.txt, runtime-pack-check.sh and jpwrap.sh are Main's,
# in $JET_STATE_ROOT/scratch/Main (Tools/agent/roots.sh).
set -uo pipefail
. "$(dirname "$0")/../roots.sh"
G=$JET_STATE_ROOT/scratch/Main
# Its own lock AND its own directories: precommit censuses never queue behind lanes' long checks, and
# never share r14 with Tools/agent/land/census-snap.sh (vbatch's launch census), whose different lock
# let a precommit delete the runtime pack directory under a running batch census (vb6).
mkdir -p "$JET_RUN_ROOT"; exec 9>"$JET_RUN_ROOT/precommit-census.lock"; flock 9
W=$JET_DEV_WORKTREE
# CENSUS_TREE names a prepared tree (integrate.sh: origin/dev plus submitted patches) used as is, with
# CENSUS_OUT for its reports; the named files then only select the Jetpack pass.
S=${CENSUS_TREE:-$JET_SCRATCH_ROOT/discovery/r14p/snap}
OUT=${CENSUS_OUT:-$JET_SCRATCH_ROOT/discovery/r14p/census}
C=${CENSUS_JET_DIR:-$(readlink -f "$JET_SEED")}
ID=${CENSUS_JET_IDENTITY:-$(cat "$C/IDENTITY")}
if [ -n "${CENSUS_TREE:-}" ]; then rm -rf "$OUT"; mkdir -p "$OUT"; else
rm -rf "$S" "$OUT"; mkdir -p "$S" "$OUT"
( cd "$W" && git archive HEAD | tar -x -C "$S" ) || { echo "EXPORT FAILED"; exit 2; }
# A named file the worktree no longer has is a deletion: remove it from the snapshot too.
for f in "$@"; do if [ -e "$W/$f" ]; then mkdir -p "$S/$(dirname "$f")" && cp "$W/$f" "$S/$f"; else rm -f "$S/$f"; fi; done
fi
cd "$S" || exit 2
JET_COMPILER_IDENTITY=$ID JET_TOOLCHAIN_ROOT=${CENSUS_TOOLCHAIN_ROOT:-$S} JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/check-keep-going.tsv" \
  prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 bash "$G/jetenv.sh" "$S" "$C/jet" check --threads 12 Compiler/JetCli > "$OUT/check.out" 2> "$OUT/check.err"
crc=$?
echo "check rc=$crc"
# The keep-going report is the census: a dependency package's errors reach the stderr only as E0626.
node "$G/census-errors.mjs" "$OUT/check-keep-going.tsv" "$S" 2>&1 | awk 'NR<=21' | cut -c1-200
[ "$crc" = 0 ] || exit 1
[ "${CENSUS_BUILD:-1}" = 0 ] && exit 0
rm -rf Compiler/JetCli/.jet/build
JET_COMPILER_IDENTITY=$ID JET_TOOLCHAIN_ROOT=${CENSUS_BUILD_ROOT:-$S} \
  JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/build-keep-going.tsv" \
  timeout 1800 prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 bash "$G/jetenv.sh" "$S" "$C/jet" build --threads 12 Compiler > "$OUT/build.out" 2> "$OUT/build.err"
brc=$?
rows=$(awk 'NR>1' "$OUT/build-keep-going.tsv" 2>/dev/null | wc -l)
echo "build rc=$brc rows=$rows"
awk -F'\t' 'NR>1{print $1"\t"$3"\t"$4}' "$OUT/build-keep-going.tsv" 2>/dev/null | sort | uniq -c | sort -rn | cut -c1-200 | head -10
[ "$brc" = 0 ] && [ "$rows" = 0 ] || { head -c 600 "$OUT/build.err"; exit 1; }
# Guards pass (CENSUS_GUARDS=0 skips): tests/guards on the seed, failing only on a guard row or closed-list
# problem not in guards-known.txt. Without it, 18 hand-written closed lists landed unseen (c65 run 6).
if [ "${CENSUS_GUARDS:-1}" = 1 ]; then
  # 2400 s (was 900): post42h's guards hit 900 s at machine load 62 with no failing guard.
  JET_COMPILER_IDENTITY=$ID JET_TOOLCHAIN_ROOT=$S timeout "${CENSUS_GUARDS_TIMEOUT:-2400}" taskset -c 16-27 bash "$G/jetenv.sh" "$S" "$C/jet" test tests/guards > "$OUT/guards.out" 2>&1
  node "$G/guards-gate.mjs" "$OUT/guards.out" "$G/guards-known.txt"; grc=$?
  echo "guards rc=$grc"
  [ "$grc" = 0 ] || exit 1
fi
# The compiler `jet build Compiler` just built (JetCli's executable target `jet`).
SELF=$S/Compiler/JetCli/.jet/build/jet
# Runtime pass, before the self-check and on every compiler census: the freshly built compiler lowers the
# runtime pack exactly as chain s1's pack step does. Compile-time evaluation runs runtime routes on the
# pack's own IR (runtime.ir, #5073 A3), which the compiler reads from `jet-runtime/` beside its
# executable, so the self-check below (like chain s2) needs this pack; any failure fails the commit.
# A tree whose Compiler/build.jet builds the pack (Compiler/Toolchain/RuntimePack.jet) already wrote it beside
# jet during the build pass above; older trees build it here through the packager.
if [ -f "$S/Compiler/Toolchain/RuntimePack.jet" ]; then
  RP="$(dirname "$(readlink -f "$SELF")")/jet-runtime"
  [ -e "$RP" ] || [ ! -d "$S/Compiler/JetCli/.jet/build/jet-runtime" ] || ln -sfn "$S/Compiler/JetCli/.jet/build/jet-runtime" "$RP"
  if [ -f "$RP/native.key" ]; then echo "runtime rc=0 (jet build Compiler)"; else echo "runtime rc=1 (no jet-runtime/native.key beside $SELF)"; exit 1; fi
else
  bash "$G/runtime-pack-check.sh" "$SELF" "$S" "$OUT/runtime" "$ID" || exit 1
  ln -sfn "$OUT/runtime/out" "$(dirname "$(readlink -f "$SELF")")/jet-runtime"
fi
# Self-check pass (CENSUS_SELFCHECK=0 skips): the compiler just built checks the compiler packages with
# keep-going, as chain s2 does; a sema/comptime row here would stop s2 (e.g. an unlowered inline module).
if [ "${CENSUS_SELFCHECK:-1}" = 1 ]; then
  JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$S" JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/selfcheck-keep-going.tsv" \
    timeout 1200 prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 bash "$G/jetenv.sh" "$S" "$SELF" check --threads 12 Compiler/JetCli > "$OUT/selfcheck.out" 2> "$OUT/selfcheck.err"
  src=$?
  srows=$(awk 'NR>1' "$OUT/selfcheck-keep-going.tsv" 2>/dev/null | wc -l)
  echo "selfcheck rc=$src rows=$srows"
  node "$G/census-errors.mjs" "$OUT/selfcheck-keep-going.tsv" "$S" 2>&1 | awk 'NR<=11' | cut -c1-200
  [ "$src" = 0 ] && [ "$srows" = 0 ] || exit 1
  # Core pass (Land.jet census selfcheck-core): every Core body, called or not, checked by the compiler just
  # built. Without it a Core spelling no compiler accepts landed (cd58a15b81 fixed it) and stopped a release preflight.
  JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$S" JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/selfcheck-core-keep-going.tsv" \
    timeout 1200 prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 bash "$G/jetenv.sh" "$S" "$SELF" check --threads 12 Core > "$OUT/selfcheck-core.out" 2> "$OUT/selfcheck-core.err"
  crc=$?
  crows=$(awk 'NR>1' "$OUT/selfcheck-core-keep-going.tsv" 2>/dev/null | wc -l)
  echo "selfcheck-core rc=$crc rows=$crows"
  node "$G/census-errors.mjs" "$OUT/selfcheck-core-keep-going.tsv" "$S" 2>&1 | awk 'NR<=11' | cut -c1-200
  [ "$crc" = 0 ] && [ "$crows" = 0 ] || exit 1
fi
# Hosted Jetpack pass, when a named file is under Jetpack/: the compiler just built assembles and builds the
# hosted jetpack exactly as vbatch.sh does. The compiler census alone never compiles Jetpack, which let
# 5ef77a52ad land three E0209 rows that stopped vb6 before any suite ran.
jp=0; for f in "$@"; do case "$f" in Jetpack/*) jp=1 ;; esac; done
if [ "$jp" = 1 ] && [ "${CENSUS_JETPACK:-1}" = 1 ]; then
  JH=$(mktemp -d -p "$OUT")
  ( cd "$S" && env HOME="$JH" JET_HOME="$JH/.jet" JET="$SELF" JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$S" \
      JETPACK_WORKER=precommit JET_STATE_ROOT="$OUT/jetpack-state" timeout 1200 bash "$G/jpwrap.sh" check --hosted ) > "$OUT/jetpack.log" 2>&1
  rm -rf "$JH"
  if grep -q 'JETPACK HOSTED OK' "$OUT/jetpack.log"; then echo "jetpack rc=0"; else
    echo "jetpack rc=1 (see $OUT/jetpack.log)"; grep -E '^(Error|  -->)' "$OUT/jetpack.log" | head -20 | cut -c1-200; exit 1
  fi
fi
exit 0
```

#### Jet twin: Tools/agent/orchestrator/CensusSnap2.jet

```jet
#!/usr/bin/env jet
// CensusSnap2.jet <file>...: collect-all census of a SNAPSHOT: HEAD of the
// dev worktree plus the named worktree files (the orchestrator's own
// uncommitted edits), so other lanes' uncommitted work in the shared tree
// never enters it. Output: $JET_SCRATCH_ROOT/discovery/r14p/census.
// Passes, all in package form in the snapshot:
//   1. `jet check Compiler/JetCli` on the seed, with keep-going.
//   2. `jet build Compiler` with keep-going, so optimize, lowering, object
//      and link rows are caught before a commit lands.
//   3. tests/guards on the seed, failing only on a problem guards-known.txt
//      does not list (CENSUS_GUARDS=0 skips).
//   4. The runtime pack beside the compiler just built.
//   5. The self-check of Compiler/JetCli and of Core with that compiler
//      (CENSUS_SELFCHECK=0 skips).
//   6. The hosted Jetpack build, when a named file is under Jetpack/
//      (CENSUS_JETPACK=0 skips).
// CENSUS_TREE names a prepared tree used as is, with CENSUS_OUT for its
// reports; CENSUS_BUILD=0 stops after the check; CENSUS_JET_DIR names another
// compiler install (default: the seed). Exit: 0 clean; 1 rows or failure; 2
// setup failure.
//
// The Jet twin of census-snap2.sh (gauntlet tool entry census-snap2): the dev
// worktree is $JET_DEV_WORKTREE, the seed $JET_SEED, the lock is in
// $JET_RUN_ROOT, and jetenv.sh, census-errors.mjs, guards-gate.mjs,
// guards-known.txt, runtime-pack-check.sh and jpwrap.sh are Main's, in
// $JET_STATE_ROOT/scratch/Main, as Tools/agent/roots.sh sets them. As the
// original does after its `cd`, a relative path names a file in the snapshot
// from the check on.
use {
    files: core.files
    process: core.process
    sys: core.sys
    term: core.term
    time: core.time
}

// The stack every check and build pass runs with.
STACK_LIMIT: [String]{"prlimit", "--stack=1073741824:1073741824", "--"}

env_or: fn(name: String, fallback: String) -[Env]> String Never! {
    value: process.env_get(name) ?? return fallback.copy()
    if value == "" -> fallback.copy() else -> value
}

// What the machine's jet/env file sets `name` to (the default of its
// `${NAME:-default}` form), None when it sets nothing.
env_file_value: fn(name: String) -[Env, FS]> String? Never! {
    home: env_or("HOME", "")
    text: files.read("{env_or("XDG_CONFIG_HOME", "{home}/.config")}/jet/env") ?? return None
    defaulted: "${{{name}:-"
    found := ""
    seen := false
    loop line in text.lines() {
        parts: line.trim().removeprefix("export ").partition("=")
        if parts.sep == "" || parts.head != name -> next
        value := parts.tail.removeprefix("\"").removesuffix("\"").removeprefix("'").removesuffix("'")
        if value.starts_with(defaulted) && value.ends_with("}}") -> value = value.removeprefix(defaulted).removesuffix("}}")
        found = value.replace("$HOME", home)
        seen = true
    }
    if seen -> Val(found) else -> None
}

// A root as Tools/agent/roots.sh sets it: the environment, else the jet/env
// file, else `fallback`.
root: fn(name: String, fallback: String) -[Env, FS]> String Never! {
    found: env_or(name, "")
    if found != "" -> return found
    env_file_value(name) ?? fallback.copy()
}

state_root: fn() -[Env, FS]> String Never! { root("JET_STATE_ROOT", "{env_or("XDG_CACHE_HOME", "{env_or("HOME", "")}/.cache")}/jet-dev") }

scratch_root: fn() -[Env, FS]> String Never! { root("JET_SCRATCH_ROOT", state_root()) }

seed_dir: fn() -[Env, FS]> String Never! {
    seeds: root("JET_SEEDS", "{env_or("XDG_DATA_HOME", "{env_or("HOME", "")}/.local/share")}/jet-seeds")
    root("JET_SEED", "{seeds}/current")
}

// `text` without its trailing newlines, as `$(...)` gives it.
chomp: fn(text: String) -> String Never! {
    out := text.copy()
    loop out.ends_with("\n") -> out = out.removesuffix("\n")
    out
}

// The lines awk reads from `text`: a last line without its newline counts.
records: fn(text: String) -> [String] Never! {
    out := text.split("\n").to_list()
    if out.len() > 0 && out[out.len() - 1] == "" -> _ := out.pop()
    out
}

// `readlink -f path`: the path with every symlink resolved; a last part that
// does not exist stays as it is.
readlink_f: fn(path: String) -[FS, Env]> String Never! {
    whole: files.canonicalize(path) ?? {
        parent: files.canonicalize(if files.dirname(path) == "" -> "." else -> files.dirname(path)) ?? return path.copy()
        return "{parent}/{files.basename(path)}"
    }
    whole
}

// `ln -sfn target link`: a directory `link` (not a symlink) takes the link
// inside it.
link_sfn: fn(target: String, link: String) Never! -[FS, IO]> {
    at: if files.is_dir(link) && !files.is_symlink(link) -> "{link}/{files.basename(target)}" else -> link.copy()
    if files.lexists(at) -> files.remove(at) ?? ()
    files.symlink(target, at) ?? term.eprint("ln: failed to create symbolic link '{at}': {err}")
}

// Writes `bytes` to stdout now, before any child writes to it.
emit: fn(bytes: [U8]) Never! -[IO]> {
    out := term.stdout()
    out.write_bytes(bytes) ?? ()
    out.flush() ?? ()
}

say: fn(line: String) Never! -[IO]> { emit("{line}\n".bytes()) }

// The first `limit` lines of `text`, each cut to its first 200 bytes
// (`awk 'NR<=limit' | cut -c1-200`).
emit_head: fn(lines: [String], limit: Int) Never! -[IO]> {
    out := [U8]{}
    loop line in lines.slice(0, if lines.len() < limit -> lines.len() else -> limit) {
        raw: line.bytes()
        out.extend(raw.slice(0, if raw.len() < 200 -> raw.len() else -> 200))
        out.push(U8{10})
    }
    if out.len() > 0 -> emit(out)
}

// The exit status the shell gives a finished child: 124 for one `timeout`
// stopped, 128 + N for signal N.
status: fn(receipt: ProcessReceipt) -> Int Never! {
    if receipt.timed_out -> return 124
    if receipt.signal == .Val(number) -> return 128 + number
    receipt.code
}

// Seconds as `timeout` reads its DURATION (a number with an s, m, h or d
// suffix), None when it reads none.
duration_seconds: fn(text: String) -> Int? Never! {
    scale := 1
    digits := text.copy()
    if {
        text.ends_with("s") -> digits = text.removesuffix("s")
        text.ends_with("m") -> {
            digits = text.removesuffix("m")
            scale = 60
        }
        text.ends_with("h") -> {
            digits = text.removesuffix("h")
            scale = 3600
        }
        text.ends_with("d") -> {
            digits = text.removesuffix("d")
            scale = 86400
        }
    }
    if digits == "" -> return None
    value := 0
    loop byte in digits.bytes() {
        code: Int.from_u8(byte)
        if code < 48 || code > 57 -> return None
        value = value * 10 + code - 48
    }
    Val(value * scale)
}

// One pass: `spec` with its stdout in `out` and its stderr in `errors` (None:
// in `out` too, as `> out 2>&1` puts it); the shell's exit status. As in the
// shell, a file that can't be opened fails the pass (1) before it runs.
pass: fn(spec: ProcessSpec, out: String, errors: String?) -[Exec, FS, IO, Time.Wait]> Int Never! {
    files.write(out, "") ?? {
        term.eprint("census: {out}: {err}")
        return 1
    }
    if errors == .Val(path) {
        files.write(path, "") ?? {
            term.eprint("census: {path}: {err}")
            return 1
        }
    }
    full := spec.stdin(.Inherit).stdout(.Capture).output_limit(1073741824)
    full = if errors == None -> full.stderr(.Stdout) else -> full.stderr(.Capture)
    receipt: full.run() ?? {
        term.eprint("census: {err}")
        return 127
    }
    files.write(out, receipt.output) ?? term.eprint("census: {out}: {err}")
    if errors == .Val(path) -> files.write(path, receipt.errors) ?? term.eprint("census: {path}: {err}")
    status(receipt)
}

// Ends the run with `code`, `message` on stderr.
quit: fn(message: String, code: Int) -[Exec, IO]> Never {
    term.eprint(message)
    process.exit(code)
}

// `bash jetenv.sh tree jet args...` after `prefix`, on CPUs 16-27, with
// `vars` set.
jetenv: fn(g: String, tree: String, jet: String, args: [String], vars: [String: String], prefix: [String]) -[Exec]> ProcessSpec Never! {
    argv := prefix.copy()
    argv.extend([String]{"bash", "{g}/jetenv.sh", tree.copy(), jet.copy()})
    argv.extend(args.copy())
    spec := process.cmd(argv).cpu_affinity(loop cpu in 16..<28 -> cpu)
    loop (name, value) in vars -> spec = spec.env(name, value)
    spec
}

// The rows of a keep-going report (`awk 'NR>1' TSV | wc -l`).
row_count: fn(tsv: String) -[FS]> Int Never! {
    count: records(files.read(tsv) ?? return 0).len()
    if count > 1 -> count - 1 else -> 0
}

// What census-errors.mjs says about a keep-going report, its first `limit`
// lines.
census_errors: fn(g: String, tsv: String, tree: String, limit: Int) Never! -[Exec, IO, Time.Wait]> {
    receipt: process.cmd([String]{"node", "{g}/census-errors.mjs", tsv.copy(), tree.copy()}).stdin(.Inherit).stdout(.Capture).stderr(.Stdout).output_limit(1073741824).run() ?? {
        say("census: node: {err}")
        return
    }
    emit_head(records(receipt.output), limit)
}

// The build report's rows by kind, file and code, most frequent first
// (`sort | uniq -c | sort -rn | cut -c1-200 | head -10`; equal counts in
// reverse byte order).
emit_build_rows: fn(tsv: String) Never! -[FS, IO]> {
    text: files.read(tsv) ?? return
    rows: records(text)
    keys := [String]{}
    loop line in rows.slice(if rows.len() > 0 -> 1 else -> 0, rows.len()) {
        fields: line.split("\t").to_list()
        third: if fields.len() > 2 -> fields[2].copy() else -> ""
        fourth: if fields.len() > 3 -> fields[3].copy() else -> ""
        keys.push("{fields[0]}\t{third}\t{fourth}")
    }
    keys.sort()
    counted := [String]{}
    at := 0
    loop at < keys.len() {
        end := at + 1
        loop end < keys.len() && keys[end] == keys[at] -> end += 1
        count: "{end - at}"
        counted.push("{" ".repeat(if count.len() < 7 -> 7 - count.len() else -> 0)}{count} {keys[at]}")
        at = end
    }
    // The counts are padded to one width, so byte order is count order.
    counted.sort()
    emit_head(loop back in 0..<counted.len() -> counted[counted.len() - 1 - back], 10)
}

// The runtime pass: the pack the build wrote beside the compiler just built,
// else the packager's.
runtime: fn(g: String, tree: String, self_jet: String, out: String, id: String) Never! -[Exec, FS, IO, Time.Wait]> {
    built: "{tree}/Compiler/JetCli/.jet/build/jet-runtime"
    beside: "{files.dirname(readlink_f(self_jet))}/jet-runtime"
    if files.is_file("{tree}/Compiler/Toolchain/RuntimePack.jet") {
        if !files.exists(beside) && files.is_dir(built) -> link_sfn(built, beside)
        if files.is_file("{beside}/native.key") -> return say("runtime rc=0 (jet build Compiler)")
        say("runtime rc=1 (no jet-runtime/native.key beside {self_jet})")
        process.exit(1)
    }
    receipt: process.cmd([String]{"bash", "{g}/runtime-pack-check.sh", self_jet.copy(), tree.copy(), "{out}/runtime", id.copy()}).stdin(.Inherit).stdout(.Inherit).stderr(.Inherit).run() ?? quit("census: bash: {err}", 1)
    if status(receipt) != 0 -> process.exit(1)
    link_sfn("{out}/runtime/out", beside)
}

// The hosted Jetpack pass with the compiler just built.
jetpack: fn(g: String, tree: String, self_jet: String, out: String, id: String) Never! -[Env, Exec, FS, IO, Time.Wait]> {
    home: "{out}/tmp.jetpack.{sys.pid()}"
    files.create_dir_all(home) ?? ()
    log: "{out}/jetpack.log"
    spec: process.cmd([String]{"bash", "{g}/jpwrap.sh", "check", "--hosted"}).cwd(tree)
        .env("HOME", home).env("JET_HOME", "{home}/.jet").env("JET", self_jet).env("JET_COMPILER_IDENTITY", id)
        .env("JET_TOOLCHAIN_ROOT", tree).env("JETPACK_WORKER", "precommit").env("JET_STATE_ROOT", "{out}/jetpack-state")
        .timeout(time.duration_seconds(1200))
    _ := pass(spec, log, None)
    files.remove_all(home) ?? ()
    text: files.read(log) ?? ""
    if text.contains("JETPACK HOSTED OK") -> return say("jetpack rc=0")
    say("jetpack rc=1 (see {log})")
    emit_head(records(text).filter(line -> line.starts_with("Error") || line.starts_with("  -->")), 20)
    process.exit(1)
}

// The snapshot: HEAD of the worktree, then each named worktree file over it;
// a named file the worktree no longer has is a deletion.
snapshot: fn(worktree: String, tree: String, named: [String]) Never! -[Exec, FS, IO, Time.Wait]> {
    files.remove_all(tree) ?? ()
    files.create_dir_all(tree) ?? ()
    exported: process.pipeline([ProcessSpec]{
        process.cmd([String]{"git", "archive", "HEAD"}).cwd(worktree),
        process.cmd([String]{"tar", "-x", "-C", tree.copy()}).cwd(worktree),
    }) ?? {
        say("EXPORT FAILED")
        quit("census: {worktree}: {err}", 2)
    }
    if exported.errors != "" -> term.eprint(chomp(exported.errors))
    if !exported.success {
        say("EXPORT FAILED")
        process.exit(2)
    }
    loop path in named {
        if {
            files.exists("{worktree}/{path}") -> {
                files.create_dir_all("{tree}/{files.dirname(path)}") ?? ()
                files.copy("{worktree}/{path}", "{tree}/{path}") ?? term.eprint("cp: {worktree}/{path}: {err}")
            }
            else -> files.remove("{tree}/{path}") ?? ()
        }
    }
}

run: fn() {
    named: process.args()
    g: "{state_root()}/scratch/Main"
    run_root: root("JET_RUN_ROOT", "{scratch_root()}/run")
    files.create_dir_all(run_root) ?? ()
    // Its own lock AND its own directories: precommit censuses never queue behind lanes' long checks, and
    // never share r14 with Tools/agent/land/census-snap.sh (vbatch's launch census).
    _held: files.lock("{run_root}/precommit-census.lock") ?? quit("census: {run_root}/precommit-census.lock: {err}", 2)
    worktree: root("JET_DEV_WORKTREE", files.dirname(files.dirname(files.dirname(sys.executable()))))
    prepared: env_or("CENSUS_TREE", "")
    tree: env_or("CENSUS_TREE", "{scratch_root()}/discovery/r14p/snap")
    out: env_or("CENSUS_OUT", "{scratch_root()}/discovery/r14p/census")
    compiler: env_or("CENSUS_JET_DIR", readlink_f(seed_dir()))
    id: env_or("CENSUS_JET_IDENTITY", chomp(files.read("{compiler}/IDENTITY") ?? {
        term.eprint("cat: {compiler}/IDENTITY: {err}")
        ""
    }))
    files.remove_all(out) ?? ()
    files.create_dir_all(out) ?? ()
    if prepared == "" -> snapshot(worktree, tree, named)
    files.chdir(tree) ?? quit("census: cd {tree}: {err}", 2)
    check_vars: [String: String]{"JET_COMPILER_IDENTITY": id.copy(), "JET_TOOLCHAIN_ROOT": env_or("CENSUS_TOOLCHAIN_ROOT", tree), "JET_KEEP_GOING": "1", "JET_KEEP_GOING_REPORT": "{out}/check-keep-going.tsv"}
    check_rc: pass(jetenv(g, tree, "{compiler}/jet", [String]{"check", "--threads", "12", "Compiler/JetCli"}, check_vars, STACK_LIMIT), "{out}/check.out", Val("{out}/check.err"))
    say("check rc={check_rc}")
    // The keep-going report is the census: a dependency package's errors reach the stderr only as E0626.
    census_errors(g, "{out}/check-keep-going.tsv", tree, 21)
    if check_rc != 0 -> process.exit(1)
    if env_or("CENSUS_BUILD", "1") == "0" -> process.exit(0)
    files.remove_all("Compiler/JetCli/.jet/build") ?? ()
    build_vars: [String: String]{"JET_COMPILER_IDENTITY": id.copy(), "JET_TOOLCHAIN_ROOT": env_or("CENSUS_BUILD_ROOT", tree), "JET_KEEP_GOING": "1", "JET_KEEP_GOING_REPORT": "{out}/build-keep-going.tsv"}
    build_rc: pass(jetenv(g, tree, "{compiler}/jet", [String]{"build", "--threads", "12", "Compiler"}, build_vars, STACK_LIMIT).timeout(time.duration_seconds(1800)), "{out}/build.out", Val("{out}/build.err"))
    rows: row_count("{out}/build-keep-going.tsv")
    say("build rc={build_rc} rows={rows}")
    emit_build_rows("{out}/build-keep-going.tsv")
    if build_rc != 0 || rows != 0 {
        report: files.read_bytes("{out}/build.err") ?? [U8]{}
        emit(report.slice(0, if report.len() < 600 -> report.len() else -> 600))
        process.exit(1)
    }
    // Guards pass: tests/guards on the seed, failing only on a guard row or closed-list problem not in
    // guards-known.txt.
    if env_or("CENSUS_GUARDS", "1") == "1" {
        limit: env_or("CENSUS_GUARDS_TIMEOUT", "2400")
        guards_vars: [String: String]{"JET_COMPILER_IDENTITY": id.copy(), "JET_TOOLCHAIN_ROOT": tree.copy()}
        guards: jetenv(g, tree, "{compiler}/jet", [String]{"test", "tests/guards"}, guards_vars, [String]{})
        // `timeout 0` runs the command with no limit.
        if duration_seconds(limit) == {
            .Val(seconds) -> _ := pass(if seconds > 0 -> guards.timeout(time.duration_seconds(seconds)) else -> guards, "{out}/guards.out", None)
            .None -> {
                term.eprint("timeout: invalid time interval '{limit}'")
                files.write("{out}/guards.out", "") ?? ()
            }
        }
        gate: process.cmd([String]{"node", "{g}/guards-gate.mjs", "{out}/guards.out", "{g}/guards-known.txt"}).stdin(.Inherit).stdout(.Inherit).stderr(.Inherit).run() ?? {
            say("guards rc=127")
            quit("census: node: {err}", 1)
        }
        say("guards rc={status(gate)}")
        if status(gate) != 0 -> process.exit(1)
    }
    // The compiler `jet build Compiler` just built (JetCli's executable target `jet`).
    self_jet: "{tree}/Compiler/JetCli/.jet/build/jet"
    runtime(g, tree, self_jet, out, id)
    // Self-check pass: the compiler just built checks the compiler packages, then every Core body, with
    // keep-going, as chain s2 does.
    if env_or("CENSUS_SELFCHECK", "1") == "1" {
        packages: [String]{"Compiler/JetCli", "Core"}
        loop package in packages {
            name: if package == "Core" -> "selfcheck-core" else -> "selfcheck"
            vars: [String: String]{"JET_COMPILER_IDENTITY": id.copy(), "JET_TOOLCHAIN_ROOT": tree.copy(), "JET_KEEP_GOING": "1", "JET_KEEP_GOING_REPORT": "{out}/{name}-keep-going.tsv"}
            rc: pass(jetenv(g, tree, self_jet, [String]{"check", "--threads", "12", package.copy()}, vars, STACK_LIMIT).timeout(time.duration_seconds(1200)), "{out}/{name}.out", Val("{out}/{name}.err"))
            count: row_count("{out}/{name}-keep-going.tsv")
            say("{name} rc={rc} rows={count}")
            census_errors(g, "{out}/{name}-keep-going.tsv", tree, 11)
            if rc != 0 || count != 0 -> process.exit(1)
        }
    }
    // Hosted Jetpack pass, when a named file is under Jetpack/: the compiler census alone never compiles Jetpack.
    if named.any(path -> path.starts_with("Jetpack/")) && env_or("CENSUS_JETPACK", "1") == "1" -> jetpack(g, tree, self_jet, out, id)
    process.exit(0)
}
```

### integ-apply6

Main's integration applier integ-apply6.sh and its Jet twin IntegApply6.jet (#5072 c15): commit the export tree as the base with the main repository's objects at hand, then apply each SUB's patch with a three-way merge, excluding generated files and edits of files an earlier SUB deleted, stopping SUBs stacked on a failed one, resolving the mechanical conflicts (keep ours in a generated region, union of a row table or of pure additions, a line-by-line merge) and leaving every other SUB unapplied; then Main's plain fixups and one run of each generator the applied SUBs named. The fixtures hold the current dev's export (tree), the old dev the SUBs were made on (repo.git), the SUBs (subs), a stand-in jet (seed/jet) whose RuntimeGenerators run exits with a checksum of the tree and the integ files, and a stand-in for Main's linemerge.mjs (scratch/Main).

- Original: `Tools/agent/orchestrator/integ-apply6.sh` (shell (bash), 134 lines)
- Jet twin: `Tools/agent/orchestrator/IntegApply6.jet` (562 lines)
- Fixture cases: 4 (args per case: `tree . subs/A-1/ subs/A-2 subs/B-1 subs/C-1 subs/D-1 subs/E-1 subs/G-1 subs/F-1 subs/H-1 subs/H-2 subs/K-1 subs/L-1 subs/M-1 subs/N-1 subs/P-1 subs/R-1 subs/S-1`; `tree .`; `missing . subs/A-1` exit 2; `(none)` exit 1)

| Bar | Result | Measurement |
|---|---|---|
| parity | PASS | 4/4 cases identical |
| speed | FAIL | wall Jet/original 0.71 (397.4 ms against 556.8 ms; every Jet round below every original round); peak memory Jet/original 1.00 (47.2 MB against 47.3 MB; inconclusive: the rounds overlap); medians of 5 interleaved rounds |
| tokens | FAIL | Jet 4250, original 1149 (+3101, Jet/original 3.70) |
| reasoning | FAIL | no review record `Tools/gauntlet/entries/integ-apply6/review.json`; branches Jet 148, original 58 (+90); control nesting Jet 4, original 4 (0) |
| verdict | FAIL | loses speed, tokens, reasoning; the original keeps running |

#### Original: Tools/agent/orchestrator/integ-apply6.sh

```sh
#!/usr/bin/env bash
# integ-apply.sh TREE I SUB... : applies each SUB's patch to the export TREE in order, recording integ files in I.
# Generated files (a "This file is generated by Tools/..." header in the tree's copy) are excluded from every patch
# and regenerated once at the end by the generators they name, so two SUBs that both touch a generated table never
# conflict. Every other hunk applies with a three-way merge against the base blobs in the main repository, so a
# SUB built on an older dev still applies when its context moved but its lines did not. A SUB that still conflicts
# is NOAPPLY and leaves the tree as it was. Each applied SUB's effective diff (generated files excluded) is saved as
# I/SUB.rebased for integ-land; the regeneration diff is I/regen.patch. Prints APPLIED/NOAPPLY lines and, last,
# the applied SUB paths one per line after a line "--applied--".
# The original of gauntlet tool entry integ-apply6; IntegApply6.jet beside it is its Jet twin. The base blobs are
# the objects of the repository $JET_DEV_WORKTREE belongs to, the generators run with the seed $JET_SEED, and
# linemerge.mjs is Main's, in $JET_STATE_ROOT/scratch/Main (Tools/agent/roots.sh).
set -u
. "$(dirname "$0")/../roots.sh"
T=${1:?tree}; I=${2:?integ dir}; shift 2
G=$JET_STATE_ROOT/scratch/Main
# gc.auto=0, maintenance.auto=false: a detached auto gc started by one SUB's commit raced the next commit and broke
# HEAD in post42h (every later .rebased and regen.patch came out empty while APPLIED still printed).
gitq() { git -C "$T" -c user.name=integ -c user.email=integ@local -c gc.auto=0 -c maintenance.auto=false "$@"; }
# -f: the export holds exactly the tracked files, some of which match .gitignore (Tools/script-census.tsv).
gitq init -q && OBJ=$(git -C "$JET_DEV_WORKTREE" rev-parse --path-format=absolute --git-common-dir)/objects && echo "$OBJ" > "$T/.git/objects/info/alternates" && gitq add -A -f && gitq commit -qm base --no-verify && gitq tag base || { echo "integ-apply: init failed"; exit 2; }
: > "$I/generators"
applied=()
for s in "$@"; do
  s=${s%/}; n=$(basename "$s")
  git apply --numstat "$s/patch" 2>/dev/null | awk -F'\t' '{print $3}' > "$I/$n.files"
  ex=()
  while IFS= read -r f; do
    # An edit of a file an earlier SUB in this batch deleted is dropped (the deleting SUB replaced the file's role,
    # e.g. ScriptLead-1 computes the script census that Tools/script-census.tsv held by hand).
    if [ ! -e "$T/$f" ] && gitq cat-file -e "base:$f" 2>/dev/null; then
      by=$(gitq log --format=%s --diff-filter=D -1 -- "$f")
      [ -n "$by" ] && { ex+=("--exclude=$f"); echo "dropped edit of $f: deleted by $by" >> "$I/$n.dropped"; }
      continue
    fi
    [ -f "$T/$f" ] || continue
    # Agent policy and skill prose may quote the generated-file header; it is never generated.
    case "$f" in AGENTS.md|.agents/*) continue;; esac
    g=$(awk 'NR<=40 && /[Gg]enerated by Tools\/[^ ;]+.*do not edit/ {match($0, /[Gg]enerated by Tools\/[^ ;]+/); print substr($0, RSTART+13, RLENGTH-13); exit}' "$T/$f")
    # The safety ledger is written whole by UnsafeRatchet --update (its regions move with every source line).
    [ "$f" = Docs/spec/safety.md ] && g=Tools/agent/UnsafeRatchet.jet
    [ -n "$g" ] || continue
    ex+=("--exclude=$f"); printf '%s\t%s\n' "${g%.}" "$f" >> "$I/generators"
  done < "$I/$n.files"
  dep=""
  for d in $(awk -F': *' '$1=="stack"{print $2}' "$s/meta" 2>/dev/null | tr ',' ' '); do
    [ "$d" = none ] && continue
    case "$d" in *-*) ;; *) d="${n%-*}-$d";; esac
    grep -qx "$d" "$I/failed" 2>/dev/null && dep=$d
  done
  # With no meta file, a SUB stacks on the previous SUB of the same lead in this batch.
  [ -f "$s/meta" ] || { prev=$(awk -v l="${n%-*}" -F/ '{b=$NF; sub(/-[^-]*$/,"",b)} b==l{p=$NF} END{print p}' "$I/order" 2>/dev/null); [ -n "$prev" ] && grep -qx "$prev" "$I/failed" 2>/dev/null && dep=$prev; }
  echo "$s" >> "$I/order"
  if [ -n "$dep" ]; then echo "stack dependency $dep did not apply" > "$I/$n.apply"; echo "NOAPPLY $n"; printf 'NOAPPLY\n' > "$I/$n.status"; echo "$n" >> "$I/failed"; continue; fi
  gitq apply --3way -p1 "${ex[@]}" "$s/patch" > "$I/$n.apply" 2>&1; arc=$?
  # Conflicts left by the three-way merge are resolved only where the answer is mechanical:
  #  - every conflict lies inside a generated region (`unsafe-ratchet:begin..end`, `BEGIN GENERATED..END GENERATED`):
  #    keep ours there and rerun that region's generator;
  #  - a .tsv row table: keep both sides' rows (union);
  #  - every conflict hunk is a pure addition on both sides (an empty base part): keep both, ours first (union);
  #    the census then checks the composed file.
  # Any other conflict makes the SUB NOAPPLY.
  ok=1
  [ $arc = 0 ] || [ -n "$(gitq diff --name-only --diff-filter=U)" ] || ok=0
  for f in $(gitq diff --name-only --diff-filter=U); do
    case "$f" in
      *.tsv) mode=union ;;
      *) mode=$(awk '
          /unsafe-ratchet:begin|BEGIN GENERATED/ {gen=1}
          /^(<<<<<<<|=======|>>>>>>>)( |$)/ {if (!gen) bad=1; seen=1}
          /unsafe-ratchet:end|END GENERATED/ {gen=0}
          END {print (seen && !bad) ? "ours" : "none"}' "$T/$f") ;;
    esac
    if [ "$mode" = none ]; then
      b=$(mktemp); o=$(mktemp); t=$(mktemp)
      gitq show ":1:$f" > "$b" 2>/dev/null; gitq show ":2:$f" > "$o"; gitq show ":3:$f" > "$t"
      mode=$(git merge-file --diff3 -p "$o" "$b" "$t" | awk '
          /^<<<<<<< /{c=1; n++; next} /^\|\|\|\|\|\|\| /{if(c){inb=1}; next} /^=======$/{if(c){inb=0}; next}
          /^>>>>>>> /{c=0; inb=0; next} inb{bad=1} END{print (n && !bad) ? "union" : "none"}')
      # Neighbouring-line edits (and name additions to one `{ a, b }` set line) merge line by line.
      if [ "$mode" = none ] && node "$G/linemerge.mjs" "$o" "$b" "$t" > "$I/linemerge.tmp" 2>/dev/null; then mode=lines; fi
      rm -f "$b" "$o" "$t"
    fi
    [ "$mode" != none ] || { ok=0; echo "unresolvable conflict outside a generated region: $f" >> "$I/$n.apply"; break; }
    if [ "$mode" = lines ]; then
      cp "$I/linemerge.tmp" "$T/$f"
    else
      b=$(mktemp); o=$(mktemp); t=$(mktemp)
      gitq show ":1:$f" > "$b" 2>/dev/null; gitq show ":2:$f" > "$o"; gitq show ":3:$f" > "$t"
      git merge-file "--$mode" -p "$o" "$b" "$t" > "$T/$f"; rm -f "$b" "$o" "$t"
    fi
    gitq add -- "$f"
    echo "resolved $f ($mode)" >> "$I/$n.apply"
    if [ "$mode" = ours ]; then
      if grep -q 'unsafe-ratchet:begin' "$T/$f"; then printf 'Tools/agent/UnsafeRatchet.jet\t%s\n' "$f" >> "$I/generators"
      else printf 'Tools/agent/CoreSourceRows.jet\t%s\n' "$f" >> "$I/generators"; fi
    fi
  done
  if [ $ok = 1 ] && [ -z "$(gitq diff --name-only --diff-filter=U)" ] && gitq add -A && gitq diff --cached --quiet HEAD; then
    # The patch changes nothing on this tree: an earlier SUB of the batch already carries it (post42h2: Main-2).
    echo "already in the tree: every change of this patch is carried by an earlier SUB" >> "$I/$n.apply"
    echo "NOAPPLY $n (empty)"; printf 'NOAPPLY\n' > "$I/$n.status"; echo "$n" >> "$I/failed"
  elif [ $ok = 1 ] && [ -z "$(gitq diff --name-only --diff-filter=U)" ]; then
    gitq commit -qm "$n" --no-verify || { echo "integ-apply: commit of $n failed; the integ repo is broken, stopping"; exit 2; }
    gitq diff --binary HEAD~1 HEAD > "$I/$n.rebased" || { echo "integ-apply: diff of $n failed; stopping"; exit 2; }
    [ -s "$I/$n.rebased" ] || { echo "integ-apply: $n.rebased is empty; stopping"; exit 2; }
    applied+=("$s"); echo "APPLIED $n"
  else
    gitq reset -q --hard HEAD; gitq clean -qfd
    echo "NOAPPLY $n"; printf 'NOAPPLY\n' > "$I/$n.status"; echo "$n" >> "$I/failed"
  fi
done
# INTEG_POST="patch ...": Main's fixups that must apply plainly (no three-way, no keep-ours) to the composed tree after
# every SUB, such as deleting ledger proof rows a merge kept. They land inside regen.patch.
for p in ${INTEG_POST:-}; do
  ( cd "$T" && git apply -p1 "$p" ) && echo "POST $p" || echo "POST-FAIL $p"
done
# Regenerate every generated file an applied SUB touched, with the seed, from the composed sources.
C=$(readlink -f "$JET_SEED")
# The runtime generator outputs (RuntimeStopExports and the rest) go stale whenever a SUB changes a runtime export,
# and no SUB names them: regenerate them in every batch.
printf "Tools/agent/RuntimeGenerators.jet\tall\n" >> "$I/generators"
for gen in $(awk -F'\t' '{print $1}' "$I/generators" | sort -u); do
  case "$gen" in Tools/*.jet) ;; *) echo "REGEN-SKIP $gen (not a Jet generator)"; continue;; esac
  [ -f "$T/$gen" ] || { echo "REGEN-SKIP $gen (missing)"; continue; }
  case "$gen" in */DiagnosticRows.jet) args=--write;; */UnsafeRatchet.jet) args=--update;; *) args="";; esac
  ( cd "$T" && JET_COMPILER_IDENTITY=$(cat "$C/IDENTITY") JET_TOOLCHAIN_ROOT=$T timeout 900 taskset -c 16-31 nice -n 19 "$C/jet" "$gen" $args ) > "$I/regen.$(basename "$gen").out" 2>&1
  echo "REGEN $gen $args rc=$?"
done
gitq add -A && gitq diff --cached --binary HEAD > "$I/regen.patch"
rm -rf "$T/.git"
echo "--applied--"
printf '%s\n' "${applied[@]}"
```

#### Jet twin: Tools/agent/orchestrator/IntegApply6.jet

```jet
#!/usr/bin/env jet
// IntegApply6.jet TREE I SUB...: applies each SUB's patch to the export TREE
// in order, recording integ files in I. Generated files (a "This file is
// generated by Tools/..." header in the tree's copy) are excluded from every
// patch and regenerated once at the end by the generators they name, so two
// SUBs that both touch a generated table never conflict. Every other hunk
// applies with a three-way merge against the base blobs in the main
// repository, so a SUB built on an older dev still applies when its context
// moved but its lines did not. A SUB that still conflicts is NOAPPLY and
// leaves the tree as it was. Each applied SUB's effective diff (generated
// files excluded) is saved as I/SUB.rebased for integ-land; the regeneration
// diff is I/regen.patch. Prints APPLIED/NOAPPLY lines and, last, the applied
// SUB paths one per line after a line "--applied--".
//
// The Jet twin of integ-apply6.sh (gauntlet tool entry integ-apply6): the
// base blobs are the objects of the repository $JET_DEV_WORKTREE belongs to,
// the generators run with the seed $JET_SEED, and linemerge.mjs is Main's, in
// $JET_STATE_ROOT/scratch/Main, as Tools/agent/roots.sh sets them.
use {
    files: core.files
    process: core.process
    regex: core.regex
    sys: core.sys
    term: core.term
    time: core.time
}

// A header line that names its file's generator, and the name in it after
// "generated by ".
GENERATED_HEADER: Regex{"[Gg]enerated by Tools/[^ ;]+.*do not edit"}
GENERATOR_NAME: Regex{"[Gg]enerated by Tools/[^ ;]+"}
// A conflict marker line a three-way merge leaves.
CONFLICT_MARKER: Regex{"^(<<<<<<<|=======|>>>>>>>)( |$)"}

env_or: fn(name: String, fallback: String) -[Env]> String Never! {
    value: process.env_get(name) ?? return fallback.copy()
    if value == "" -> fallback.copy() else -> value
}

// What the machine's jet/env file sets `name` to (the default of its
// `${NAME:-default}` form), None when it sets nothing.
env_file_value: fn(name: String) -[Env, FS]> String? Never! {
    home: env_or("HOME", "")
    text: files.read("{env_or("XDG_CONFIG_HOME", "{home}/.config")}/jet/env") ?? return None
    defaulted: "${{{name}:-"
    found := ""
    seen := false
    loop line in text.lines() {
        parts: line.trim().removeprefix("export ").partition("=")
        if parts.sep == "" || parts.head != name -> next
        value := parts.tail.removeprefix("\"").removesuffix("\"").removeprefix("'").removesuffix("'")
        if value.starts_with(defaulted) && value.ends_with("}}") -> value = value.removeprefix(defaulted).removesuffix("}}")
        found = value.replace("$HOME", home)
        seen = true
    }
    if seen -> Val(found) else -> None
}

// A root as Tools/agent/roots.sh sets it: the environment, else the jet/env
// file, else `fallback`.
root: fn(name: String, fallback: String) -[Env, FS]> String Never! {
    found: env_or(name, "")
    if found != "" -> return found
    env_file_value(name) ?? fallback.copy()
}

state_root: fn() -[Env, FS]> String Never! { root("JET_STATE_ROOT", "{env_or("XDG_CACHE_HOME", "{env_or("HOME", "")}/.cache")}/jet-dev") }

seed_dir: fn() -[Env, FS]> String Never! {
    seeds: root("JET_SEEDS", "{env_or("XDG_DATA_HOME", "{env_or("HOME", "")}/.local/share")}/jet-seeds")
    root("JET_SEED", "{seeds}/current")
}

// `text` without its trailing newlines, as `$(...)` gives it.
chomp: fn(text: String) -> String Never! {
    out := text.copy()
    loop out.ends_with("\n") -> out = out.removesuffix("\n")
    out
}

// The lines awk reads from `text`: a last line without its newline counts.
records: fn(text: String) -> [String] Never! {
    out := text.split("\n").to_list()
    if out.len() > 0 && out[out.len() - 1] == "" -> _ := out.pop()
    out
}

// The words the shell splits `text` into (no globbing).
words: fn(text: String) -> [String] Never! {
    out := [String]{}
    loop word in text.replace("\t", " ").replace("\n", " ").split(" ") {
        if word != "" -> out.push(word.copy())
    }
    out
}

// `${name%-*}`: `name` without its last `-` part.
lead_of: fn(name: String) -> String Never! {
    parts: name.split("-").to_list()
    if parts.len() < 2 -> return name.copy()
    parts[0..<parts.len() - 1].join("-")
}

// `readlink -f path`: the path with every symlink resolved; a last part that
// does not exist stays as it is.
readlink_f: fn(path: String) -[FS, Env]> String Never! {
    whole: files.canonicalize(path) ?? {
        parent: files.canonicalize(if files.dirname(path) == "" -> "." else -> files.dirname(path)) ?? return path.copy()
        return "{parent}/{files.basename(path)}"
    }
    whole
}

// Writes `line` to stdout now, before any child writes to it.
say: fn(line: String) Never! -[IO]> {
    out := term.stdout()
    out.write_bytes("{line}\n".bytes()) ?? ()
    out.flush() ?? ()
}

// `printf ... >> path`.
append: fn(path: String, text: String) Never! -[FS, IO]> {
    writer: files.append(path) ?? {
        term.eprint("integ-apply: {path}: {err}")
        return
    }
    writer.write_all(text.bytes()) ?? term.eprint("integ-apply: {path}: {err}")
    writer.flush() ?? ()
}

// `printf ... > path`.
put: fn(path: String, text: String) Never! -[FS, IO]> {
    files.write(path, text) ?? term.eprint("integ-apply: {path}: {err}")
}

// The exit status the shell gives a finished child: 124 for one `timeout`
// stopped, 128 + N for signal N.
status: fn(receipt: ProcessReceipt) -> Int Never! {
    if receipt.timed_out -> return 124
    if receipt.signal == .Val(number) -> return 128 + number
    receipt.code
}

// Ends the run with `code` after `line` on stdout.
stop: fn(line: String, code: Int) -[Exec, IO]> Never {
    say(line)
    process.exit(code)
}

// Ends the run with `code` after `message` on stderr.
quit: fn(message: String, code: Int) -[Exec, IO]> Never {
    term.eprint(message)
    process.exit(code)
}

// `git -C tree` with the integ identity and no automatic gc: a detached auto
// gc started by one SUB's commit raced the next commit and broke HEAD (every
// later .rebased and regen.patch came out empty while APPLIED still printed).
gitq: fn(tree: String, args: [String]) -[Exec]> ProcessSpec Never! {
    argv := [String]{"git", "-C", tree.copy(), "-c", "user.name=integ", "-c", "user.email=integ@local", "-c", "gc.auto=0", "-c", "maintenance.auto=false"}
    argv.extend(args.copy())
    process.cmd(argv)
}

// Runs `spec` on the program's streams; its exit status.
runs: fn(spec: ProcessSpec) -[Exec, IO, Time.Wait]> Int Never! {
    receipt: spec.stdin(.Inherit).stdout(.Inherit).stderr(.Inherit).run() ?? {
        term.eprint("integ-apply: {err}")
        return 127
    }
    status(receipt)
}

// Runs `spec` with its stderr discarded (`2>/dev/null`); its exit status.
runs_quiet: fn(spec: ProcessSpec) -[Exec, Time.Wait]> Int Never! {
    receipt: spec.stdin(.Inherit).stdout(.Inherit).stderr(.Capture).output_limit(1073741824).run() ?? return 127
    status(receipt)
}

// `$(spec)` with its stderr shown: None when it could not start.
captured: fn(spec: ProcessSpec) -[Exec, IO, Time.Wait]> ProcessReceipt? Never! {
    receipt: spec.stdin(.Inherit).stdout(.Capture).stderr(.Inherit).output_limit(1073741824).run() ?? {
        term.eprint("integ-apply: {err}")
        return None
    }
    Val(receipt)
}

// `$(spec)`, its stdout without trailing newlines.
output: fn(spec: ProcessSpec) -[Exec, IO, Time.Wait]> String Never! {
    receipt: captured(spec) ?? return ""
    chomp(receipt.output)
}

// `spec > path`, its stderr as the caller set it; the shell's exit status. As
// in the shell, a file that can't be opened fails (1) before `spec` runs.
into: fn(spec: ProcessSpec, path: String) -[Exec, FS, IO, Time.Wait]> Int Never! {
    files.write(path, "") ?? {
        term.eprint("integ-apply: {path}: {err}")
        return 1
    }
    receipt: spec.stdin(.Inherit).stdout(.Capture).output_limit(1073741824).run() ?? {
        term.eprint("integ-apply: {err}")
        return 127
    }
    files.write(path, receipt.output) ?? term.eprint("integ-apply: {path}: {err}")
    status(receipt)
}

// `spec > path`, its stderr shown.
into_file: fn(spec: ProcessSpec, path: String) -[Exec, FS, IO, Time.Wait]> Int Never! { into(spec.stderr(.Inherit), path) }

// `spec > path 2>&1`.
into_merged: fn(spec: ProcessSpec, path: String) -[Exec, FS, IO, Time.Wait]> Int Never! { into(spec.stderr(.Stdout), path) }

// `spec > path 2>/dev/null`.
into_quiet: fn(spec: ProcessSpec, path: String) -[Exec, FS, IO, Time.Wait]> Int Never! { into(spec.stderr(.Capture), path) }

// The base commit: the export with every tracked file (-f: some match
// .gitignore, as Tools/script-census.tsv does), the main repository's objects
// at hand for the three-way merges; whether every step succeeded.
based: fn(tree: String, worktree: String) -[Exec, FS, IO, Time.Wait]> Bool Never! {
    init: gitq(tree, [String]{"init", "-q"})
    if runs(init) != 0 -> return false
    common: captured(process.cmd([String]{"git", "-C", worktree.copy(), "rev-parse", "--path-format=absolute", "--git-common-dir"})) ?? return false
    if status(common) != 0 -> return false
    alternates: "{tree}/.git/objects/info/alternates"
    files.write(alternates, "{chomp(common.output)}/objects\n") ?? {
        term.eprint("integ-apply: {alternates}: {err}")
        return false
    }
    add: gitq(tree, [String]{"add", "-A", "-f"})
    commit: gitq(tree, [String]{"commit", "-qm", "base", "--no-verify"})
    tagged: gitq(tree, [String]{"tag", "base"})
    runs(add) == 0 && runs(commit) == 0 && runs(tagged) == 0
}

// The paths `git apply --numstat` lists for `patch` (its third column).
patch_paths: fn(patch: String) -[Exec, Time.Wait]> [String] Never! {
    receipt: process.cmd([String]{"git", "apply", "--numstat", patch.copy()}).stdin(.Inherit).stdout(.Capture).stderr(.Capture).output_limit(1073741824).run() ?? return [String]{}
    out := [String]{}
    loop line in records(receipt.output) {
        fields: line.split("\t").to_list()
        out.push(if fields.len() > 2 -> fields[2].copy() else -> "")
    }
    out
}

// The generator a file's header names in its first 40 lines, one trailing
// `.` dropped; empty when none does.
generator_of: fn(path: String) -[FS]> String Never! {
    text: files.read(path) ?? return ""
    lines: records(text)
    loop line in lines[0..<(if lines.len() < 40 -> lines.len() else -> 40)] {
        if !regex.is_match(GENERATED_HEADER, line) -> next
        found: regex.find(GENERATOR_NAME, line) ?? return ""
        return found.removeprefix("generated by ").removeprefix("Generated by ").removesuffix(".")
    }
    ""
}

// The SUBs of a meta file's stack lines (`stack: a, b`); a bare number is a
// SUB of the same lead.
stacked_on: fn(meta: String, lead: String) -[FS]> [String] Never! {
    text: files.read(meta) ?? return [String]{}
    out := [String]{}
    loop line in records(text) {
        parts: line.partition(":")
        if parts.head != "stack" -> next
        value := parts.tail.copy()
        loop value.starts_with(" ") -> value = value.removeprefix(" ")
        loop word in words(value.partition(":").head.replace(",", " ")) {
            if word == "none" -> next
            out.push(if word.contains("-") -> word.copy() else -> "{lead}-{word}")
        }
    }
    out
}

// The last SUB of `lead` in the batch so far.
previous_of: fn(order: String, lead: String) -[FS]> String Never! {
    text: files.read(order) ?? return ""
    found := ""
    loop line in records(text) {
        parts: line.split("/").to_list()
        last: parts[parts.len() - 1].copy()
        if lead_of(last) == lead -> found = last.copy()
    }
    found
}

// The unmerged paths of the tree's index.
unmerged: fn(tree: String) -[Exec, IO, Time.Wait]> [String] Never! {
    diff: gitq(tree, [String]{"diff", "--name-only", "--diff-filter=U"})
    words(output(diff))
}

// A file whose every conflict lies inside a generated region keeps ours there
// ("ours"); any other conflict is "none"; "" when the file can't be read.
marker_mode: fn(path: String) -[FS]> String Never! {
    text: files.read(path) ?? return ""
    generated := false
    bad := false
    seen := false
    loop line in records(text) {
        if line.contains("unsafe-ratchet:begin") || line.contains("BEGIN GENERATED") -> generated = true
        if regex.is_match(CONFLICT_MARKER, line) {
            if !generated -> bad = true
            seen = true
        }
        if line.contains("unsafe-ratchet:end") || line.contains("END GENERATED") -> generated = false
    }
    if seen && !bad -> "ours" else -> "none"
}

// The base, ours and theirs of an unmerged `path` in `work` as `b`, `o`, `t`.
stages: fn(tree: String, path: String, work: String) Never! -[Exec, FS, IO, Time.Wait]> {
    _ := into_quiet(gitq(tree, [String]{"show", ":1:{path}"}), "{work}/b")
    _ := into_file(gitq(tree, [String]{"show", ":2:{path}"}), "{work}/o")
    _ := into_file(gitq(tree, [String]{"show", ":3:{path}"}), "{work}/t")
}

// A scratch directory for one file's stages.
scratch: fn() -[FS, IO]> String Never! {
    files.mkdtemp("integ-apply") ?? {
        term.eprint("mktemp: {err}")
        "."
    }
}

// The mode a conflict the markers leave unresolved merges by: "union" when
// every conflict hunk is a pure addition on both sides (an empty base part);
// else "lines" when linemerge.mjs merges neighbouring-line edits line by line,
// its result in I/linemerge.tmp; else "none".
staged_mode: fn(tree: String, integ: String, g: String, path: String) -[Exec, FS, IO, Time.Wait]> String Never! {
    work: scratch()
    stages(tree, path, work)
    merged: output(process.cmd([String]{"git", "merge-file", "--diff3", "-p", "{work}/o", "{work}/b", "{work}/t"}))
    open := false
    hunks := 0
    in_base := false
    bad := false
    loop line in records(merged) {
        if {
            line.starts_with("<<<<<<< ") -> {
                open = true
                hunks += 1
            }
            line.starts_with("||||||| ") -> { if open -> in_base = true }
            line == "=======" -> { if open -> in_base = false }
            line.starts_with(">>>>>>> ") -> {
                open = false
                in_base = false
            }
            in_base -> bad = true
        }
    }
    mode := if hunks > 0 && !bad -> "union" else -> "none"
    linemerge: process.cmd([String]{"node", "{g}/linemerge.mjs", "{work}/o", "{work}/b", "{work}/t"})
    if mode == "none" && into_quiet(linemerge, "{integ}/linemerge.tmp") == 0 -> mode = "lines"
    files.remove_all(work) ?? ()
    mode
}

// Writes the `mode` merge of an unmerged `path` over the tree's copy.
merge_into: fn(tree: String, path: String, mode: String) Never! -[Exec, FS, IO, Time.Wait]> {
    work: scratch()
    stages(tree, path, work)
    _ := into_file(process.cmd([String]{"git", "merge-file", "--{mode}", "-p", "{work}/o", "{work}/b", "{work}/t"}), "{tree}/{path}")
    files.remove_all(work) ?? ()
}

// Records SUB `name` as not applied.
failed: fn(integ: String, name: String, note: String) Never! -[FS, IO]> {
    say("NOAPPLY {name}{note}")
    put("{integ}/{name}.status", "NOAPPLY\n")
    append("{integ}/failed", "{name}\n")
}

// Applies SUB `sub`; its path when it applied.
apply_sub: fn(tree: String, integ: String, g: String, sub: String) -[Exec, FS, IO, Time.Wait]> String? Never! {
    s: sub.removesuffix("/")
    n: files.basename(s)
    listed: patch_paths("{s}/patch")
    put("{integ}/{n}.files", if listed.len() == 0 -> "" else -> "{listed.join("\n")}\n")
    excluded := [String]{}
    loop f in listed {
        if f == "" -> next
        path: "{tree}/{f}"
        // An edit of a file an earlier SUB in this batch deleted is dropped (the deleting SUB replaced the file's
        // role, e.g. ScriptLead-1 computes the script census that Tools/script-census.tsv held by hand).
        if !files.exists(path) {
            probe: gitq(tree, [String]{"cat-file", "-e", "base:{f}"})
            if runs_quiet(probe) == 0 {
                by: output(gitq(tree, [String]{"log", "--format=%s", "--diff-filter=D", "-1", "--", f.copy()}))
                if by != "" {
                    excluded.push("--exclude={f}")
                    append("{integ}/{n}.dropped", "dropped edit of {f}: deleted by {by}\n")
                }
                next
            }
        }
        if !files.is_file(path) -> next
        // Agent policy and skill prose may quote the generated-file header; it is never generated.
        if f == "AGENTS.md" || f.starts_with(".agents/") -> next
        // The safety ledger is written whole by UnsafeRatchet --update (its regions move with every source line).
        generator: if f == "Docs/spec/safety.md" -> "Tools/agent/UnsafeRatchet.jet" else -> generator_of(path)
        if generator == "" -> next
        excluded.push("--exclude={f}")
        append("{integ}/generators", "{generator}\t{f}\n")
    }
    lead: lead_of(n)
    failed_text: files.read("{integ}/failed") ?? ""
    failures: records(failed_text)
    dependency := ""
    loop d in stacked_on("{s}/meta", lead) {
        if failures.contains(d) -> dependency = d.copy()
    }
    // With no meta file, a SUB stacks on the previous SUB of the same lead in this batch.
    if !files.is_file("{s}/meta") {
        previous: previous_of("{integ}/order", lead)
        if previous != "" && failures.contains(previous) -> dependency = previous.copy()
    }
    append("{integ}/order", "{s}\n")
    apply_log: "{integ}/{n}.apply"
    if dependency != "" {
        put(apply_log, "stack dependency {dependency} did not apply\n")
        failed(integ, n, "")
        return None
    }
    apply_args := [String]{"apply", "--3way", "-p1"}
    apply_args.extend(excluded)
    apply_args.push("{s}/patch")
    applied: into_merged(gitq(tree, apply_args), apply_log)
    // Conflicts left by the three-way merge are resolved only where the answer is mechanical:
    //  - every conflict lies inside a generated region (`unsafe-ratchet:begin..end`, `BEGIN GENERATED..END
    //    GENERATED`): keep ours there and rerun that region's generator;
    //  - a .tsv row table: keep both sides' rows (union);
    //  - every conflict hunk is a pure addition on both sides (an empty base part): keep both, ours first
    //    (union); the census then checks the composed file.
    // Any other conflict makes the SUB NOAPPLY.
    ok := applied == 0 || unmerged(tree).len() > 0
    loop f in unmerged(tree) {
        mode := if f.ends_with(".tsv") -> "union" else -> marker_mode("{tree}/{f}")
        if mode == "none" -> mode = staged_mode(tree, integ, g, f)
        if mode == "none" {
            ok = false
            append(apply_log, "unresolvable conflict outside a generated region: {f}\n")
            break
        }
        if {
            mode == "lines" -> files.copy("{integ}/linemerge.tmp", "{tree}/{f}") ?? term.eprint("cp: {integ}/linemerge.tmp: {err}")
            else -> merge_into(tree, f, mode)
        }
        _ := runs(gitq(tree, [String]{"add", "--", f.copy()}))
        append(apply_log, "resolved {f} ({mode})\n")
        if mode == "ours" {
            text: files.read("{tree}/{f}") ?? ""
            regenerated: if text.contains("unsafe-ratchet:begin") -> "Tools/agent/UnsafeRatchet.jet" else -> "Tools/agent/CoreSourceRows.jet"
            append("{integ}/generators", "{regenerated}\t{f}\n")
        }
    }
    clean: ok && unmerged(tree).len() == 0
    add: gitq(tree, [String]{"add", "-A"})
    unchanged: gitq(tree, [String]{"diff", "--cached", "--quiet", "HEAD"})
    if {
        clean && runs(add) == 0 && runs(unchanged) == 0 -> {
            // The patch changes nothing on this tree: an earlier SUB of the batch already carries it.
            append(apply_log, "already in the tree: every change of this patch is carried by an earlier SUB\n")
            failed(integ, n, " (empty)")
            None
        }
        clean -> {
            commit: gitq(tree, [String]{"commit", "-qm", n.copy(), "--no-verify"})
            if runs(commit) != 0 -> stop("integ-apply: commit of {n} failed; the integ repo is broken, stopping", 2)
            rebased: "{integ}/{n}.rebased"
            diff: gitq(tree, [String]{"diff", "--binary", "HEAD~1", "HEAD"})
            if into_file(diff, rebased) != 0 -> stop("integ-apply: diff of {n} failed; stopping", 2)
            written: files.read_bytes(rebased) ?? [U8]{}
            if written.len() == 0 -> stop("integ-apply: {n}.rebased is empty; stopping", 2)
            say("APPLIED {n}")
            Val(s)
        }
        else -> {
            _ := runs(gitq(tree, [String]{"reset", "-q", "--hard", "HEAD"}))
            _ := runs(gitq(tree, [String]{"clean", "-qfd"}))
            failed(integ, n, "")
            None
        }
    }
}

// Regenerates every generated file an applied SUB touched, with the seed,
// from the composed sources.
regenerate: fn(tree: String, integ: String) Never! -[Env, Exec, FS, IO, Time.Wait]> {
    seed: readlink_f(seed_dir())
    // The runtime generator outputs (RuntimeStopExports and the rest) go stale whenever a SUB changes a runtime
    // export, and no SUB names them: regenerate them in every batch.
    append("{integ}/generators", "Tools/agent/RuntimeGenerators.jet\tall\n")
    text: files.read("{integ}/generators") ?? ""
    names := [String]{}
    loop line in records(text) {
        loop name in words(line.partition("\t").head) -> names.push(name)
    }
    names.sort()
    identity: chomp(files.read("{seed}/IDENTITY") ?? "")
    at := 0
    loop at < names.len() {
        gen: names[at].copy()
        at += 1
        if at > 1 && names[at - 2] == gen -> next
        if !gen.starts_with("Tools/") || !gen.ends_with(".jet") {
            say("REGEN-SKIP {gen} (not a Jet generator)")
            next
        }
        if !files.is_file("{tree}/{gen}") {
            say("REGEN-SKIP {gen} (missing)")
            next
        }
        flag: if {
            gen.ends_with("/DiagnosticRows.jet") -> "--write"
            gen.ends_with("/UnsafeRatchet.jet") -> "--update"
            else -> ""
        }
        argv := [String]{"nice", "-n", "19", "{seed}/jet", gen.copy()}
        if flag != "" -> argv.push(flag.copy())
        spec: process.cmd(argv).cwd(tree).env("JET_COMPILER_IDENTITY", identity).env("JET_TOOLCHAIN_ROOT", tree)
            .timeout(time.duration_seconds(900)).cpu_affinity(loop cpu in 16..<32 -> cpu)
        rc: into_merged(spec, "{integ}/regen.{files.basename(gen)}.out")
        say("REGEN {gen} {flag} rc={rc}")
    }
}

run: fn() {
    args: process.args()
    if args.len() < 1 || args[0] == "" -> quit("integ-apply: 1: tree", 1)
    if args.len() < 2 || args[1] == "" -> quit("integ-apply: 2: integ dir", 1)
    tree: args[0].copy()
    integ: args[1].copy()
    g: "{state_root()}/scratch/Main"
    worktree: root("JET_DEV_WORKTREE", files.dirname(files.dirname(files.dirname(sys.executable()))))
    if !based(tree, worktree) -> stop("integ-apply: init failed", 2)
    put("{integ}/generators", "")
    applied := [String]{}
    loop sub in args[2..<args.len()] {
        if apply_sub(tree, integ, g, sub) == .Val(path) -> applied.push(path)
    }
    // INTEG_POST="patch ...": Main's fixups that must apply plainly (no three-way, no keep-ours) to the composed
    // tree after every SUB, such as deleting ledger proof rows a merge kept. They land inside regen.patch.
    loop patch in words(env_or("INTEG_POST", "")) {
        post: process.cmd([String]{"git", "apply", "-p1", patch.copy()}).cwd(tree)
        say(if runs(post) == 0 -> "POST {patch}" else -> "POST-FAIL {patch}")
    }
    regenerate(tree, integ)
    add: gitq(tree, [String]{"add", "-A"})
    if runs(add) == 0 -> _ := into_file(gitq(tree, [String]{"diff", "--cached", "--binary", "HEAD"}), "{integ}/regen.patch")
    files.remove_all("{tree}/.git") ?? ()
    say("--applied--")
    if applied.len() == 0 -> say("")
    loop path in applied -> say(path)
}
```

### integ-land4

The Lander's integ-land4.sh and its Jet twin IntegLand4.jet (#5072 c15): land a passed integration batch's SUBs on dev, one commit each, after checking the census, each SUB's status, origin/dev against BASE and the landed tree against the tested one, then push. The fixtures are the dev worktree itself (its git directory is work.git, its origin the bare origin.git) with four batches: one that lands, one whose census failed, one whose tested tree differs, and one whose SUB did not apply.

- Original: `Tools/agent/orchestrator/integ-land4.sh` (shell (bash), 61 lines)
- Jet twin: `Tools/agent/orchestrator/IntegLand4.jet` (288 lines)
- Fixture cases: 4 (args per case: `b1 inbox/L-1/`; `b2 inbox/L-1` exit 1; `b3 inbox/L-1` exit 1; `b4 inbox/L-2` exit 1)

| Bar | Result | Measurement |
|---|---|---|
| parity | PASS | 4/4 cases identical |
| speed | FAIL | wall Jet/original 0.61 (147.3 ms against 242.4 ms; inconclusive: the rounds overlap); peak memory Jet/original 0.29 (14.0 MB against 47.9 MB; every Jet round below every original round); medians of 5 interleaved rounds |
| tokens | FAIL | Jet 2453, original 670 (+1783, Jet/original 3.66) |
| reasoning | FAIL | no review record `Tools/gauntlet/entries/integ-land4/review.json`; branches Jet 98, original 39 (+59); control nesting Jet 3, original 2 (+1) |
| verdict | FAIL | loses speed, tokens, reasoning; the original keeps running |

#### Original: Tools/agent/orchestrator/integ-land4.sh

```sh
#!/usr/bin/env bash
# integ-land.sh BATCH SUB... : lands the named submissions of a passed integration batch on dev, one commit each, in
# the order given (the order integrate.sh applied them). Refuses unless integ/BATCH's census and ratchets passed, each
# SUB's status is OK, and origin/dev is still the batch's BASE (any other movement means the composed tree was not
# the one tested: rerun integrate.sh). Commits in the dev worktree stage exactly each patch's files; the author's
# lane name goes in a Lane: trailer. Pushes once, then appends the shas to batch-queue.txt.
# The original of gauntlet tool entry integ-land4; IntegLand4.jet beside it is its Jet twin. The batches live in
# $JET_SCRATCH_ROOT/lanes/Main/integ, the dev worktree is $JET_DEV_WORKTREE, and treeonly.mjs, batch-queue.txt and
# landed-subs.txt are Main's, in $JET_STATE_ROOT/scratch/Main (Tools/agent/roots.sh).
set -u
BATCH=${1:?batch}; shift
. "$(dirname "$0")/../roots.sh"
M=$JET_SCRATCH_ROOT/lanes/Main
W=$JET_DEV_WORKTREE
G=$JET_STATE_ROOT/scratch/Main
I=$M/integ/$BATCH
grep -q '^Census rc=0 ' "$I/REPORT.md" || { echo "integ-land: batch $BATCH did not pass its census/ratchets"; exit 1; }
cd "$W" || exit 2
[ -z "$(git status --porcelain=v1 -uno)" ] || { echo "integ-land: dev worktree has changes"; exit 1; }
git fetch -q origin
base=$(cat "$I/BASE")
# origin/dev may have moved past BASE only by changes that are not build inputs and share no file with this batch:
# the net diff BASE..origin/dev may touch no Compiler/, Core/, Jetpack/ or Tools/ file and none of the batch files.
moved=$(git diff --name-only "$base" origin/dev)
bad=$(printf "%s\n" "$moved" | while read -r f; do [ -n "$f" ] || continue; case "$f" in Compiler/*|Core/*|Jetpack/*|Tools/*) echo "$f";; *) cat "$I"/*.files | grep -qx "$f" && echo "$f";; esac; done)
[ -z "$bad" ] || { echo "integ-land: origin/dev moved from $base in build inputs or batch files: $(echo $bad | cut -c1-300); rerun integrate.sh"; exit 1; }
git merge -q --ff-only origin/dev || { echo "integ-land: cannot fast-forward the worktree to origin/dev"; exit 1; }
shas=()
for s in "$@"; do
  s=${s%/}; n=$(basename "$s")
  [ "$(cat "$I/$n.status" 2>/dev/null)" = OK ] || { echo "integ-land: $n is not OK in $BATCH; stopping before it"; break; }
  P="$s/patch"; [ -s "$I/$n.rebased" ] && P="$I/$n.rebased"
  # An empty .rebased means the integ repo lost the commit (b6d: gc race); the raw patch is not what was tested.
  [ -e "$I/$n.rebased" ] && [ ! -s "$I/$n.rebased" ] && { echo "integ-land: $I/$n.rebased is empty; rebuild it (rebuild-rebased.sh) before landing"; exit 1; }
  git apply -p1 --index "$P" || { echo "integ-land: $n does not apply on $(git rev-parse --short HEAD)"; exit 1; }
  # Every path the patch staged, rename sources included (numstat names only the rename target: b6d left 12 behind).
  git diff --cached --name-only --no-renames > "$I/$n.landfiles"
  lane=${n%-*}
  { cat "$s/msg"; printf '\nLane: %s\nIntegrated: %s on %s\n' "$lane" "$BATCH" "$base"; } > "$I/$n.commitmsg"
  git commit -q -F "$I/$n.commitmsg" -- $(cat "$I/$n.landfiles") || { echo "integ-land: commit of $n failed"; exit 1; }
  shas+=("$(git rev-parse --short HEAD) $n")
done
[ ${#shas[@]} -gt 0 ] || exit 1
# The generated tables integ-apply regenerated from the composed sources land last, only when EVERY SUB the batch
# applied landed (b4b: landing a subset with the full regen put SchedLead-6 rows on dev without their source).
napplied=$(ls "$I"/*.files 2>/dev/null | awk "!/\/regen\.files$/" | wc -l)
if [ -s "$I/regen.patch" ]; then
  # INTEG_LANDED_BEFORE=k: k SUBs of this batch already landed in an earlier, interrupted run.
  if [ $(( ${#shas[@]} + ${INTEG_LANDED_BEFORE:-0} )) -eq "$napplied" ]; then
    git apply -p1 --index "$I/regen.patch" && git diff --cached --name-only --no-renames > "$I/regen.files" && printf "integration: regenerate the generated tables for %s\n\nIntegrated: %s on %s\n" "$BATCH" "$BATCH" "$base" > "$I/regen.commitmsg" && git commit -q -F "$I/regen.commitmsg" -- $(cat "$I/regen.files") && shas+=("$(git rev-parse --short HEAD) regen") || { echo "integ-land: regen commit failed"; exit 1; }
  else echo "integ-land: not every SUB landed, so regen.patch was not applied; rerun the generators"; exit 1; fi
fi
# The landed tree must be the tested composed tree, byte for byte, before anything is pushed.
[ -z "$(git status --porcelain=v1 -uno)" ] || { echo "integ-land: staged leftovers after landing: $(git status --porcelain=v1 -uno | head -n5 | tr '\n' ' ')"; exit 1; }
if [ ${#shas[@]} -gt 0 ] && [ "$(( ${#shas[@]} + ${INTEG_LANDED_BEFORE:-0} ))" -ge "$napplied" ]; then
  drift=$(git -c core.quotepath=off ls-files | while read -r f; do [ -L "$f" ] && continue; [ -e "$I/tree/$f" ] || { [ -e "$f" ] && echo "NOTINTREE $f"; continue; }; cmp -s "$f" "$I/tree/$f" || echo "DIFF $f"; done; node "$G/treeonly.mjs" "$I/tree" . | awk '!/ only-in-tree$/ && NF {print "ONLYINTREE " $0}')
  [ -z "$drift" ] || { echo "integ-land: landed tree differs from $I/tree: $(echo "$drift" | head -n8 | tr '\n' ' ')"; exit 1; }
fi
git fetch -q origin && git merge-base --is-ancestor origin/dev HEAD && git push -q origin HEAD:dev || { echo "integ-land: push failed"; exit 1; }
for x in "${shas[@]}"; do echo "${x%% *}" >> "$G/batch-queue.txt"; [ "${x#* }" = regen ] || echo "${x#* }" >> "$G/landed-subs.txt"; echo "LANDED $x"; done
```

#### Jet twin: Tools/agent/orchestrator/IntegLand4.jet

```jet
#!/usr/bin/env jet
// IntegLand4.jet BATCH SUB...: lands the named submissions of a passed
// integration batch on dev, one commit each, in the order given (the order
// integrate.sh applied them). Refuses unless integ/BATCH's census and
// ratchets passed, each SUB's status is OK, and origin/dev is still the
// batch's BASE (any other movement means the composed tree was not the one
// tested: rerun integrate.sh). Commits in the dev worktree stage exactly
// each patch's files; the author's lane name goes in a Lane: trailer. Pushes
// once, then appends the shas to batch-queue.txt.
//
// The Jet twin of integ-land4.sh (gauntlet tool entry integ-land4): the
// batches live in $JET_SCRATCH_ROOT/lanes/Main/integ, the dev worktree is
// $JET_DEV_WORKTREE, and batch-queue.txt and landed-subs.txt are Main's, in
// $JET_STATE_ROOT/scratch/Main, as Tools/agent/roots.sh sets them. As the
// original does after its `cd`, a relative path names a file in the
// worktree from the second step on.
use {
    files: core.files
    process: core.process
    sys: core.sys
    term: core.term
}

// The directories the tree-only check never enters (treeonly.mjs).
SKIPPED_DIRS: [String]{".jet", ".git", "target", "node_modules", ".chain"}

env_or: fn(name: String, fallback: String) -[Env]> String Never! {
    value: process.env_get(name) ?? return fallback.copy()
    if value == "" -> fallback.copy() else -> value
}

// What the machine's jet/env file sets `name` to (the default of its
// `${NAME:-default}` form), None when it sets nothing.
env_file_value: fn(name: String) -[Env, FS]> String? Never! {
    home: env_or("HOME", "")
    text: files.read("{env_or("XDG_CONFIG_HOME", "{home}/.config")}/jet/env") ?? return None
    defaulted: "${{{name}:-"
    found := ""
    seen := false
    loop line in text.lines() {
        parts: line.trim().removeprefix("export ").partition("=")
        if parts.sep == "" || parts.head != name -> next
        value := parts.tail.removeprefix("\"").removesuffix("\"").removeprefix("'").removesuffix("'")
        if value.starts_with(defaulted) && value.ends_with("}}") -> value = value.removeprefix(defaulted).removesuffix("}}")
        found = value.replace("$HOME", home)
        seen = true
    }
    if seen -> Val(found) else -> None
}

// A root as Tools/agent/roots.sh sets it: the environment, else the jet/env
// file, else `fallback`.
root: fn(name: String, fallback: String) -[Env, FS]> String Never! {
    found: env_or(name, "")
    if found != "" -> return found
    env_file_value(name) ?? fallback.copy()
}

state_root: fn() -[Env, FS]> String Never! { root("JET_STATE_ROOT", "{env_or("XDG_CACHE_HOME", "{env_or("HOME", "")}/.cache")}/jet-dev") }

scratch_root: fn() -[Env, FS]> String Never! { root("JET_SCRATCH_ROOT", state_root()) }

// `path` as the shell finds it from `dir`.
at: fn(dir: String, path: String) -> String Never! {
    if path.starts_with("/") -> path.copy() else -> "{dir}/{path}"
}

// `text` without its trailing newlines, as `$(...)` gives it.
chomp: fn(text: String) -> String Never! {
    out := text.copy()
    loop out.ends_with("\n") -> out = out.removesuffix("\n")
    out
}

// The first `count` lines of `text`, each followed by a space (`head -n N | tr '\n' ' '`).
first_lines: fn(text: String, count: Int) -> String Never! {
    out := ""
    taken := 0
    loop line in text.lines() {
        if taken == count -> break
        out = "{out}{line} "
        taken += 1
    }
    out
}

// The words of `text`, as an unquoted `$(cat FILE)` splits it.
words: fn(text: String) -> [String] Never! {
    out := [String]{}
    loop line in text.lines() {
        loop word in line.replace("\t", " ").split(" ") {
            if word != "" -> out.push(word.copy())
        }
    }
    out
}

// Runs git in `dir`, passing its stderr through, and gives its exit code
// and stdout.
git: fn(dir: String, args: [String]) -[Exec, IO, Time.Wait]> (code: Int, output: String) Never! {
    argv := [String]{"git"}
    argv.extend(args.copy())
    receipt: process.cmd(argv).cwd(dir).stdin(.Inherit).stderr(.Inherit).output_limit(1073741824).run() ?? {
        term.eprint("git: {err}")
        return (code: 127, output: "")
    }
    code: if receipt.signal != None -> 128 + (receipt.signal ?? 0) else -> receipt.code
    (code: code, output: receipt.output.copy())
}

head_sha: fn(dir: String) -[Exec, IO, Time.Wait]> String Never! { chomp(git(dir, [String]{"rev-parse", "--short", "HEAD"}).output) }

has_line_prefix: fn(text: String, prefix: String) -> Bool Never! {
    loop line in text.lines() {
        if line.starts_with(prefix) -> return true
    }
    false
}

// The size of the file at `path`, -1 when there is none.
size_of: fn(path: String) -[FS]> Int Never! {
    if files.stat(path) == {
        .Ok(info) -> info.size
        .Err(_) -> -1
    }
}

// The `*.files` lists in `dir`.
file_lists: fn(dir: String) -[FS]> [String] Never! {
    out := [String]{}
    entries: files.list_dir(dir) ?? return out
    loop entry in entries {
        if entry.name.ends_with(".files") -> out.push(entry.name.copy())
    }
    out.sort()
    out
}

// What `cat "$I"/*.files` lists, one path per line.
batch_files: fn(integ: String) -[FS]> [String] Never! {
    out := [String]{}
    loop name in file_lists(integ) {
        loop line in (files.read("{integ}/{name}") ?? "").lines() -> out.push(line.copy())
    }
    out
}

fail: fn(message: String, code: Int) -[Exec, IO]> Never {
    print(message)
    process.exit(code)
}

// The files of `tree` (below `rel`) that git does not track, skipping
// build output (treeonly.mjs).
tree_only: fn(tree: String, rel: String, tracked: [String], out: &[String]) -[FS]> () {
    entries: files.list_dir(if rel == "" -> tree.copy() else -> "{tree}/{rel}") ?? return
    loop entry in entries {
        path: if rel == "" -> entry.name.copy() else -> "{rel}/{entry.name}"
        directory: entry.is_dir && !files.is_symlink(entry.path)
        if {
            directory -> {
                if !SKIPPED_DIRS.contains(entry.name) -> tree_only(tree, path, tracked, out)
            }
            !tracked.contains(path) -> out.push(path)
        }
    }
}

append_line: fn(path: String, line: String) -[FS, IO]> () {
    existing: files.read(path) ?? ""
    files.write(path, "{existing}{line}\n") ?? term.eprint("{path}: {err}")
}

run: fn() {
    args: process.args()
    if args.len() == 0 {
        term.eprint("usage: jet Tools/agent/orchestrator/IntegLand4.jet BATCH SUB...")
        process.exit(1)
    }
    batch: args[0]
    worktree: root("JET_DEV_WORKTREE", files.dirname(files.dirname(files.dirname(sys.executable()))))
    scratch: "{state_root()}/scratch/Main"
    integ: "{scratch_root()}/lanes/Main/integ/{batch}"
    if !has_line_prefix(files.read("{integ}/REPORT.md") ?? "", "Census rc=0 ") -> fail("integ-land: batch {batch} did not pass its census/ratchets", 1)
    if !files.is_dir(worktree) {
        term.eprint("IntegLand4.jet: cd: {worktree}: No such file or directory")
        process.exit(2)
    }
    here: at(worktree, integ)
    if chomp(git(worktree, [String]{"status", "--porcelain=v1", "-uno"}).output) != "" -> fail("integ-land: dev worktree has changes", 1)
    git(worktree, [String]{"fetch", "-q", "origin"})
    base: chomp(files.read("{here}/BASE") ?? "")
    moved: git(worktree, [String]{"diff", "--name-only", base, "origin/dev"}).output
    listed: batch_files(here)
    bad := [String]{}
    loop path in moved.lines() {
        if path == "" -> next
        input: path.starts_with("Compiler/") || path.starts_with("Core/") || path.starts_with("Jetpack/") || path.starts_with("Tools/")
        if input || listed.contains(path) -> bad.push(path.copy())
    }
    if bad.len() > 0 {
        named: bad.join(" ")
        cut: if named.bytes().len() > 300 -> String.from_bytes(named.bytes().slice(0, 300)) ?? named.copy() else -> named.copy()
        fail("integ-land: origin/dev moved from {base} in build inputs or batch files: {cut}; rerun integrate.sh", 1)
    }
    if git(worktree, [String]{"merge", "-q", "--ff-only", "origin/dev"}).code != 0 -> fail("integ-land: cannot fast-forward the worktree to origin/dev", 1)
    landed := [String]{}
    loop given in args.slice(1, args.len()) {
        sub: given.removesuffix("/")
        name: files.basename(sub)
        if chomp(files.read("{here}/{name}.status") ?? "") != "OK" {
            print("integ-land: {name} is not OK in {batch}; stopping before it")
            break
        }
        rebased: "{integ}/{name}.rebased"
        rebased_size: size_of(at(worktree, rebased))
        patch: if rebased_size > 0 -> rebased.copy() else -> "{sub}/patch"
        // An empty .rebased means the integ repo lost the commit (b6d: gc race); the raw patch is not what was tested.
        if rebased_size == 0 -> fail("integ-land: {rebased} is empty; rebuild it (rebuild-rebased.sh) before landing", 1)
        if git(worktree, [String]{"apply", "-p1", "--index", patch}).code != 0 -> fail("integ-land: {name} does not apply on {head_sha(worktree)}", 1)
        // Every path the patch staged, rename sources included (numstat names only the rename target: b6d left 12 behind).
        staged: git(worktree, [String]{"diff", "--cached", "--name-only", "--no-renames"}).output
        files.write("{here}/{name}.landfiles", staged) ?? term.eprint("{integ}/{name}.landfiles: {err}")
        lane: if name.contains("-") -> name.slice(0, name.rfind("-")) else -> name.copy()
        message: files.read(at(worktree, "{sub}/msg")) ?? ""
        files.write("{here}/{name}.commitmsg", "{message}\nLane: {lane}\nIntegrated: {batch} on {base}\n") ?? term.eprint("{integ}/{name}.commitmsg: {err}")
        commit := [String]{"commit", "-q", "-F", "{integ}/{name}.commitmsg", "--"}
        commit.extend(words(staged))
        if git(worktree, commit).code != 0 -> fail("integ-land: commit of {name} failed", 1)
        landed.push("{head_sha(worktree)} {name}")
    }
    if landed.len() == 0 -> process.exit(1)
    // The generated tables integ-apply regenerated from the composed sources land last, only when EVERY SUB the batch
    // applied landed (b4b: landing a subset with the full regen put SchedLead-6 rows on dev without their source).
    applied: file_lists(here).filter(name -> name != "regen.files").len()
    // INTEG_LANDED_BEFORE=k: k SUBs of this batch already landed in an earlier, interrupted run.
    before: env_or("INTEG_LANDED_BEFORE", "0").to_int() ?? 0
    if size_of("{here}/regen.patch") > 0 {
        if landed.len() + before != applied -> fail("integ-land: not every SUB landed, so regen.patch was not applied; rerun the generators", 1)
        if git(worktree, [String]{"apply", "-p1", "--index", "{integ}/regen.patch"}).code != 0 -> fail("integ-land: regen commit failed", 1)
        regen: git(worktree, [String]{"diff", "--cached", "--name-only", "--no-renames"}).output
        files.write("{here}/regen.files", regen) ?? term.eprint("{integ}/regen.files: {err}")
        files.write("{here}/regen.commitmsg", "integration: regenerate the generated tables for {batch}\n\nIntegrated: {batch} on {base}\n") ?? term.eprint("{integ}/regen.commitmsg: {err}")
        commit := [String]{"commit", "-q", "-F", "{integ}/regen.commitmsg", "--"}
        commit.extend(words(regen))
        if git(worktree, commit).code != 0 -> fail("integ-land: regen commit failed", 1)
        landed.push("{head_sha(worktree)} regen")
    }
    // The landed tree must be the tested composed tree, byte for byte, before anything is pushed.
    leftovers: git(worktree, [String]{"status", "--porcelain=v1", "-uno"}).output
    if chomp(leftovers) != "" -> fail("integ-land: staged leftovers after landing: {first_lines(leftovers, 5)}", 1)
    if landed.len() + before >= applied {
        tracked := [String]{}
        loop path in git(worktree, [String]{"-c", "core.quotepath=off", "ls-files"}).output.lines() -> tracked.push(path.copy())
        drift := [String]{}
        loop path in tracked {
            if files.is_symlink(at(worktree, path)) -> next
            if !files.exists("{here}/tree/{path}") {
                if files.exists(at(worktree, path)) -> drift.push("NOTINTREE {path}")
                next
            }
            same: if files.read_bytes(at(worktree, path)) == {
                .Ok(landed_bytes) -> if files.read_bytes("{here}/tree/{path}") == {
                    .Ok(tree_bytes) -> landed_bytes == tree_bytes
                    .Err(_) -> false
                }
                .Err(_) -> false
            }
            if !same -> drift.push("DIFF {path}")
        }
        only := [String]{}
        tree_only("{here}/tree", "", tracked, only)
        loop path in only.slice(0, if only.len() > 40 -> 40 else -> only.len()) {
            if path.trim() != "" -> drift.push("ONLYINTREE {path}")
        }
        if drift.len() > 0 -> fail("integ-land: landed tree differs from {integ}/tree: {first_lines(drift.join("\n"), 8)}", 1)
    }
    pushed: git(worktree, [String]{"fetch", "-q", "origin"}).code == 0 && git(worktree, [String]{"merge-base", "--is-ancestor", "origin/dev", "HEAD"}).code == 0 && git(worktree, [String]{"push", "-q", "origin", "HEAD:dev"}).code == 0
    if !pushed -> fail("integ-land: push failed", 1)
    queue: at(worktree, scratch)
    loop entry in landed {
        parts: entry.partition(" ")
        append_line("{queue}/batch-queue.txt", parts.head)
        if parts.tail != "regen" -> append_line("{queue}/landed-subs.txt", parts.tail)
        print("LANDED {entry}")
    }
}
```

### integrate10

Main's integration pass integrate10.sh and its Jet twin Integrate10.jet (#5072 c15): export origin/dev from the dev worktree's repository, apply INTEG_PRE's stacked patches and then every SUB through integ-apply6.sh, stop on a failed regeneration, run the census (census-snap2.sh under capped.sh) with the files the applied SUBs touch, then every ratchet check with the seed at once, link the batch compiler and jetpacks, run each SUB's test lines four SUBs at a time with runsub6.sh, rerun the failing ones on a clean dev export with the seed, run the goldens that cite a touched Core or Prelude file, and write the report; it prints the applied SUBs' lines, the census and ratchet results, the jetpack choice and the report's path. The fixtures hold the dev repository (dev.git, whose export links back to the fixture's lanes and subs), the worktree's repository that fetches it (work.git), the SUBs (subs), the census rows (rows), a stand-in seed (seed), stand-ins for integ-apply6.sh, census-snap2.sh and the compiler the census builds (orch), and stand-ins for Main's capped.sh and runsub6.sh (lanes/scratch/Main). The cases run under LC_ALL=C, so the original's sorts keep the byte order the twin's do.

- Original: `Tools/agent/orchestrator/integrate10.sh` (shell (bash), 152 lines)
- Jet twin: `Tools/agent/orchestrator/Integrate10.jet` (755 lines)
- Fixture cases: 9 (args per case: `b1 subs/A-1/ subs/B-1 subs/C-1 subs/D-1 subs/E-1 subs/F-1`; `b2 subs/A-1 subs/C-1 subs/E-1`; `b3 subs/A-1 subs/D-1`; `b4`; `b5 subs/A-1` exit 3; `b6 subs/A-1` exit 2; `b7 subs/A-1` exit 2; `b8 subs/A-1` exit 2; `(none)` exit 1)

| Bar | Result | Measurement |
|---|---|---|
| parity | PASS | 9/9 cases identical |
| speed | FAIL | wall Jet/original 0.83 (588.8 ms against 711.6 ms; every Jet round below every original round); peak memory Jet/original 2.21 (15.2 MB against 6.9 MB; lost: every original round below every Jet round); medians of 5 interleaved rounds |
| tokens | FAIL | Jet 5843, original 1616 (+4227, Jet/original 3.62) |
| reasoning | FAIL | no review record `Tools/gauntlet/entries/integrate10/review.json`; branches Jet 185, original 64 (+121); control nesting Jet 4, original 5 (-1) |
| verdict | FAIL | loses speed, tokens, reasoning; the original keeps running |

#### Original: Tools/agent/orchestrator/integrate10.sh

```sh
#!/usr/bin/env bash
# integrate.sh BATCH SUB... : Main's integration pass (owner order 2026-11-08: leads write code, Main builds and
# tests). Each SUB is an inbox folder ($JET_SCRATCH_ROOT/lanes/Main/inbox/<Lead>-<n>/) holding `patch` (git apply
# -p1 against origin/dev), `msg`, `tests` (one shell command per line, run from the tree root with `jet` = the
# compiler this pass builds) and `cards`. Steps, all collect-all:
#   1. export origin/dev to integ/BATCH/tree and apply every SUB in order (a SUB that does not apply is reported
#      and skipped; the rest still run);
#   2. one census of the composed tree (check, build, guards, runtime pack, self-check, Core self-check) through
#      census-snap.sh under the shared precommit lock and a 32G cap; every keep-going row is attributed to the
#      SUBs whose touched files it names;
#   3. if the build produced a compiler, every SUB's tests run with it, each recorded with rc and output tail.
# Writes integ/BATCH/REPORT.md (one section per SUB: OK, or every failure found) and prints its path.
# Never edit this file in place while it runs: write a new file and mv it over.
# The original of gauntlet tool entry integrate10; Integrate10.jet beside it is its Jet twin. The batches are in
# $JET_SCRATCH_ROOT/lanes/Main/integ, the dev worktree is $JET_DEV_WORKTREE and the seed $JET_SEED; integ-apply6.sh
# and census-snap2.sh are the ones in $JET_ORCHESTRATOR_DIR (this file's directory unless set), and capped.sh and
# runsub6.sh are Main's, in $JET_STATE_ROOT/scratch/Main (Tools/agent/roots.sh).
set -u
. "$(dirname "$0")/../roots.sh"
BATCH=${1:?batch}; shift
M=$JET_SCRATCH_ROOT/lanes/Main
W=$JET_DEV_WORKTREE
G=$JET_STATE_ROOT/scratch/Main
O=${JET_ORCHESTRATOR_DIR:-$(dirname "$0")}
I=$M/integ/$BATCH; T=$I/tree
rm -rf "$I"; mkdir -p "$T" "$I/tests" "$I/bin"
( cd "$W" && git fetch -q origin && git rev-parse --short origin/dev > "$I/BASE" && git archive origin/dev | tar -x -C "$T" ) || { echo "integrate: export failed"; exit 2; }
R=$I/REPORT.md
printf '# Integration %s on origin/dev %s\n\n' "$BATCH" "$(cat "$I/BASE")" > "$R"
# INTEG_PRE="BATCH:SUB ..." stacks this batch on another batch still landing: those SUBs (already tested there) are
# applied first, untested and unreported; integ-land accepts origin/dev moving only by that batch commits.
for ps in ${INTEG_PRE:-}; do
  pb=${ps%%:*}; pd=${ps#*:}
  ( cd "$T" && git apply -p1 "$pd/patch" ) || { echo "integrate: pre $pd does not apply"; exit 2; }
  echo "$pb" >> "$I/PRE"; echo "PRE $(basename "$pd") from $pb"
done
# Apply (three-way, generated files excluded and regenerated): integ-apply.sh prints the applied SUBs last.
bash "$O/integ-apply6.sh" "$T" "$I" "$@" > "$I/apply.out" 2>&1 || { echo "integrate: the apply step failed: $(awk '/^integ-apply:/' "$I/apply.out" | tail -1)"; exit 2; }
awk "/^--applied--$/{exit} {print}" "$I/apply.out"
mapfile -t applied < <(awk "f{print} /^--applied--$/{f=1}" "$I/apply.out" | awk NF)
# A generator that fails on the composed sources leaves every generated table stale, so the built compiler would
# miss the batch's new rows and every later result would be noise: stop here and report the regeneration.
if awk '/^REGEN / && !/ rc=0$/{bad=1} END{exit !bad}' "$I/apply.out"; then
  { printf 'REGEN FAILED: the census did not run.\n'; for f in "$I"/regen.*.out; do printf '\n%s:\n```\n' "$(basename "$f")"; tail -20 "$f" | cut -c1-300; printf '```\n'; done; } >> "$R"
  echo REGENFAIL > "$I/REGENFAIL"; echo "$R"; exit 3
fi
# The census: every file any applied SUB touches selects the Jetpack pass when it is under Jetpack/.
mapfile -t touched < <(for s in "${applied[@]}"; do cat "$I/$(basename "$s").files"; done | sort -u)
echo "census start"
CENSUS_TREE=$T CENSUS_OUT=$I/census CAP_CPUS=16-31 bash "$G/capped.sh" 32G bash "$O/census-snap2.sh" "${touched[@]}" > "$I/census.out" 2>&1
crc=$?
echo "census rc=$crc"
# The repository ratchets and generator checks (Land.jet ratchets, as precommit.sh runs them), with the seed on
# the composed tree: a batch that passes here lands without a second census.
C=$(readlink -f "$JET_SEED")
mapfile -t ratchets < <(cd "$T" && JET_COMPILER_IDENTITY=$(cat "$C/IDENTITY") JET_TOOLCHAIN_ROOT=$T "$C/jet" Tools/agent/Land.jet ratchets 2>/dev/null)
mkdir -p "$I/ratchets"; rrc=0
[ ${#ratchets[@]} -gt 0 ] || { echo "no ratchets listed" > "$I/ratchets/none.out"; rrc=1; }
for tool in "${ratchets[@]}"; do
  ( cd "$T" && JET_COMPILER_IDENTITY=$(cat "$C/IDENTITY") JET_TOOLCHAIN_ROOT=$T timeout 900 nice -n 19 "$C/jet" "$tool" --check > "$I/ratchets/$(basename "$tool").out" 2>&1; echo $? > "$I/ratchets/$(basename "$tool").rc" ) &
done
wait
for f in "$I"/ratchets/*.rc; do [ -e "$f" ] || continue; [ "$(cat "$f")" = 0 ] || rrc=1; done
echo "ratchets rc=$rrc"
[ "$rrc" = 0 ] || crc=1
SELF=$T/Compiler/JetCli/.jet/build/jet
ID=$(cat "$(readlink -f "$JET_SEED")/IDENTITY")
built=0; [ -x "$SELF" ] && built=1 && ln -sfn "$SELF" "$I/bin/jet"
# The engine `jet` talks to must be the seed's jetpack, whose identity JET_COMPILER_IDENTITY names (not the one on the login PATH).
[ -x "$(readlink -f "$JET_SEED")/jetpack" ] && ln -sfn "$(readlink -f "$JET_SEED")/jetpack" "$I/bin/jetpack"
# The engine beside the batch compiler must be the jetpack this tree builds (the version cut makes the seed's
# jetpack disagree with the batch jet): the census's hosted bootstrap names it as hosted-binary.
HB=$(awk "/^hosted-binary: /{print \$2}" "$I/census/jetpack.log" 2>/dev/null | tail -1)
if [ -n "$HB" ] && [ -x "$HB" ]; then cp "$HB" "$I/bin/jetpack.built" && ln -sfn "$I/bin/jetpack.built" "$I/bin/jetpack" && ln -sfn "$I/bin/jetpack.built" "$(dirname "$(readlink -f "$SELF")")/jetpack"; echo "tests use the built jetpack $HB"; else echo "no built jetpack (census jetpack row failed); tests use the seed's"; fi
export I T SELF ID G
# Every SUB runs its test lines as one job, four SUBs at a time (owner 2026-11-09: land fast; serial tests took 1.5 h).
if [ $built = 1 ]; then
  for s in "${applied[@]}"; do [ -s "$s/tests" ] && printf "%s\n" "$s"; done | xargs -r -P 4 -I{} bash "$G/runsub6.sh" new {}
fi
# A failing line reruns on a clean origin/dev export with the seed, so rows that already fail on dev are told apart
# from the ones this batch introduces (I/tests/SUB.K.base.out; a FAIL row present in both counts as pre-existing).
if [ $built = 1 ] && cat "$I"/tests/*.rc 2>/dev/null | awk -F"\t" "\$1!=0{f=1} END{exit !f}"; then
  B=$I/basetree; C0=$(readlink -f "$JET_SEED"); mkdir -p "$B" "$I/basebin"
  ( cd "$W" && git archive "$(cat "$I/BASE")" | tar -x -C "$B" )
  ln -sfn "$C0/jet" "$I/basebin/jet"; ln -sfn "$C0/jetpack" "$I/basebin/jetpack"
  export B C0
  for rcf in "$I"/tests/*.rc; do awk -F"\t" "\$1!=0{f=1} END{exit !f}" "$rcf" && printf "%s\n" "$(basename "$rcf" .rc)"; done | xargs -r -P 4 -I{} bash "$G/runsub6.sh" base {}
  rm -rf "$B"
fi
# Goldens that cite a line of a Core or Prelude file this batch touches (`<corelib>/Core/X.jet:N`) drift when
# that file moves; run each one so the batch, not the release gate, catches it.
if [ $built = 1 ]; then
  mapfile -t corefiles < <(printf "%s\n" "${touched[@]}" | awk "/^(Core|Compiler\/Prelude)\/.*\.jet$/")
  : > "$I/goldens.list"
  for f in "${corefiles[@]}"; do ( cd "$T" && git grep -l --no-index -F "<corelib>/$f:" -- "*.out" 2>/dev/null ) >> "$I/goldens.list"; done
  sort -u -o "$I/goldens.list" "$I/goldens.list"
  while IFS= read -r g; do
    [ -n "$g" ] || continue; stem=${g%.out}; stem=${stem%.err}
    ( cd "$T" && env PATH="$I/bin:$PATH" JET="$SELF" JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$T" \
        CAP_CPUS=16-31 timeout 1800 bash "$G/capped.sh" 24G nice -n 19 jet test Compiler --profile golden "$stem" ) > "$I/goldens.$(printf %s "$stem" | tr / _).out" 2>&1
    printf "%s\t%s\n" "$?" "$stem" >> "$I/goldens.rc"
  done < "$I/goldens.list"
  # Reported in the summary, not folded into crc: Main attributes a drifted golden to the SUB that moved the line.
fi
# Report.
rows=$(cat "$I/census/"*keep-going.tsv 2>/dev/null)
{
  printf 'Census rc=%s (built compiler: %s). Summary:\n```\n' "$crc" "$([ $built = 1 ] && echo yes || echo no)"
  awk '/ rc=|rows=/' "$I/census.out"
  for f in "$I"/ratchets/*.rc; do [ -e "$f" ] || continue; printf 'ratchet %s rc=%s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
  [ -f "$I/goldens.rc" ] && awk -F"\t" '{print "golden " $2 " rc=" $1}' "$I/goldens.rc"
  printf '```\n'
  for f in "$I"/ratchets/*.rc; do [ -e "$f" ] && [ "$(cat "$f")" != 0 ] && { printf 'Ratchet %s failed:\n```\n' "$(basename "$f" .rc)"; tail -30 "${f%.rc}.out" | cut -c1-300; printf '```\n'; }; done
  printf '\n'
  for s in "$@"; do
    s=${s%/}; n=$(basename "$s")
    printf '## %s\n' "$n"
    if [ -f "$I/$n.status" ]; then printf 'Does not apply to origin/dev %s:\n```\n' "$(cat "$I/BASE")"; cut -c1-300 "$I/$n.apply" | awk 'NR<=20'; printf '```\n\n'; continue; fi
    bad=0
    hits=$(while IFS= read -r f; do [ -n "$f" ] && printf '%s\n' "$rows" | grep -F -- "$f"; done < "$I/$n.files" | sort -u | cut -c1-400)
    if [ -n "$hits" ]; then bad=1; printf 'Census rows in files this patch touches:\n```\n%s\n```\n' "$hits"; fi
    if [ -f "$I/tests/$n.rc" ]; then
      k=0
      while IFS=$'\t' read -r rc line; do
        k=$((k+1))
        if [ "$rc" = 0 ]; then printf -- '- PASS `%s`\n' "$line"; continue; fi
        o=$I/tests/$n.$k.out; bo=$I/tests/$n.$k.base.out
        if [ -f "$bo" ] && [ "$(cat "$I/tests/$n.$k.base.rc")" != 0 ]; then
          frows() { awk '/FAIL/{gsub(/[0-9]+(\.[0-9]+)? ?(ms|s|µs)/,""); print}' "$1" | sort -u; }
          new=$(comm -23 <(frows "$o") <(frows "$bo"))
          if [ -z "$new" ] && [ -n "$(frows "$o")" ]; then
            printf -- '- PRE-EXISTING rc=%s `%s`: every FAIL row also fails on origin/dev with the seed (%s)\n' "$rc" "$line" "$bo"; continue
          fi
          bad=1; printf -- '- FAIL rc=%s `%s` (full output %s; dev with the seed also fails, rc=%s, %s). FAIL rows not on dev:\n```\n' "$rc" "$line" "$o" "$(cat "$I/tests/$n.$k.base.rc")" "$bo"
          if [ -n "$new" ]; then printf '%s\n' "$new" | cut -c1-300 | awk 'NR<=25'; else awk '/error|Error|panic|failed/' "$o" | cut -c1-300 | awk 'NR<=25'; tail -5 "$o" | cut -c1-300; fi
          printf '```\n'; continue
        fi
        bad=1; printf -- '- FAIL rc=%s `%s` (full output %s; passes on origin/dev with the seed or was not rerun):\n```\n' "$rc" "$line" "$o"; awk '/FAIL|error|Error|panic|failed/' "$o" | cut -c1-300 | awk 'NR<=25'; tail -5 "$o" | cut -c1-300; printf '```\n'
      done < "$I/tests/$n.rc"
    elif [ $built = 0 ]; then bad=1; printf 'Tests not run: the composed tree did not build (see census).\n'
    else printf 'No tests listed.\n'; fi
    if [ $bad = 0 ] && [ "$crc" = 0 ]; then printf '\n**OK**\n\n'; echo OK > "$I/$n.status"; else printf '\n**NOT OK**\n\n'; echo BAD > "$I/$n.status"; fi
  done
  if [ "$crc" != 0 ]; then
    printf '## Census rows not in any submitted file\n```\n'
    all=$(for s in "${applied[@]}"; do cat "$I/$(basename "$s").files"; done)
    printf '%s\n' "$rows" | grep -v -F -f <(printf '%s\n' "$all" | awk 'NF') | cut -c1-300 | awk 'NR<=40'
    printf '```\n'
  fi
} >> "$R"
echo "$R"
```

#### Jet twin: Tools/agent/orchestrator/Integrate10.jet

```jet
#!/usr/bin/env jet
// Integrate10.jet BATCH SUB...: Main's integration pass. Each SUB is an inbox
// folder ($JET_SCRATCH_ROOT/lanes/Main/inbox/<Lead>-<n>/) holding `patch`
// (git apply -p1 against origin/dev), `msg`, `tests` (one shell command per
// line, run from the tree root with `jet` = the compiler this pass builds)
// and `cards`. Steps, all collect-all:
//   1. export origin/dev to integ/BATCH/tree and apply every SUB in order (a
//      SUB that does not apply is reported and skipped; the rest still run);
//   2. one census of the composed tree (check, build, guards, runtime pack,
//      self-check, Core self-check) through census-snap2.sh under a 32G cap;
//      every keep-going row is attributed to the SUBs whose touched files it
//      names;
//   3. if the build produced a compiler, every SUB's tests run with it, each
//      recorded with rc and output tail.
// Writes integ/BATCH/REPORT.md (one section per SUB: OK, or every failure
// found) and prints its path.
//
// The Jet twin of integrate10.sh (gauntlet tool entry integrate10): the
// batches are in $JET_SCRATCH_ROOT/lanes/Main/integ, the dev worktree is
// $JET_DEV_WORKTREE and the seed $JET_SEED; integ-apply6.sh and
// census-snap2.sh are the ones in $JET_ORCHESTRATOR_DIR (this program's
// directory unless set), and capped.sh and runsub6.sh are Main's, in
// $JET_STATE_ROOT/scratch/Main, as Tools/agent/roots.sh sets them. A relative
// path the original names after a `cd` is read from that directory, as the
// shell reads it. Lines sort in byte order, as the original sorts them with
// LC_ALL=C.
use {
    files: core.files
    process: core.process
    regex: core.regex
    sys: core.sys
    term: core.term
    time: core.time
}

// The test durations a FAIL row drops before two runs' rows are compared.
DURATION: Regex{"[0-9]+(\.[0-9]+)? ?(ms|s|µs)"}
// The touched files whose lines goldens cite.
CORE_SOURCE: Regex{"^(Core|Compiler/Prelude)/.*\.jet$"}

// One variable the test jobs get.
Var: struct {
    name: String
    value: String
}

// The census's exit status (1 when a ratchet failed too) and whether it built
// the batch compiler.
CensusResult: struct {
    crc: Int
    built: Bool
}

env_or: fn(name: String, fallback: String) -[Env]> String Never! {
    value: process.env_get(name) ?? return fallback.copy()
    if value == "" -> fallback.copy() else -> value
}

// What the machine's jet/env file sets `name` to (the default of its
// `${NAME:-default}` form), None when it sets nothing.
env_file_value: fn(name: String) -[Env, FS]> String? Never! {
    home: env_or("HOME", "")
    text: files.read("{env_or("XDG_CONFIG_HOME", "{home}/.config")}/jet/env") ?? return None
    defaulted: "${{{name}:-"
    found := ""
    seen := false
    loop line in text.lines() {
        parts: line.trim().removeprefix("export ").partition("=")
        if parts.sep == "" || parts.head != name -> next
        value := parts.tail.removeprefix("\"").removesuffix("\"").removeprefix("'").removesuffix("'")
        if value.starts_with(defaulted) && value.ends_with("}}") -> value = value.removeprefix(defaulted).removesuffix("}}")
        found = value.replace("$HOME", home)
        seen = true
    }
    if seen -> Val(found) else -> None
}

// A root as Tools/agent/roots.sh sets it: the environment, else the jet/env
// file, else `fallback`.
root: fn(name: String, fallback: String) -[Env, FS]> String Never! {
    found: env_or(name, "")
    if found != "" -> return found
    env_file_value(name) ?? fallback.copy()
}

state_root: fn() -[Env, FS]> String Never! { root("JET_STATE_ROOT", "{env_or("XDG_CACHE_HOME", "{env_or("HOME", "")}/.cache")}/jet-dev") }

scratch_root: fn() -[Env, FS]> String Never! { root("JET_SCRATCH_ROOT", state_root()) }

seed_dir: fn() -[Env, FS]> String Never! {
    seeds: root("JET_SEEDS", "{env_or("XDG_DATA_HOME", "{env_or("HOME", "")}/.local/share")}/jet-seeds")
    root("JET_SEED", "{seeds}/current")
}

// `text` without its trailing newlines, as `$(...)` gives it.
chomp: fn(text: String) -> String Never! {
    out := text.copy()
    loop out.ends_with("\n") -> out = out.removesuffix("\n")
    out
}

// The lines awk reads from `text`: a last line without its newline counts.
records: fn(text: String) -> [String] Never! {
    out := text.split("\n").to_list()
    if out.len() > 0 && out[out.len() - 1] == "" -> _ := out.pop()
    out
}

// The lines `while read` reads from `text`: a last line without its newline
// does not count.
read_lines: fn(text: String) -> [String] Never! {
    out := text.split("\n").to_list()
    _ := out.pop()
    out
}

// The lines awk reads from the file at `path`, none when it can't be read.
lines_of: fn(path: String) -[FS]> [String] Never! { records(files.read(path) ?? "") }

// The words the shell splits `text` into (no globbing).
words: fn(text: String) -> [String] Never! {
    out := [String]{}
    loop word in text.replace("\t", " ").replace("\n", " ").split(" ") {
        if word != "" -> out.push(word.copy())
    }
    out
}

// `lines` sorted, each once (`sort -u`).
unique: fn(lines: [String]) -> [String] Never! {
    sorted := lines.copy()
    sorted.sort()
    out := [String]{}
    loop line in sorted {
        if out.len() == 0 || out[out.len() - 1] != line -> out.push(line.copy())
    }
    out
}

// The last `count` of `lines` (`tail -count`).
tail: fn(lines: [String], count: Int) -> [String] Never! {
    lines[(if lines.len() > count -> lines.len() - count else -> 0)..<lines.len()]
}

// The first `count` of `lines` (`awk 'NR<=count'`).
head: fn(lines: [String], count: Int) -> [String] Never! {
    lines[0..<(if lines.len() < count -> lines.len() else -> count)]
}

// The lines that hold one of `needles` (`awk '/a|b/'`).
holding: fn(lines: [String], needles: [String]) -> [String] Never! {
    out := [String]{}
    loop line in lines {
        probe: line.copy()
        if needles.any(needle -> probe.contains(needle)) -> out.push(line.copy())
    }
    out
}

// `text` without the tabs at its start and end, as `IFS=$'\t' read` trims.
trim_tabs: fn(text: String) -> String Never! {
    out := text.copy()
    loop out.starts_with("\t") -> out = out.removeprefix("\t")
    loop out.ends_with("\t") -> out = out.removesuffix("\t")
    out
}

// `path` as the shell opens it after a `cd dir`.
within: fn(dir: String, path: String) -> String Never! {
    if path.starts_with("/") -> path.copy() else -> "{dir}/{path}"
}

// The names in `dir` that `dir/prefix*suffix` matches, sorted.
matching: fn(dir: String, prefix: String, suffix: String) -[FS]> [String] Never! {
    names := [String]{}
    loop entry in (files.list_dir(dir) ?? return [String]{}) {
        name: entry.name
        if !name.starts_with(".") && name.starts_with(prefix) && name.ends_with(suffix) && name.bytes().len() >= prefix.bytes().len() + suffix.bytes().len() -> names.push(name.copy())
    }
    names.sort()
    names
}

// The text of every file in `dir` that `dir/*suffix` matches, joined as `cat`
// joins them.
cat_matching: fn(dir: String, suffix: String) -[FS]> String Never! {
    out := ""
    loop name in matching(dir, "", suffix) -> out = "{out}{files.read("{dir}/{name}") ?? ""}"
    out
}

// Whether `path` names something of nonzero size (`[ -s path ]`).
nonempty: fn(path: String) -[FS]> Bool Never! {
    info: files.stat(path, links: .Follow) ?? return false
    info.size > 0
}

// `readlink -f path`: the path with every symlink resolved; a last part that
// does not exist stays as it is.
readlink_f: fn(path: String) -[FS, Env]> String Never! {
    whole: files.canonicalize(path) ?? {
        parent: files.canonicalize(if files.dirname(path) == "" -> "." else -> files.dirname(path)) ?? return path.copy()
        return "{parent}/{files.basename(path)}"
    }
    whole
}

// `ln -sfn target link`: a link that is a directory (not a link to one) gets
// the new link inside it; whether it was made.
link_sfn: fn(target: String, link: String) -[FS, IO]> Bool Never! {
    at: if files.is_dir(link) && !files.is_symlink(link) -> "{link}/{files.basename(target)}" else -> link.copy()
    if files.lexists(at) -> files.remove(at) ?? ()
    files.symlink(target, at) ?? {
        term.eprint("ln: failed to create symbolic link '{at}': {err}")
        return false
    }
    true
}

// `cp from to`: the copy keeps the permission bits; whether it was made.
copy_file: fn(from: String, to: String) -[FS, IO]> Bool Never! {
    files.copy(from, to) ?? {
        term.eprint("cp: {from}: {err}")
        return false
    }
    info: files.stat(from, links: .Follow) ?? return true
    files.set_mode(to, info.mode % 512) ?? term.eprint("cp: {to}: {err}")
    true
}

// Writes `bytes` to stdout now, before any child writes to it.
emit: fn(bytes: [U8]) Never! -[IO]> {
    out := term.stdout()
    out.write_bytes(bytes) ?? ()
    out.flush() ?? ()
}

say: fn(line: String) Never! -[IO]> { emit("{line}\n".bytes()) }

// `printf ... >> path`.
append: fn(path: String, bytes: [U8]) Never! -[FS, IO]> {
    writer: files.append(path) ?? {
        term.eprint("integrate: {path}: {err}")
        return
    }
    writer.write_all(bytes) ?? term.eprint("integrate: {path}: {err}")
    writer.flush() ?? ()
}

// `printf ... > path`.
put: fn(path: String, text: String) Never! -[FS, IO]> {
    files.write(path, text) ?? term.eprint("integrate: {path}: {err}")
}

add: fn(out: &[U8], text: String) { out.extend(text.bytes()) }

// `... | cut -c1-width`: each line cut to its first `width` bytes.
add_cut: fn(out: &[U8], lines: [String], width: Int) {
    loop line in lines {
        raw: line.bytes()
        out.extend(raw.slice(0, if raw.len() < width -> raw.len() else -> width))
        out.push(U8{10})
    }
}

// The exit status the shell gives a finished child: 124 for one `timeout`
// stopped, 128 + N for signal N.
status: fn(receipt: ProcessReceipt) -> Int Never! {
    if receipt.timed_out -> return 124
    if receipt.signal == .Val(number) -> return 128 + number
    receipt.code
}

// Ends the run with `code` after `line` on stdout.
stop: fn(line: String, code: Int) -[Exec, IO]> Never {
    say(line)
    process.exit(code)
}

// Ends the run with `code` after `message` on stderr.
quit: fn(message: String, code: Int) -[Exec, IO]> Never {
    term.eprint(message)
    process.exit(code)
}

// Runs `spec` on the program's streams; its exit status.
runs: fn(spec: ProcessSpec) -[Exec, IO, Time.Wait]> Int Never! {
    receipt: spec.stdin(.Inherit).stdout(.Inherit).stderr(.Inherit).run() ?? {
        term.eprint("integrate: {err}")
        return 127
    }
    status(receipt)
}

// `$(spec 2>/dev/null)`, its stdout as it printed it.
quiet_output: fn(spec: ProcessSpec) -[Exec, Time.Wait]> String Never! {
    receipt: spec.stdin(.Inherit).stdout(.Capture).stderr(.Capture).output_limit(1073741824).run() ?? return ""
    receipt.output
}

// `spec > path`, its stderr as the caller set it; the shell's exit status. As
// in the shell, a file that can't be opened fails (1) before `spec` runs.
into: fn(spec: ProcessSpec, path: String) -[Exec, FS, IO, Time.Wait]> Int Never! {
    files.write(path, "") ?? {
        term.eprint("integrate: {path}: {err}")
        return 1
    }
    receipt: spec.stdin(.Inherit).stdout(.Capture).output_limit(1073741824).run() ?? {
        term.eprint("integrate: {err}")
        return 127
    }
    files.write(path, receipt.output) ?? term.eprint("integrate: {path}: {err}")
    status(receipt)
}

// `spec > path 2>&1`.
into_merged: fn(spec: ProcessSpec, path: String) -[Exec, FS, IO, Time.Wait]> Int Never! { into(spec.stderr(.Stdout), path) }

// `spec` with the variables `vars` set.
with_vars: fn(spec: ProcessSpec, vars: [Var]) -> ProcessSpec Never! {
    out := spec
    loop item in vars -> out = out.env(item.name, item.value)
    out
}

// `... | xargs -r -P 4 -I{} bash G/runsub6.sh MODE {}`: one job per item,
// four at a time, each on the program's streams with `vars` exported.
runsubs: fn(g: String, mode: String, items: [String], vars: [Var]) Never! -[Exec, IO, Time.Wait]> {
    if items.len() == 0 -> return
    specs := [ProcessSpec]{}
    loop item in items {
        spec: process.cmd([String]{"bash", "{g}/runsub6.sh", mode.copy(), item.copy()}).stdin(.Inherit).stdout(.Inherit).stderr(.Inherit)
        specs.push(with_vars(spec, vars))
    }
    ran: process.run_all(specs, 4) ?? {
        term.eprint("xargs: {err}")
        return
    }
    loop result in ran {
        if result == .Err(error) -> term.eprint("xargs: {error}")
    }
}

// `(cd W && git fetch -q origin && git rev-parse --short origin/dev > I/BASE
// && git archive origin/dev | tar -x -C T)`: whether it succeeded.
exported: fn(w: String, i: String, t: String) -[Exec, FS, IO, Time.Wait]> Bool Never! {
    if !files.is_dir(w) {
        term.eprint("integrate: cd: {w}: No such file or directory")
        return false
    }
    if runs(process.cmd([String]{"git", "fetch", "-q", "origin"}).cwd(w)) != 0 -> return false
    based: process.cmd([String]{"git", "rev-parse", "--short", "origin/dev"}).cwd(w).stderr(.Inherit)
    if into(based, within(w, "{i}/BASE")) != 0 -> return false
    archive: process.pipeline([ProcessSpec]{
        process.cmd([String]{"git", "archive", "origin/dev"}).cwd(w),
        process.cmd([String]{"tar", "-x", "-C", t.copy()}).cwd(w),
    }) ?? {
        term.eprint("integrate: {err}")
        return false
    }
    if archive.errors != "" -> term.eprint(chomp(archive.errors))
    archive.success
}

// `(cd W && git archive "$(cat I/BASE)" | tar -x -C B)`: the base commit's
// tree for the reruns.
base_export: fn(w: String, i: String, b: String) Never! -[Exec, FS, IO, Time.Wait]> {
    if !files.is_dir(w) {
        term.eprint("integrate: cd: {w}: No such file or directory")
        return
    }
    commit: chomp(files.read(within(w, "{i}/BASE")) ?? "")
    archive: process.pipeline([ProcessSpec]{
        process.cmd([String]{"git", "archive", commit}).cwd(w),
        process.cmd([String]{"tar", "-x", "-C", b.copy()}).cwd(w),
    }) ?? {
        term.eprint("integrate: {err}")
        return
    }
    if archive.errors != "" -> term.eprint(chomp(archive.errors))
}

// INTEG_PRE="BATCH:SUB ...": those SUBs (already tested in a batch still
// landing) applied first, untested and unreported.
pre_apply: fn(i: String, t: String) Never! -[Env, Exec, FS, IO, Time.Wait]> {
    loop ps in words(env_or("INTEG_PRE", "")) {
        parts: ps.partition(":")
        pb: parts.head.copy()
        pd: if parts.sep == "" -> ps.copy() else -> parts.tail.copy()
        applied: if files.is_dir(t) -> runs(process.cmd([String]{"git", "apply", "-p1", "{pd}/patch"}).cwd(t)) else -> 1
        if applied != 0 -> stop("integrate: pre {pd} does not apply", 2)
        append("{i}/PRE", "{pb}\n".bytes())
        say("PRE {files.basename(pd)} from {pb}")
    }
}

// Stops the run (3) when a generator failed on the composed sources, with the
// regeneration logs in the report.
regen_check: fn(i: String, report: String, apply_lines: [String]) Never! -[Exec, FS, IO]> {
    if !apply_lines.any(line -> line.starts_with("REGEN ") && !line.ends_with(" rc=0")) -> return
    out := [U8]{}
    add(out, "REGEN FAILED: the census did not run.\n")
    logs := matching(i, "regen.", ".out")
    if logs.len() == 0 -> logs.push("regen.*.out")
    loop name in logs {
        add(out, "\n{name}:\n```\n")
        add_cut(out, tail(lines_of("{i}/{name}"), 20), 300)
        add(out, "```\n")
    }
    append(report, out)
    put("{i}/REGENFAIL", "REGENFAIL\n")
    stop(report, 3)
}

// The repository ratchets and generator checks (Land.jet ratchets), with the
// seed on the composed tree, all at once; 0 when every one passes.
ratchet_checks: fn(i: String, t: String) -[Env, Exec, FS, IO, Time.Wait]> Int Never! {
    c: readlink_f(seed_dir())
    identity: chomp(files.read("{c}/IDENTITY") ?? "")
    listing: process.cmd([String]{"{c}/jet", "Tools/agent/Land.jet", "ratchets"}).cwd(t).env("JET_COMPILER_IDENTITY", identity).env("JET_TOOLCHAIN_ROOT", t)
    tools: if files.is_dir(t) -> records(quiet_output(listing)) else -> [String]{}
    files.create_dir_all("{i}/ratchets") ?? term.eprint("mkdir: {i}/ratchets: {err}")
    rrc := 0
    if tools.len() == 0 {
        put("{i}/ratchets/none.out", "no ratchets listed\n")
        rrc = 1
    }
    specs := [ProcessSpec]{}
    started := [String]{}
    loop tool in tools {
        name: files.basename(tool)
        // As in the shell, a log that can't be opened fails (1) before the check runs.
        files.write(within(t, "{i}/ratchets/{name}.out"), "") ?? {
            term.eprint("integrate: {i}/ratchets/{name}.out: {err}")
            put(within(t, "{i}/ratchets/{name}.rc"), "1\n")
            next
        }
        started.push(name.copy())
        specs.push(process.cmd([String]{"nice", "-n", "19", "{c}/jet", tool.copy(), "--check"}).cwd(t)
            .env("JET_COMPILER_IDENTITY", identity).env("JET_TOOLCHAIN_ROOT", t).timeout(time.duration_seconds(900))
            .stdin(.Inherit).stdout(.Capture).stderr(.Stdout).output_limit(1073741824))
    }
    ran := [Result<ProcessReceipt, IOError>]{}
    if specs.len() > 0 {
        ran = process.run_all(specs, specs.len()) ?? {
            term.eprint("integrate: {err}")
            [Result<ProcessReceipt, IOError>]{}
        }
    }
    loop at in 0..<ran.len() {
        name: started[at]
        code: if ran[at] == {
            .Ok(receipt) -> {
                files.write(within(t, "{i}/ratchets/{name}.out"), receipt.output) ?? term.eprint("integrate: {i}/ratchets/{name}.out: {err}")
                status(receipt)
            }
            .Err(error) -> {
                term.eprint("integrate: {error}")
                127
            }
        }
        put(within(t, "{i}/ratchets/{name}.rc"), "{code}\n")
    }
    loop name in matching("{i}/ratchets", "", ".rc") {
        if chomp(files.read("{i}/ratchets/{name}") ?? "") != "0" -> rrc = 1
    }
    rrc
}

// Goldens that cite a line of a Core or Prelude file this batch touches
// (`<corelib>/Core/X.jet:N`), each run with the batch compiler.
goldens: fn(i: String, t: String, g: String, self_jet: String, id: String, touched: [String]) Never! -[Env, Exec, FS, IO, Time.Wait]> {
    list: "{i}/goldens.list"
    put(list, "")
    loop f in touched {
        if !regex.is_match(CORE_SOURCE, f) -> next
        search: process.cmd([String]{"git", "grep", "-l", "--no-index", "-F", "<corelib>/{f}:", "--", "*.out"}).cwd(t)
        found: if files.is_dir(t) -> quiet_output(search) else -> ""
        append(list, found.bytes())
    }
    sorted: unique(lines_of(list))
    put(list, if sorted.len() == 0 -> "" else -> "{sorted.join("\n")}\n")
    loop golden in sorted {
        if golden == "" -> next
        stem: golden.removesuffix(".out").removesuffix(".err")
        argv: [String]{"bash", "{g}/capped.sh", "24G", "nice", "-n", "19", "jet", "test", "Compiler", "--profile", "golden", stem.copy()}
        spec: process.cmd(argv).cwd(t).env("PATH", "{i}/bin:{env_or("PATH", "")}").env("JET", self_jet).env("JET_COMPILER_IDENTITY", id)
            .env("JET_TOOLCHAIN_ROOT", t).env("CAP_CPUS", "16-31").timeout(time.duration_seconds(1800))
        rc: if files.is_dir(t) -> into_merged(spec, "{i}/goldens.{stem.replace("/", "_")}.out") else -> 1
        append("{i}/goldens.rc", "{rc}\t{stem}\n".bytes())
    }
}

// The FAIL rows of a test log without their durations, sorted once each.
fail_rows: fn(path: String) -[FS]> [String] Never! {
    rows := [String]{}
    loop line in lines_of(path) {
        if line.contains("FAIL") -> rows.push(regex.replace(DURATION, line, ""))
    }
    unique(rows)
}

// One SUB test line's report entry; whether it counts against the SUB.
test_entry: fn(out: &[U8], i: String, n: String, k: Int, rc: String, line: String) -[FS]> Bool Never! {
    if rc == "0" {
        add(out, "- PASS `{line}`\n")
        return false
    }
    o: "{i}/tests/{n}.{k}.out"
    bo: "{i}/tests/{n}.{k}.base.out"
    base_rc: chomp(files.read("{i}/tests/{n}.{k}.base.rc") ?? "")
    if files.is_file(bo) && base_rc != "0" {
        ours: fail_rows(o)
        theirs: fail_rows(bo)
        fresh := [String]{}
        loop row in ours {
            if !theirs.contains(row) -> fresh.push(row.copy())
        }
        if fresh.len() == 0 && ours.len() > 0 {
            add(out, "- PRE-EXISTING rc={rc} `{line}`: every FAIL row also fails on origin/dev with the seed ({bo})\n")
            return false
        }
        add(out, "- FAIL rc={rc} `{line}` (full output {o}; dev with the seed also fails, rc={base_rc}, {bo}). FAIL rows not on dev:\n```\n")
        if {
            fresh.len() > 0 -> add_cut(out, head(fresh, 25), 300)
            else -> {
                lines: lines_of(o)
                add_cut(out, head(holding(lines, [String]{"error", "Error", "panic", "failed"}), 25), 300)
                add_cut(out, tail(lines, 5), 300)
            }
        }
        add(out, "```\n")
        return true
    }
    add(out, "- FAIL rc={rc} `{line}` (full output {o}; passes on origin/dev with the seed or was not rerun):\n```\n")
    lines: lines_of(o)
    add_cut(out, head(holding(lines, [String]{"FAIL", "error", "Error", "panic", "failed"}), 25), 300)
    add_cut(out, tail(lines, 5), 300)
    add(out, "```\n")
    true
}

// The census rows that name one of `paths` (`grep -F`), sorted once each.
rows_naming: fn(rows: [String], paths: [String]) -> [String] Never! {
    hits := [String]{}
    loop path in paths {
        if path == "" -> next
        loop row in rows {
            if row.contains(path) -> hits.push(row.copy())
        }
    }
    unique(hits)
}

// The report body after the header: the summary, then one section per SUB.
report: fn(i: String, subs: [String], applied: [String], census: CensusResult) -[FS, IO]> [U8] Never! {
    crc: census.crc
    built: census.built
    out := [U8]{}
    rows: records("{chomp(cat_matching("{i}/census", "keep-going.tsv"))}\n")
    add(out, "Census rc={crc} (built compiler: {if built -> "yes" else -> "no"}). Summary:\n```\n")
    loop line in holding(lines_of("{i}/census.out"), [String]{" rc=", "rows="}) -> add(out, "{line}\n")
    checks: matching("{i}/ratchets", "", ".rc")
    loop name in checks -> add(out, "ratchet {name.removesuffix(".rc")} rc={chomp(files.read("{i}/ratchets/{name}") ?? "")}\n")
    if files.is_file("{i}/goldens.rc") {
        loop line in lines_of("{i}/goldens.rc") {
            fields: line.split("\t").to_list()
            add(out, "golden {if fields.len() > 1 -> fields[1].copy() else -> ""} rc={fields[0]}\n")
        }
    }
    add(out, "```\n")
    loop name in checks {
        if chomp(files.read("{i}/ratchets/{name}") ?? "") == "0" -> next
        check: name.removesuffix(".rc")
        add(out, "Ratchet {check} failed:\n```\n")
        add_cut(out, tail(lines_of("{i}/ratchets/{check}.out"), 30), 300)
        add(out, "```\n")
    }
    add(out, "\n")
    base: chomp(files.read("{i}/BASE") ?? "")
    loop sub in subs {
        n: files.basename(sub.removesuffix("/"))
        add(out, "## {n}\n")
        if files.is_file("{i}/{n}.status") {
            add(out, "Does not apply to origin/dev {base}:\n```\n")
            add_cut(out, head(lines_of("{i}/{n}.apply"), 20), 300)
            add(out, "```\n\n")
            next
        }
        bad := false
        hits := [U8]{}
        add_cut(hits, rows_naming(rows, read_lines(files.read("{i}/{n}.files") ?? "")), 400)
        loop hits.len() > 0 && hits[hits.len() - 1] == (U8{10}) -> _ := hits.pop()
        if hits.len() > 0 {
            bad = true
            add(out, "Census rows in files this patch touches:\n```\n")
            out.extend(hits)
            add(out, "\n```\n")
        }
        if {
            files.is_file("{i}/tests/{n}.rc") -> {
                k := 0
                loop entry in read_lines(files.read("{i}/tests/{n}.rc") ?? "") {
                    k += 1
                    parts: trim_tabs(entry).partition("\t")
                    if test_entry(out, i, n, k, parts.head, trim_tabs(parts.tail)) -> bad = true
                }
            }
            !built -> {
                bad = true
                add(out, "Tests not run: the composed tree did not build (see census).\n")
            }
            else -> add(out, "No tests listed.\n")
        }
        if {
            !bad && crc == 0 -> {
                add(out, "\n**OK**\n\n")
                put("{i}/{n}.status", "OK\n")
            }
            else -> {
                add(out, "\n**NOT OK**\n\n")
                put("{i}/{n}.status", "BAD\n")
            }
        }
    }
    if crc != 0 {
        add(out, "## Census rows not in any submitted file\n```\n")
        named := ""
        loop s in applied -> named = "{named}{files.read("{i}/{files.basename(s)}.files") ?? ""}"
        paths := [String]{}
        loop line in records("{chomp(named)}\n") {
            if words(line).len() > 0 -> paths.push(line.copy())
        }
        kept := [String]{}
        loop row in rows {
            probe: row.copy()
            if !paths.any(path -> probe.contains(path)) -> kept.push(row.copy())
        }
        add_cut(out, head(kept, 40), 300)
        add(out, "```\n")
    }
    out
}

run: fn() {
    args: process.args()
    if args.len() < 1 || args[0] == "" -> quit("integrate: 1: batch", 1)
    batch: args[0].copy()
    subs: args[1..<args.len()]
    m: "{scratch_root()}/lanes/Main"
    w: root("JET_DEV_WORKTREE", files.dirname(files.dirname(files.dirname(sys.executable()))))
    g: "{state_root()}/scratch/Main"
    o: env_or("JET_ORCHESTRATOR_DIR", files.dirname(sys.executable()))
    i: "{m}/integ/{batch}"
    t: "{i}/tree"
    files.remove_all(i) ?? ()
    made: [String]{t, "{i}/tests", "{i}/bin"}
    loop dir in made -> files.create_dir_all(dir) ?? term.eprint("mkdir: {dir}: {err}")
    if !exported(w, i, t) -> stop("integrate: export failed", 2)
    report_path: "{i}/REPORT.md"
    put(report_path, "# Integration {batch} on origin/dev {chomp(files.read("{i}/BASE") ?? "")}\n\n")
    pre_apply(i, t)
    // Apply (three-way, generated files excluded and regenerated): integ-apply6.sh prints the applied SUBs last.
    apply_argv := [String]{"bash", "{o}/integ-apply6.sh", t, i}
    apply_argv.extend(subs.copy())
    if into_merged(process.cmd(apply_argv), "{i}/apply.out") != 0 {
        last := ""
        loop line in lines_of("{i}/apply.out") {
            if line.starts_with("integ-apply:") -> last = line.copy()
        }
        stop("integrate: the apply step failed: {last}", 2)
    }
    apply_lines: lines_of("{i}/apply.out")
    shown := [U8]{}
    applied := [String]{}
    marked := false
    loop line in apply_lines {
        if {
            marked -> { if words(line).len() > 0 -> applied.push(line.copy()) }
            line == "--applied--" -> marked = true
            else -> add(shown, "{line}\n")
        }
    }
    if shown.len() > 0 -> emit(shown)
    regen_check(i, report_path, apply_lines)
    // The census: every file any applied SUB touches selects the Jetpack pass when it is under Jetpack/.
    named := ""
    loop s in applied -> named = "{named}{files.read("{i}/{files.basename(s)}.files") ?? ""}"
    touched: unique(records(named))
    say("census start")
    census_argv := [String]{"bash", "{g}/capped.sh", "32G", "bash", "{o}/census-snap2.sh"}
    census_argv.extend(touched.copy())
    census: process.cmd(census_argv).env("CENSUS_TREE", t).env("CENSUS_OUT", "{i}/census").env("CAP_CPUS", "16-31")
    crc := into_merged(census, "{i}/census.out")
    say("census rc={crc}")
    rrc: ratchet_checks(i, t)
    say("ratchets rc={rrc}")
    if rrc != 0 -> crc = 1
    self_jet: "{t}/Compiler/JetCli/.jet/build/jet"
    seed: readlink_f(seed_dir())
    id: chomp(files.read("{seed}/IDENTITY") ?? "")
    built: files.can_execute(self_jet)
    if built -> _ := link_sfn(self_jet, "{i}/bin/jet")
    // The engine `jet` talks to must be the seed's jetpack, whose identity JET_COMPILER_IDENTITY names.
    if files.can_execute("{seed}/jetpack") -> _ := link_sfn("{seed}/jetpack", "{i}/bin/jetpack")
    // The engine beside the batch compiler must be the jetpack this tree builds: the census's hosted bootstrap
    // names it as hosted-binary.
    hb := ""
    loop line in lines_of("{i}/census/jetpack.log") {
        if line.starts_with("hosted-binary: ") {
            fields: words(line)
            hb = if fields.len() > 1 -> fields[1].copy() else -> ""
        }
    }
    if {
        hb != "" && files.can_execute(hb) -> {
            built_pack: "{i}/bin/jetpack.built"
            if copy_file(hb, built_pack) && link_sfn(built_pack, "{i}/bin/jetpack") -> _ := link_sfn(built_pack, "{files.dirname(readlink_f(self_jet))}/jetpack")
            say("tests use the built jetpack {hb}")
        }
        else -> say("no built jetpack (census jetpack row failed); tests use the seed's")
    }
    vars := [Var]{Var{name: "I", value: i.copy()}, Var{name: "T", value: t.copy()}, Var{name: "SELF", value: self_jet.copy()}, Var{name: "ID", value: id.copy()}, Var{name: "G", value: g.copy()}}
    // Every SUB runs its test lines as one job, four SUBs at a time.
    if built {
        listed := [String]{}
        loop s in applied {
            if nonempty("{s}/tests") -> listed.push(s.copy())
        }
        runsubs(g, "new", listed, vars)
    }
    // A failing line reruns on a clean origin/dev export with the seed, so rows that already fail on dev are told
    // apart from the ones this batch introduces (a FAIL row present in both counts as pre-existing).
    failing := [String]{}
    loop name in matching("{i}/tests", "", ".rc") {
        if lines_of("{i}/tests/{name}").any(line -> line.partition("\t").head != "0") -> failing.push(name.removesuffix(".rc"))
    }
    if built && failing.len() > 0 {
        b: "{i}/basetree"
        made_base: [String]{b, "{i}/basebin"}
        loop dir in made_base -> files.create_dir_all(dir) ?? term.eprint("mkdir: {dir}: {err}")
        base_export(w, i, b)
        _ := link_sfn("{seed}/jet", "{i}/basebin/jet")
        _ := link_sfn("{seed}/jetpack", "{i}/basebin/jetpack")
        base_vars := vars.copy()
        base_vars.push(Var{name: "B", value: b.copy()})
        base_vars.push(Var{name: "C0", value: seed.copy()})
        runsubs(g, "base", failing, base_vars)
        files.remove_all(b) ?? ()
    }
    if built -> goldens(i, t, g, self_jet, id, touched)
    append(report_path, report(i, subs, applied, CensusResult{crc: crc, built: built}))
    say(report_path)
}
```

### jetc-trace

The compiler trace summarizer summarize.mjs and its Jet twin Summarize.jet (#5072 c21, c22, c24): read a JET_TRACE_FILE trace one event per line (an unterminated array whose last line may be half written), print one row per span kind with its count, total and longest seconds, items and peak MB, then the trace time and the spans still open with their progress counter and the time left at the current rate. compile.json is the first 800 lines of a real compile's trace, cut mid-run, without its trace.start line (whose 'last event N s ago' depends on the clock); progress.json holds counter restarts, an index/total span, a stray end, a category kind, a half-written last line and durations whose seconds sit on a rounding edge (2.675 s prints 2.67, 14.45 s prints 14.4).

- Original: `Tools/perf/jetc-trace/summarize.mjs` (JavaScript (node), 167 lines)
- Jet twin: `Tools/perf/jetc-trace/Summarize.jet` (173 lines)
- Fixture cases: 2 (args per case: `compile.json`; `progress.json`)

| Bar | Result | Measurement |
|---|---|---|
| parity | PASS | 2/2 cases identical |
| speed | PASS | wall Jet/original 0.31 (25.6 ms against 81.5 ms; every Jet round below every original round); peak memory Jet/original 0.25 (12.2 MB against 47.9 MB; every Jet round below every original round); medians of 5 interleaved rounds |
| tokens | FAIL | Jet 1579, original 1396 (+183, Jet/original 1.13) |
| reasoning | FAIL | no review record `Tools/gauntlet/entries/jetc-trace/review.json`; branches Jet 66, original 54 (+12); control nesting Jet 3, original 4 (-1) |
| verdict | FAIL | loses tokens, reasoning; the original keeps running |

#### Original: Tools/perf/jetc-trace/summarize.mjs

```js
#!/usr/bin/env node
// Summarize a compiler trace written with JET_TRACE_FILE=<path> (jetc or the
// Rust `jet`; JET_TRACE_DETAIL=functions adds per-function spans).
//
//   node Tools/perf/jetc-trace/summarize.mjs <trace.json> [--watch[=seconds]]
//
// Prints one row per span kind (count, total and longest wall time, items,
// peak RSS at its ends) and the spans still open with the latest progress
// counter, so a live, still-growing (unterminated) trace shows the current
// phase and how far along it is. --watch re-reads the file every 2 s (or the
// given seconds). The same file also loads in https://ui.perfetto.dev.

import { readFileSync } from "node:fs";

const args = process.argv.slice(2);
const path = args.find((arg) => !arg.startsWith("--"));
const watchArg = args.find((arg) => arg === "--watch" || arg.startsWith("--watch="));
if (!path) {
  console.error("usage: summarize.mjs <trace.json> [--watch[=seconds]]");
  process.exit(64);
}
const watchSeconds = watchArg ? Number(watchArg.split("=")[1] ?? 2) || 2 : 0;

// One event per line; the array may be unterminated and its last line may be
// half written by a running compiler.
function readEvents(file) {
  const events = [];
  for (const raw of readFileSync(file, "utf8").split("\n")) {
    const line = raw.trim().replace(/,$/, "");
    if (line === "" || line === "[" || line === "]") continue;
    try {
      events.push(JSON.parse(line));
    } catch {
      // A partial last line: the next read sees it whole.
    }
  }
  return events;
}

function summarize(events) {
  const kinds = new Map();
  const stacks = new Map();
  const progress = new Map();
  let startUnixMs = null;
  let lastTs = 0;
  let rssKb = null;
  let peakKb = null;
  const kindRow = (kind, ts) => {
    let row = kinds.get(kind);
    if (!row) {
      row = { kind, first: ts, count: 0, totalUs: 0, maxUs: 0, items: 0, hasItems: false, peakKb: 0 };
      kinds.set(kind, row);
    }
    return row;
  };
  for (const event of events) {
    if (typeof event.ts === "number") lastTs = Math.max(lastTs, event.ts);
    if (event.name === "trace.start") startUnixMs = event.args?.unix_ms ?? null;
    const stack = stacks.get(event.tid) ?? [];
    stacks.set(event.tid, stack);
    if (event.ph === "B") {
      stack.push({ kind: event.cat ?? event.name, label: event.name, ts: event.ts, args: event.args ?? {} });
      kindRow(event.cat ?? event.name, event.ts);
    } else if (event.ph === "E") {
      const open = stack.pop();
      if (!open) continue;
      const row = kindRow(open.kind, open.ts);
      const duration = event.ts - open.ts;
      row.count += 1;
      row.totalUs += duration;
      row.maxUs = Math.max(row.maxUs, duration);
      if (typeof event.args?.items === "number") {
        row.items += event.args.items;
        row.hasItems = true;
      }
      if (typeof event.args?.peak_rss_kb === "number") {
        row.peakKb = Math.max(row.peakKb, event.args.peak_rss_kb);
        peakKb = Math.max(peakKb ?? 0, event.args.peak_rss_kb);
        rssKb = event.args.rss_kb;
      }
    } else if (event.ph === "C" && event.name !== "memory") {
      const previous = progress.get(event.name);
      const done = event.args?.done ?? 0;
      // A restart (done fell) begins a new rate window.
      const first = previous && done >= previous.done ? previous.first : { done, ts: event.ts };
      progress.set(event.name, { done, total: event.args?.total ?? 0, ts: event.ts, first });
    }
  }
  // Spans still open count with their running time so far.
  for (const stack of stacks.values()) {
    for (const open of stack) {
      const row = kindRow(open.kind, open.ts);
      row.open = (row.open ?? 0) + 1;
      row.totalUs += lastTs - open.ts;
      row.maxUs = Math.max(row.maxUs, lastTs - open.ts);
    }
  }
  return { kinds: [...kinds.values()], stacks, progress, startUnixMs, lastTs, rssKb, peakKb };
}

const seconds = (us) => (us / 1e6).toFixed(us >= 1e8 ? 0 : us >= 1e7 ? 1 : 2);
const pad = (text, width) => String(text).padEnd(width);
const lpad = (text, width) => String(text).padStart(width);
const mb = (kb) => (kb == null ? "-" : `${Math.round(kb / 1024)}`);

function render(file) {
  const summary = summarize(readEvents(file));
  const lines = [];
  const width = Math.max(14, ...summary.kinds.map((row) => row.kind.length));
  lines.push(`${pad("span", width)} ${lpad("count", 7)} ${lpad("total s", 9)} ${lpad("max s", 8)} ${lpad("items", 9)} ${lpad("peak MB", 8)}`);
  for (const row of summary.kinds.sort((left, right) => left.first - right.first)) {
    // `+N`: N spans of this kind are still open; their time so far is included.
    const count = row.open ? `${row.count}+${row.open}` : row.count;
    lines.push(
      `${pad(row.kind, width)} ${lpad(count, 7)} ${lpad(seconds(row.totalUs), 9)} ${lpad(seconds(row.maxUs), 8)} ${lpad(row.hasItems ? row.items : "-", 9)} ${lpad(row.peakKb ? mb(row.peakKb) : "-", 8)}`,
    );
  }
  lines.push("");
  const stale = summary.startUnixMs == null ? null : (Date.now() - (summary.startUnixMs + summary.lastTs / 1000)) / 1000;
  lines.push(
    `trace time ${seconds(summary.lastTs)} s` +
      (stale == null ? "" : `, last event ${stale.toFixed(0)} s ago`) +
      `, rss ${mb(summary.rssKb)} MB, peak ${mb(summary.peakKb)} MB`,
  );
  let anyOpen = false;
  for (const [tid, stack] of summary.stacks) {
    if (stack.length === 0) continue;
    anyOpen = true;
    lines.push(`open on thread ${tid}:`);
    for (const [depth, span] of stack.entries()) {
      const place = span.args.index >= 0 ? ` [${span.args.index + 1}/${span.args.total}]` : "";
      const label = span.label === span.kind ? span.kind : `${span.kind} ${span.label}`;
      let line = `${"  ".repeat(depth + 1)}${label}${place} running ${seconds(summary.lastTs - span.ts)} s`;
      const counter = summary.progress.get(span.kind);
      if (counter && counter.ts >= span.ts && counter.total > 0) {
        const percent = ((100 * counter.done) / counter.total).toFixed(1);
        line += `, ${counter.done}/${counter.total} (${percent}%)`;
        const doneSince = counter.done - counter.first.done;
        const elapsed = counter.ts - counter.first.ts;
        if (doneSince > 0 && elapsed > 0) {
          const remainingUs = ((counter.total - counter.done) * elapsed) / doneSince;
          line += `, ~${seconds(remainingUs)} s left at the current rate`;
        }
      }
      lines.push(line);
    }
  }
  if (!anyOpen) lines.push("no open spans (compile finished or not started)");
  return lines.join("\n");
}

if (watchSeconds > 0) {
  const tick = () => {
    let text;
    try {
      text = render(path);
    } catch (error) {
      text = `cannot read ${path}: ${error.message}`;
    }
    process.stdout.write(`\x1b[2J\x1b[H${new Date().toLocaleTimeString()}  ${path}\n\n${text}\n`);
  };
  tick();
  setInterval(tick, watchSeconds * 1000);
} else {
  console.log(render(path));
}
```

#### Jet twin: Tools/perf/jetc-trace/Summarize.jet

```jet
#!/usr/bin/env jet
// Summarize a compiler trace written with JET_TRACE_FILE=<path> (jetc or the
// Rust `jet`; JET_TRACE_DETAIL=functions adds per-function spans).
//
//   jet Tools/perf/jetc-trace/Summarize.jet <trace.json> [--watch[=seconds]]
//
// Prints one row per span kind (count, total and longest wall time, items,
// peak RSS at its ends) and the spans still open with the latest progress
// counter, so a live, still-growing (unterminated) trace shows the current
// phase and how far along it is. --watch re-reads the file every 2 s (or the
// given seconds). The same file also loads in https://ui.perfetto.dev.

use {
    json: core.encoding.json
    files: core.files
    math: core.math
    process: core.process
    sys: core.sys
    term: core.term
    time: core.time
}

Args: struct { unix_ms: Int?, items: Int?, peak_rss_kb: Int?, rss_kb: Int?, done: Int?, total: Int?, index: Int? }
Event: struct { name: String, ph: String, cat: String?, ts: Int?, tid: Int, args: Args? }
Span: struct { kind: String, label: String, ts: Int, args: Args? }
Row: struct { kind: String, first: Int, count: Int{0}, open: Int{0}, total: Int{0}, max: Int{0}, items: Int?{None}, peak: Int{0} }
Counter: struct { done: Int, total: Int, ts: Int, first_done: Int, first_ts: Int }
Thread: struct { tid: Int, spans: [Span]{} }

seconds: fn(us: Float) -> String {
    x: us / 1e6
    if us >= 1e8 -> "{x:Fixed(0)}" else if us >= 1e7 -> "{x:Fixed(1)}" else -> "{x:Fixed(2)}"
}
mb: fn(kb: Int?) -> String { kb.map(k -> "{(k + 512) /% 1024}") ?? "-" }
row: fn(rows: &[Row], at: &[String: Int], kind: String, ts: Int) -> Int {
    at.get(kind) ?? {
        at[kind] = rows.len()
        rows.push(Row{kind, first: ts})
        rows.len() - 1
    }
}

render: fn(file: String) -> String IOError! {
    rows := [Row]{}
    at := [String: Int]{}
    threads := [Thread]{}
    thread_at := [Int: Int]{}
    progress := [String: Counter]{}
    start := Int?{None}
    last := 0
    rss := Int?{None}
    peak := Int?{None}
    // One event per line; the array may be unterminated and its last line may be
    // half written by a running compiler.
    loop raw in files.read(file).split("\n") {
        line: raw.trim().remove_suffix(",")
        if line == "" || line == "[" || line == "]" -> next
        // A partial last line: the next read sees it whole.
        event: json.decode<Event>(line) ?? next
        ts: event.ts ?? 0
        if event.ts != None -> last = max(last, ts)
        if event.name == "trace.start" -> start = event.args?.unix_ms
        t: thread_at.get(event.tid) ?? {
            thread_at[event.tid] = threads.len()
            threads.push(Thread{tid: event.tid})
            threads.len() - 1
        }
        if event.ph == "B" {
            threads[t].spans.push(Span{kind: event.cat ?? event.name, label: event.name, ts, args: event.args})
            _: row(rows, at, event.cat ?? event.name, ts)
        } else if event.ph == "E" {
            open: threads[t].spans.pop() ?? next
            r: row(rows, at, open.kind, open.ts)
            duration: ts - open.ts
            rows[r].count += 1
            rows[r].total += duration
            rows[r].max = max(rows[r].max, duration)
            if event.args?.items == .Val(items) -> rows[r].items = Val((rows[r].items ?? 0) + items)
            if event.args?.peak_rss_kb == .Val(kb) {
                rows[r].peak = max(rows[r].peak, kb)
                peak = Val(max(peak ?? 0, kb))
                rss = event.args?.rss_kb
            }
        } else if event.ph == "C" && event.name != "memory" {
            previous: progress.get(event.name)
            done: event.args?.done ?? 0
            // A restart (done fell) begins a new rate window.
            keep: previous != None && done >= (previous?.done ?? 0)
            progress[event.name] = Counter{done, total: event.args?.total ?? 0, ts, first_done: if keep -> previous?.first_done ?? 0 else -> done, first_ts: if keep -> previous?.first_ts ?? 0 else -> ts}
        }
    }
    // Spans still open count with their running time so far.
    loop thread in threads {
        loop open in thread.spans {
            r: row(rows, at, open.kind, open.ts)
            rows[r].open += 1
            rows[r].total += last - open.ts
            rows[r].max = max(rows[r].max, last - open.ts)
        }
    }
    width: max(14, rows.map(r -> r.kind.len()).max() ?? 0)
    lines := ["{"span".pad_end(width, " ")} {"count":PadLeft(7)} {"total s":PadLeft(9)} {"max s":PadLeft(8)} {"items":PadLeft(9)} {"peak MB":PadLeft(8)}"]
    rows.sort_by(r -> r.first)
    loop r in rows {
        // `+N`: N spans of this kind are still open; their time so far is included.
        count: if r.open > 0 -> "{r.count}+{r.open}" else -> "{r.count}"
        items: r.items.map(n -> "{n}") ?? "-"
        kb: if r.peak > 0 -> mb(Val(r.peak)) else -> "-"
        lines.push("{r.kind.pad_end(width, " ")} {count:PadLeft(7)} {seconds(Float.from_int(r.total)):PadLeft(9)} {seconds(Float.from_int(r.max)):PadLeft(8)} {items:PadLeft(9)} {kb:PadLeft(8)}")
    }
    lines.push("")
    stale: start.map(s -> ", last event {(Float.from_int(time.now().to_unix_ms() - s) - Float.from_int(last) / 1000.0) / 1000.0:Fixed(0)} s ago") ?? ""
    lines.push("trace time {seconds(Float.from_int(last))} s{stale}, rss {mb(rss)} MB, peak {mb(peak)} MB")
    any_open := false
    loop thread in threads {
        if thread.spans.len() == 0 -> next
        any_open = true
        lines.push("open on thread {thread.tid}:")
        loop depth in 0..<thread.spans.len() {
            span: thread.spans[depth]
            index: span.args?.index ?? -1
            place: if index >= 0 -> " [{index + 1}/{span.args?.total ?? 0}]" else -> ""
            label: if span.label == span.kind -> span.kind else -> "{span.kind} {span.label}"
            line := "{"  ".repeat(depth + 1)}{label}{place} running {seconds(Float.from_int(last - span.ts))} s"
            if progress.get(span.kind) == .Val(c) && c.ts >= span.ts && c.total > 0 {
                line = "{line}, {c.done}/{c.total} ({Float{100.0} * Float.from_int(c.done) / Float.from_int(c.total):Fixed(1)}%)"
                since: c.done - c.first_done
                elapsed: c.ts - c.first_ts
                if since > 0 && elapsed > 0 -> line = "{line}, ~{seconds(Float.from_int(c.total - c.done) * Float.from_int(elapsed) / Float.from_int(since))} s left at the current rate"
            }
            lines.push(line)
        }
    }
    if !any_open -> lines.push("no open spans (compile finished or not started)")
    lines.join("\n")
}

two: fn(value: Int) -> String { if value < 10 -> "0{value}" else -> "{value}" }

// Date.prototype.toLocaleTimeString() in en-US for the host's zone: TZ, else
// the zone /etc/localtime links to, else UTC.
local_clock: fn() -[Env, FS.Read, Time]> String {
    name := (sys.get("TZ") ?? "").remove_prefix(":")
    if name == "" -> name = (files.read_link("/etc/localtime") ?? "").split_once("zoneinfo/")?.after ?? "UTC"
    local: time.now().in_zone(time.zone(name) ?? time.utc()).time()
    hour: local.hour()
    "{if hour % 12 == 0 -> 12 else -> hour % 12}:{two(local.minute())}:{two(local.second())} {if hour < 12 -> "AM" else -> "PM"}"
}

run: fn() {
    args: process.args()
    path: args.find(a -> !a.starts_with("--")) ?? {
        term.eprint("usage: Summarize.jet <trace.json> [--watch[=seconds]]")
        process.exit(64)
        return
    }
    watch: args.find(a -> a == "--watch" || a.starts_with("--watch="))
    every: watch.map(w -> Float.parse(w.split_once("=")?.after ?? "") ?? 2.0) ?? 0.0
    if every > 0 {
        loop {
            text: render(path) ? out -> out ! e -> "cannot read {path}: {e}"
            esc: String.from_bytes_lossy([U8]{27})
            print("{esc}[2J{esc}[H{local_clock()}  {path}\n\n{text}")
            time.sleep(time.duration_ns(math.round(every * 1e9)))
        }
    }
    print(render(path) ?? {
        term.eprint("{err}")
        process.exit(1)
        return
    })
}
```

### lane-commit

Main's lane lander lane-commit.sh and its Jet twin LaneCommit.jet (#5072 c15): queue a lane's patch, take the commit lock, drain every queued job (drop one whose caller died, skip a held lane or a frozen path, leave a job that touches a path already in the batch for the next round), apply each clean patch, run one precommit over the union of paths, commit each job with its own message, and on a failed batch precommit revert and land each job alone. The fixtures hold a repository whose gitdir is repo/ and whose worktree is repo/work (with one uncommitted file and a commit-msg hook that refuses REFUSE), a stand-in precommit (pre.sh) that fails on a bad path, the patches and messages, and run roots whose queues hold another lane's job (live caller pid 0, a dead one above pid_max), a hold switch, a live path freeze and an expired one. Commit dates are fixed, so each landed sha is the same on both sides: the batch and next-round cases land on top of lane B's commit, the fallback, dead-caller, hold and freeze cases land alone.

- Original: `Tools/agent/orchestrator/lane-commit.sh` (shell (bash), 173 lines)
- Jet twin: `Tools/agent/orchestrator/LaneCommit.jet` (501 lines)
- Fixture cases: 16 (args per case: `A msg/a.txt patches/a.patch`; `A msg/rename.txt patches/rename.patch`; `A msg/a.txt patches/garbage.patch` exit 2; `A msg/none.txt patches/a.patch` exit 2; `A msg/a.txt patches/dirty.patch` exit 3; `A msg/a.txt patches/noapply.patch` exit 4; `A msg/a.txt patches/bad.patch` exit 5; `A msg/refuse.txt patches/a.patch` exit 6; `A msg/a.txt patches/a.patch`; `A msg/a.txt patches/a.patch`; `A msg/a.txt patches/a.patch`; `A msg/a.txt patches/a.patch`; `A msg/a.txt patches/a.patch`; `A msg/a.txt patches/a.patch`; `A msg/a.txt patches/a.patch`; `(none)` exit 1)

| Bar | Result | Measurement |
|---|---|---|
| parity | PASS | 16/16 cases identical |
| speed | PASS | wall Jet/original 0.41 (806.0 ms against 1970.0 ms; every Jet round below every original round); peak memory Jet/original 0.31 (14.4 MB against 45.8 MB; every Jet round below every original round); medians of 5 interleaved rounds |
| tokens | FAIL | Jet 3801, original 1133 (+2668, Jet/original 3.35) |
| reasoning | FAIL | no review record `Tools/gauntlet/entries/lane-commit/review.json`; branches Jet 130, original 57 (+73); control nesting Jet 2, original 3 (-1) |
| verdict | FAIL | loses tokens, reasoning; the original keeps running |

#### Original: Tools/agent/orchestrator/lane-commit.sh

```sh
#!/usr/bin/env bash
# lane-commit.sh <lane> <message file> <patch file>
# The ONLY way lanes put changes onto dev. Lanes develop in their own export (outside the repo), never
# in the shared worktree. To commit: make a patch of your changes (repo-relative paths with a/ b/
# prefixes; adds, deletes and renames are all fine), then run this script.
#
# Batched landing: the call queues the patch under $JET_RUN_ROOT/commit-queue and waits. Whoever holds
# the machine-wide commit lock drains every queued job at once: it applies each patch that applies
# cleanly and touches no path another job in the batch touches, runs ONE precommit over the union of
# touched paths, and on success commits each job separately with its own message. If the batch
# precommit fails, it reverts everything and lands the jobs one at a time, so a bad patch only fails
# itself.
# Prints `lane-commit: <sha> by <lane>` on success; exit codes: 2 bad patch, 3 dirty path, 4 does not
# apply, 5 precommit failed, 6 commit failed, 7 the lander was interrupted.
# Never edit this file in place while it runs: write a new file and mv it over.
#
# The original of gauntlet tool entry lane-commit; LaneCommit.jet beside it is its Jet twin. The
# worktree is $LANE_COMMIT_WORKTREE, else $JET_DEV_WORKTREE; the queue, lock, hold switch and path
# freeze are in $JET_RUN_ROOT; the precommit is Main's precommit.sh, in $JET_STATE_ROOT/scratch/Main
# (Tools/agent/roots.sh). A relative root names a path in the caller's directory.
set -uo pipefail
unset GIT_INDEX_FILE GIT_DIR GIT_WORK_TREE
LANE=${1:?lane} MSG=${2:?message file} PATCH=${3:?patch file}
. "$(dirname "$0")/../roots.sh"
# A root as an absolute path: the drain works in the worktree.
abs() { case $1 in /*) printf '%s' "$1";; *) printf '%s/%s' "$PWD" "$1";; esac; }
W=$(abs "${LANE_COMMIT_WORKTREE:-$JET_DEV_WORKTREE}")
Q=$(abs "${LANE_COMMIT_QUEUE:-$JET_RUN_ROOT/commit-queue}")
PRE=$(abs "${LANE_COMMIT_PRECOMMIT:-$JET_STATE_ROOT/scratch/Main/precommit.sh}")
LOCK=$(abs "${LANE_COMMIT_LOCK:-$JET_RUN_ROOT/commit.lock}")
HOLD=$(abs "$JET_RUN_ROOT/commit-hold")
FREEZE=$(abs "$JET_RUN_ROOT/path-freeze")
mkdir -p "$Q"
# Hold switch: while $HOLD exists, only the lanes it lists (one per line) land.
while [ -f "$HOLD" ] && ! grep -qxF "$LANE" "$HOLD"; do sleep 20; done

job="$Q/$(date +%s%N)-$$"
mkdir "$job" && cp "$MSG" "$job/msg" && cp "$PATCH" "$job/patch" && printf '%s\n' "$LANE" > "$job/lane" && printf '%s\n' "$$" > "$job/pid" && touch "$job/ready" || { echo "lane-commit: cannot queue the job"; exit 2; }

# Every path a patch touches, one per line, each ending in a newline (a last line without one is skipped
# by `while read`); a rename contributes both its old and its new path.
files_of() {
  git apply --numstat -z "$1" 2>/dev/null | node -e '
    const b = require("fs").readFileSync(0); const parts = b.toString("utf8").split("\0"); const out = [];
    for (let i = 0; i < parts.length; i++) {
      const p = parts[i]; if (!p) continue;
      const m = p.match(/^(\S+)\t(\S+)\t(.*)$/s);
      if (!m) continue;
      if (m[3] === "") { out.push(parts[i + 1], parts[i + 2]); i += 2; } else out.push(m[3]);
    }
    for (const line of require("fs").readFileSync(process.argv[1], "utf8").split("\n")) { const r = line.match(/^rename from (.+)$/); if (r) out.push(r[1]); }
    const paths = [...new Set(out.filter(Boolean))];
    process.stdout.write(paths.length ? paths.join("\n") + "\n" : "");' "$1"
}
finish() { # job rc text
  rm -f "$1/applied"
  printf '%s\n' "$3" > "$1/out"; printf '%s\n' "$2" > "$1/rc"; touch "$1/result"
}
revert_job() { # job
  local j=$1 f
  git apply -R "$j/patch"
  while IFS= read -r f; do [ -n "$f" ] && [ -e "$f" ] && ! git ls-files --error-unmatch -- "$f" >/dev/null 2>&1 && rm -f -- "$f"; done < "$j/files"
  rm -f "$j/applied"
}
# A lock holder killed mid-batch must not leave patches applied in the shared tree: revert every job
# still marked applied (newest first) whose paths still differ from HEAD (a committed job matches HEAD).
on_signal() {
  local j
  [ -n "$PRE_PID" ] && { pkill -TERM -P "$PRE_PID" 2>/dev/null; kill -TERM "$PRE_PID" 2>/dev/null; wait "$PRE_PID" 2>/dev/null; }
  cd "$W" || exit 143
  for j in $(ls -d "$Q"/*/ 2>/dev/null | sort -r); do
    j=${j%/}
    [ -f "$j/applied" ] || continue
    mapfile -t fl < "$j/files"
    git reset -q -- "${fl[@]}" 2>/dev/null
    if ! git diff --quiet HEAD -- "${fl[@]}" 2>/dev/null || [ -n "$(git ls-files --others --exclude-standard -- "${fl[@]}")" ]; then revert_job "$j"; fi
    finish "$j" 7 "lane-commit: lander interrupted; reverted, resubmit"
  done
  exit 143
}
# Precommit runs in the background so a signal interrupts `wait` at once (bash defers traps until a
# foreground child exits); the trap kills it before reverting.
PRE_PID=""
run_pre() {
  local log="$Q/.precommit.$$.out"
  bash "$PRE" "$@" > "$log" 2>&1 & PRE_PID=$!
  wait "$PRE_PID"; PRE_RC=$?; PRE_PID=""
  PRE_OUT=$(cat "$log"); rm -f "$log"
}
commit_job() { # job
  local j=$1 lane; lane=$(cat "$j/lane")
  mapfile -t fl < "$j/files"
  git add -A -f -- "${fl[@]}"
  if git commit -q -F "$j/msg" -- "${fl[@]}"; then
    finish "$j" 0 "lane-commit: $(git rev-parse --short HEAD) by $lane"
  else
    git reset -q -- "${fl[@]}"; revert_job "$j"; finish "$j" 6 "lane-commit: commit failed; reverted"
  fi
}
# Apply one job onto the current tree (with earlier batch jobs applied). Returns 0 if applied.
apply_job() { # job
  local j=$1 f
  files_of "$j/patch" > "$j/files"
  [ -s "$j/files" ] || { finish "$j" 2 "lane-commit: patch lists no files or does not parse"; return 1; }
  while IFS= read -r f; do
    if [ -n "$(git status --porcelain -- "$f")" ]; then finish "$j" 3 "lane-commit: $f has uncommitted changes in the shared worktree; refusing"; return 1; fi
  done < "$j/files"
  git apply --check "$j/patch" 2>/dev/null || { finish "$j" 4 "lane-commit: patch does not apply to HEAD $(git rev-parse --short HEAD); rebase your export"; return 1; }
  touch "$j/applied"
  git apply "$j/patch" || { finish "$j" 4 "lane-commit: patch failed to apply"; return 1; }
  return 0
}
# Path freeze: $FREEZE holds the end time (epoch seconds) on line 1, the exempt lane on line 2, then
# one repo-relative path prefix per line. Until the end time, a job from another lane that touches a
# frozen prefix waits in the queue (it neither fails nor lands).
frozen() { # lane filesfile
  local end exempt p f
  [ -f "$FREEZE" ] || return 1
  end=$(sed -n 1p "$FREEZE"); exempt=$(sed -n 2p "$FREEZE")
  [ -n "$end" ] && [ "$(date +%s)" -lt "$end" ] || return 1
  [ "$1" = "$exempt" ] && return 1
  while IFS= read -r p; do
    [ -n "$p" ] || continue
    while IFS= read -r f; do case "$f" in "$p"*) return 0;; esac; done < "$2"
  done < <(sed -n "3,\$p" "$FREEZE")
  return 1
}
drain() {
  cd "$W" || return
  local j lane batch=() union=() taken
  for j in $(ls -d "$Q"/*/ 2>/dev/null | sort); do
    j=${j%/}
    [ -f "$j/ready" ] && [ ! -f "$j/result" ] || continue
    # A job whose caller died is dropped, never landed.
    if ! kill -0 "$(cat "$j/pid")" 2>/dev/null; then rm -rf "$j"; continue; fi
    lane=$(cat "$j/lane")
    if [ -f "$HOLD" ] && ! grep -qxF "$lane" "$HOLD"; then continue; fi
    files_of "$j/patch" > "$j/files.peek"
    frozen "$lane" "$j/files.peek" && continue
    # A job that touches a path already in the batch waits for the next round.
    taken=0
    while IFS= read -r f; do for u in "${union[@]}"; do [ "$f" = "$u" ] && taken=1; done; done < "$j/files.peek"
    [ $taken = 1 ] && continue
    if apply_job "$j"; then
      batch+=("$j"); mapfile -t -O "${#union[@]}" union < "$j/files"
    fi
  done
  [ ${#batch[@]} -gt 0 ] || return
  local out rc
  run_pre "${union[@]}"; rc=$PRE_RC; out=$PRE_OUT
  if [ $rc = 0 ]; then
    for j in "${batch[@]}"; do commit_job "$j"; done
    return
  fi
  # Batch failed: revert all (newest first), then land each job alone so only the bad one fails.
  for ((i=${#batch[@]}-1; i>=0; i--)); do revert_job "${batch[$i]}"; done
  if [ ${#batch[@]} = 1 ]; then finish "${batch[0]}" 5 "$(printf '%s\n' "$out" | tail -20)
lane-commit: precommit failed; reverted"; return; fi
  for j in "${batch[@]}"; do
    apply_job "$j" || continue
    mapfile -t fl < "$j/files"
    run_pre "${fl[@]}"; out=$PRE_OUT
    if [ "$PRE_RC" = 0 ]; then commit_job "$j"; else revert_job "$j"; finish "$j" 5 "$(printf '%s\n' "$out" | tail -20)
lane-commit: precommit failed; reverted"; fi
  done
}

exec 9>"$LOCK"
while [ ! -f "$job/result" ]; do
  if flock -n 9; then trap on_signal TERM INT HUP; drain; trap - TERM INT HUP; flock -u 9; else sleep 5; fi
done
cat "$job/out"; rc=$(cat "$job/rc"); rm -rf "$job"; exit "$rc"
```

#### Jet twin: Tools/agent/orchestrator/LaneCommit.jet

```jet
#!/usr/bin/env jet
// LaneCommit.jet LANE MESSAGE PATCH: the ONLY way lanes put changes onto dev.
// Lanes develop in their own export (outside the repo), never in the shared
// worktree. To commit: make a patch of your changes (repo-relative paths with
// a/ b/ prefixes; adds, deletes and renames are all fine), then run this.
//
// Batched landing: the call queues the patch under $JET_RUN_ROOT/commit-queue
// and waits. Whoever holds the machine-wide commit lock drains every queued job
// at once: it applies each patch that applies cleanly and touches no path
// another job in the batch touches, runs ONE precommit over the union of
// touched paths, and on success commits each job separately with its own
// message. If the batch precommit fails, it reverts everything and lands the
// jobs one at a time, so a bad patch only fails itself. Prints
// `lane-commit: <sha> by <lane>` on success; exit codes: 2 bad patch, 3 dirty
// path, 4 does not apply, 5 precommit failed, 6 commit failed, 7 the lander
// was interrupted.
//
// The Jet twin of lane-commit.sh (gauntlet tool entry lane-commit): the
// worktree is $LANE_COMMIT_WORKTREE, else $JET_DEV_WORKTREE; the queue, lock,
// hold switch and path freeze are in $JET_RUN_ROOT; the precommit is Main's
// precommit.sh, in $JET_STATE_ROOT/scratch/Main, as Tools/agent/roots.sh sets
// them. A relative root names a path in the caller's directory. The original
// reverts a drain on SIGTERM, SIGINT and SIGHUP; Core handles only SIGINT
// (sys.on_interrupt), so this twin reverts on SIGINT alone.
use {
    files: core.files
    process: core.process
    sys: core.sys
    term: core.term
    time: core.time
}

TOOL: "/Tools/agent/orchestrator/LaneCommit.jet"

env_or: fn(name: String, fallback: String) -[Env]> String Never! {
    value: process.env_get(name) ?? return fallback.copy()
    if value == "" -> fallback.copy() else -> value
}

// What the machine's jet/env file sets `name` to (the default of its
// `${NAME:-default}` form), None when it sets nothing.
env_file_value: fn(name: String) -[Env, FS]> String? Never! {
    home: env_or("HOME", "")
    text: files.read("{env_or("XDG_CONFIG_HOME", "{home}/.config")}/jet/env") ?? return None
    defaulted: "${{{name}:-"
    found := ""
    seen := false
    loop line in text.lines() {
        parts: line.trim().removeprefix("export ").partition("=")
        if parts.sep == "" || parts.head != name -> next
        value := parts.tail.removeprefix("\"").removesuffix("\"").removeprefix("'").removesuffix("'")
        if value.starts_with(defaulted) && value.ends_with("}}") -> value = value.removeprefix(defaulted).removesuffix("}}")
        found = value.replace("$HOME", home)
        seen = true
    }
    if seen -> Val(found) else -> None
}

// A root as Tools/agent/roots.sh sets it: the environment, else the jet/env
// file, else `fallback`.
root: fn(name: String, fallback: String) -[Env, FS]> String Never! {
    found: env_or(name, "")
    if found != "" -> return found
    env_file_value(name) ?? fallback.copy()
}

state_root: fn() -[Env, FS]> String Never! { root("JET_STATE_ROOT", "{env_or("XDG_CACHE_HOME", "{env_or("HOME", "")}/.cache")}/jet-dev") }

scratch_root: fn() -[Env, FS]> String Never! { root("JET_SCRATCH_ROOT", state_root()) }

run_root: fn() -[Env, FS]> String Never! { root("JET_RUN_ROOT", "{scratch_root()}/run") }

// The checkout holding this script, as roots.sh defaults $JET_DEV_WORKTREE.
dev_worktree: fn() -[Env, FS]> String Never! {
    script: sys.executable()
    fallback: if script.ends_with(TOOL) -> script.removesuffix(TOOL) else -> files.cwd() ?? "."
    root("JET_DEV_WORKTREE", fallback)
}

// `path` as an absolute path: a relative one names a path in `cwd`.
absolute: fn(cwd: String, path: String) -> String Never! {
    if path.starts_with("/") -> path.copy() else -> "{cwd}/{path}"
}

// `text` without its trailing newlines, as `$(...)` gives it.
chomp: fn(text: String) -> String Never! {
    out := text.copy()
    loop out.ends_with("\n") -> out = out.removesuffix("\n")
    out
}

// The lines `read` or `grep` find in `text`: a last line without its newline
// counts.
records: fn(text: String) -> [String] Never! {
    out := text.split("\n").to_list()
    if out.len() > 0 && out[out.len() - 1] == "" -> _ := out.pop()
    out
}

// Writes `text` to stdout now, before any child writes to it.
emit: fn(text: String) Never! -[IO]> {
    out := term.stdout()
    out.write_bytes(text.bytes()) ?? ()
    out.flush() ?? ()
}

// Ends the run with `code` after `message` on stderr.
quit: fn(message: String, code: Int) -[Exec, IO]> Never {
    term.eprint(message)
    process.exit(code)
}

// The exit status the shell gives a finished child: 128 + N for signal N.
status: fn(receipt: ProcessReceipt) -> Int Never! {
    if receipt.signal == .Val(number) -> return 128 + number
    receipt.code
}

// Where the lander works and keeps its queue.
Lander: struct {
    worktree: String
    queue: String
    precommit: String
    hold: String
    freeze: String
}

// `argv` in the worktree, with the caller's git index and directories unset.
command: fn(l: Lander, argv: [String]) -[Exec]> ProcessSpec Never! {
    process.cmd(argv).cwd(l.worktree).env_remove("GIT_INDEX_FILE").env_remove("GIT_DIR").env_remove("GIT_WORK_TREE")
}

git: fn(l: Lander, args: [String]) -[Exec]> ProcessSpec Never! {
    argv := [String]{"git"}
    argv.extend(args.copy())
    command(l, argv)
}

// Runs `spec` on the program's streams; its exit status.
runs: fn(spec: ProcessSpec) -[Exec, IO, Time.Wait]> Int Never! {
    receipt: spec.stdin(.Inherit).stdout(.Inherit).stderr(.Inherit).run() ?? {
        term.eprint("lane-commit: {err}")
        return 127
    }
    status(receipt)
}

// Runs `spec` with its output discarded (`>/dev/null 2>&1`); its exit status.
runs_quiet: fn(spec: ProcessSpec) -[Exec, Time.Wait]> Int Never! {
    receipt: spec.stdin(.Inherit).stdout(.Capture).stderr(.Capture).output_limit(1073741824).run() ?? return 127
    status(receipt)
}

// `$(spec)`, its stderr shown, without trailing newlines.
output: fn(spec: ProcessSpec) -[Exec, IO, Time.Wait]> String Never! {
    receipt: spec.stdin(.Inherit).stdout(.Capture).stderr(.Inherit).output_limit(1073741824).run() ?? {
        term.eprint("lane-commit: {err}")
        return ""
    }
    chomp(receipt.output)
}

short_head: fn(l: Lander) -[Exec, IO, Time.Wait]> String Never! { output(git(l, [String]{"rev-parse", "--short", "HEAD"})) }

// `$(cat path)`: the file's text without trailing newlines, "" when unread.
contents: fn(path: String) -[FS]> String Never! { chomp(files.read(path) ?? "") }

// `printf '%s\n' text > path`.
put: fn(path: String, text: String) Never! -[FS, IO]> {
    files.write(path, "{text}\n") ?? term.eprint("lane-commit: {path}: {err}")
}

touch: fn(path: String) Never! -[FS, IO]> {
    if files.exists(path) -> return
    files.write(path, "") ?? term.eprint("lane-commit: {path}: {err}")
}

drop: fn(path: String) Never! -[FS]> {
    files.remove(path) ?? ()
}

// Every path a patch touches, one per line; a rename contributes both its old
// and its new path. `git apply --numstat -z` gives `added TAB deleted TAB path`
// records, a rename an empty path followed by its old and new path.
files_of: fn(l: Lander, patch: String) -[Exec, FS, Time.Wait]> [String] Never! {
    found := [String]{}
    receipt: git(l, [String]{"apply", "--numstat", "-z", patch.copy()}).stdin(.Inherit).stdout(.Capture).stderr(.Capture).output_limit(1073741824).run() ?? return found
    nul: String.from_bytes([U8]{U8{0}}) ?? ""
    parts: receipt.output.split(nul).to_list()
    at := 0
    loop at < parts.len() {
        part: parts[at]
        at += 1
        if part == "" -> next
        first: part.partition("\t")
        if first.sep == "" || first.head == "" || first.head.contains(" ") -> next
        second: first.tail.partition("\t")
        if second.sep == "" || second.head == "" || second.head.contains(" ") -> next
        if second.tail != "" {
            found.push(second.tail.copy())
            next
        }
        if at < parts.len() -> found.push(parts[at].copy())
        if at + 1 < parts.len() -> found.push(parts[at + 1].copy())
        at += 2
    }
    loop line in (files.read(patch) ?? "").split("\n") {
        if line.starts_with("rename from ") && line.count_bytes() > 12 -> found.push(line.removeprefix("rename from "))
    }
    seen := [String: Bool]{}
    out := [String]{}
    loop path in found {
        if path == "" || seen.get(path) != None -> next
        seen[path.copy()] = true
        out.push(path.copy())
    }
    out
}

// The path list as files_of writes it: one path per line.
listed: fn(paths: [String]) -> String Never! {
    if paths.len() == 0 -> "" else -> "{paths.join("\n")}\n"
}

finish: fn(job: String, code: Int, text: String) Never! -[FS, IO]> {
    drop("{job}/applied")
    put("{job}/out", text)
    put("{job}/rc", "{code}")
    touch("{job}/result")
}

revert_job: fn(l: Lander, job: String) Never! -[Exec, FS, IO, Time.Wait]> {
    _ := runs(git(l, [String]{"apply", "-R", "{job}/patch"}))
    loop path in records(files.read("{job}/files") ?? "") {
        if path == "" || !files.exists(absolute(l.worktree, path)) -> next
        if runs_quiet(git(l, [String]{"ls-files", "--error-unmatch", "--", path.copy()})) != 0 -> drop(absolute(l.worktree, path))
    }
    drop("{job}/applied")
}

// The queue's job directories, sorted (`ls -d "$Q"/*/ | sort`).
jobs_in: fn(queue: String) -[FS]> [String] Never! {
    entries: files.list_dir(queue) ?? return [String]{}
    names := [String]{}
    loop entry in entries {
        if entry.name.starts_with(".") || !files.is_dir("{queue}/{entry.name}") -> next
        names.push(entry.name.copy())
    }
    names.sort()
    loop name in names -> "{queue}/{name}"
}

// A lock holder interrupted mid-batch must not leave patches applied in the
// shared tree: revert every job still marked applied (newest first) whose
// paths still differ from HEAD (a committed job matches HEAD).
interrupted: fn(l: Lander) -[Exec, FS, IO, Time.Wait]> Never {
    if !files.is_dir(l.worktree) -> process.exit(143)
    queued := jobs_in(l.queue)
    queued.reverse()
    loop job in queued {
        if !files.exists("{job}/applied") -> next
        paths: records(files.read("{job}/files") ?? "")
        reset := [String]{"reset", "-q", "--"}
        reset.extend(paths.copy())
        _ := runs_quiet(git(l, reset))
        differs := [String]{"diff", "--quiet", "HEAD", "--"}
        differs.extend(paths.copy())
        others := [String]{"ls-files", "--others", "--exclude-standard", "--"}
        others.extend(paths.copy())
        if runs_quiet(git(l, differs)) != 0 || output(git(l, others)) != "" -> revert_job(l, job)
        finish(job, 7, "lane-commit: lander interrupted; reverted, resubmit")
    }
    process.exit(143)
}

// Precommit's exit status and its output (stdout and stderr together, without
// trailing newlines). It dies with this process when the lander is interrupted.
run_pre: fn(l: Lander, paths: [String]) -[Exec, IO, Time.Wait]> (code: Int, output: String) Never! {
    argv := [String]{"bash", l.precommit.copy()}
    argv.extend(paths.copy())
    ran: process.run_all([ProcessSpec]{command(l, argv).stdout(.Capture).stderr(.Stdout).output_limit(1073741824)}, 1) ?? {
        term.eprint("lane-commit: bash: {err}")
        return (code: 127, output: "")
    }
    receipt: ran[0] ?? {
        term.eprint("lane-commit: bash: {err}")
        return (code: 127, output: "")
    }
    (code: status(receipt), output: chomp(receipt.output))
}

// The precommit's last 20 lines, then the failure line.
pre_failed: fn(out: String) -> String Never! {
    lines: out.split("\n").to_list()
    from: if lines.len() > 20 -> lines.len() - 20 else -> 0
    "{chomp(lines[from..<lines.len()].join("\n"))}\nlane-commit: precommit failed; reverted"
}

commit_job: fn(l: Lander, job: String) Never! -[Exec, FS, IO, Time.Wait]> {
    lane: contents("{job}/lane")
    paths: records(files.read("{job}/files") ?? "")
    add := [String]{"add", "-A", "-f", "--"}
    add.extend(paths.copy())
    _ := runs(git(l, add))
    commit := [String]{"commit", "-q", "-F", "{job}/msg", "--"}
    commit.extend(paths.copy())
    if runs(git(l, commit)) == 0 {
        finish(job, 0, "lane-commit: {short_head(l)} by {lane}")
        return
    }
    reset := [String]{"reset", "-q", "--"}
    reset.extend(paths.copy())
    _ := runs(git(l, reset))
    revert_job(l, job)
    finish(job, 6, "lane-commit: commit failed; reverted")
}

// Apply one job onto the current tree (with earlier batch jobs applied).
// Whether it applied.
apply_job: fn(l: Lander, job: String) -[Exec, FS, IO, Time.Wait]> Bool Never! {
    paths: files_of(l, "{job}/patch")
    put_list("{job}/files", paths)
    if paths.len() == 0 {
        finish(job, 2, "lane-commit: patch lists no files or does not parse")
        return false
    }
    loop path in paths {
        if output(git(l, [String]{"status", "--porcelain", "--", path.copy()})) != "" {
            finish(job, 3, "lane-commit: {path} has uncommitted changes in the shared worktree; refusing")
            return false
        }
    }
    if runs_quiet(git(l, [String]{"apply", "--check", "{job}/patch"})) != 0 {
        finish(job, 4, "lane-commit: patch does not apply to HEAD {short_head(l)}; rebase your export")
        return false
    }
    touch("{job}/applied")
    if runs(git(l, [String]{"apply", "{job}/patch"})) != 0 {
        finish(job, 4, "lane-commit: patch failed to apply")
        return false
    }
    true
}

put_list: fn(path: String, paths: [String]) Never! -[FS, IO]> {
    files.write(path, listed(paths)) ?? term.eprint("lane-commit: {path}: {err}")
}

// Path freeze: the freeze file holds the end time (epoch seconds) on line 1,
// the exempt lane on line 2, then one repo-relative path prefix per line. Until
// the end time, a job from another lane that touches a frozen prefix waits in
// the queue (it neither fails nor lands).
frozen: fn(l: Lander, lane: String, paths: [String]) -[FS, Time]> Bool Never! {
    text: files.read(l.freeze) ?? return false
    lines: records(text)
    if lines.len() == 0 -> return false
    end: lines[0].to_int() ?? return false
    if time.unix_ns() /% 1000000000 >= end -> return false
    exempt: if lines.len() > 1 -> lines[1].copy() else -> ""
    if lane == exempt -> return false
    loop prefix in lines[(if lines.len() > 2 -> 2 else -> lines.len())..<lines.len()] {
        if prefix == "" -> next
        loop path in paths {
            if path.starts_with(prefix) -> return true
        }
    }
    false
}

// Whether the hold switch keeps `lane` from landing: it exists and does not
// list the lane.
held: fn(l: Lander, lane: String) -[FS]> Bool Never! {
    text: files.read(l.hold) ?? return false
    !records(text).contains(lane)
}

// Whether the process `pid` names is alive (`kill -0`).
alive: fn(pid: String) -[Env]> Bool Never! {
    number: pid.to_int() ?? return false
    #Unsafe("signal 0 delivers nothing: it only asks whether a queued job's caller still runs") {
        if sys.kill(number, 0) == .Err(_) -> return false
    }
    true
}

// Whether a path of `paths` is already one of `taken`.
overlaps: fn(paths: [String], taken: [String]) -> Bool Never! {
    loop path in paths {
        if taken.contains(path) -> return true
    }
    false
}

drain: fn(l: Lander) Never! -[Env, Exec, FS, IO, Time, Time.Wait]> {
    if !files.is_dir(l.worktree) -> return
    batch := [String]{}
    union := [String]{}
    loop job in jobs_in(l.queue) {
        if !files.exists("{job}/ready") || files.exists("{job}/result") -> next
        // A job whose caller died is dropped, never landed.
        if !alive(contents("{job}/pid")) {
            files.remove_all(job) ?? ()
            next
        }
        lane: contents("{job}/lane")
        if held(l, lane) -> next
        peek: files_of(l, "{job}/patch")
        put_list("{job}/files.peek", peek)
        if frozen(l, lane, peek) -> next
        // A job that touches a path already in the batch waits for the next round.
        if overlaps(peek, union) -> next
        if apply_job(l, job) {
            batch.push(job.copy())
            union.extend(records(files.read("{job}/files") ?? ""))
        }
    }
    if batch.len() == 0 -> return
    whole: run_pre(l, union)
    if whole.code == 0 {
        loop job in batch -> commit_job(l, job)
        return
    }
    // Batch failed: revert all (newest first), then land each job alone so only the bad one fails.
    loop at in 0..<batch.len() -> revert_job(l, batch[batch.len() - 1 - at])
    if batch.len() == 1 {
        finish(batch[0], 5, pre_failed(whole.output))
        return
    }
    loop job in batch {
        if !apply_job(l, job) -> next
        alone: run_pre(l, records(files.read("{job}/files") ?? ""))
        if alone.code == 0 {
            commit_job(l, job)
            next
        }
        revert_job(l, job)
        finish(job, 5, pre_failed(alone.output))
    }
}

// Whether a lander is draining, read by the interrupt handler: only a drain
// has patches of its own to revert.
Draining: struct {
    on: Bool
}

run: fn() {
    args: process.args()
    if args.len() < 1 || args[0] == "" -> quit("lane-commit: 1: lane", 1)
    if args.len() < 2 || args[1] == "" -> quit("lane-commit: 2: message file", 1)
    if args.len() < 3 || args[2] == "" -> quit("lane-commit: 3: patch file", 1)
    lane: args[0].copy()
    cwd: files.cwd() ?? "."
    run_dir: run_root()
    l: Lander{
        worktree: absolute(cwd, env_or("LANE_COMMIT_WORKTREE", dev_worktree())),
        queue: absolute(cwd, env_or("LANE_COMMIT_QUEUE", "{run_dir}/commit-queue")),
        precommit: absolute(cwd, env_or("LANE_COMMIT_PRECOMMIT", "{state_root()}/scratch/Main/precommit.sh")),
        hold: absolute(cwd, "{run_dir}/commit-hold"),
        freeze: absolute(cwd, "{run_dir}/path-freeze"),
    }
    lock: absolute(cwd, env_or("LANE_COMMIT_LOCK", "{run_dir}/commit.lock"))
    files.create_dir_all(l.queue) ?? term.eprint("lane-commit: {l.queue}: {err}")
    // Hold switch: while the hold file exists, only the lanes it lists (one per line) land.
    loop held(l, lane) -> time.sleep(20s)

    job: "{l.queue}/{time.unix_ns()}-{sys.pid()}"
    queued: files.create_dir_all(job) == .Ok(_) && files.copy(args[1], "{job}/msg") == .Ok(_) && files.copy(args[2], "{job}/patch") == .Ok(_)
    if !queued {
        emit("lane-commit: cannot queue the job\n")
        process.exit(2)
    }
    put("{job}/lane", lane)
    put("{job}/pid", "{sys.pid()}")
    touch("{job}/ready")

    draining: shared Draining{on: false}
    // Copies of a shared value share its cell.
    handler_draining: draining.copy()
    handler_lander: l.copy()
    sys.on_interrupt(() -> {
        permit: handler_draining.guard_read()
        if !permit.value.on -> process.exit(130)
        interrupted(handler_lander)
    })
    loop !files.exists("{job}/result") {
        // The lock is held until this round ends.
        _held: files.try_lock(lock) ?? {
            time.sleep(5s)
            next
        }
        draining.guard_edit().value.on = true
        drain(l)
        draining.guard_edit().value.on = false
    }
    emit(files.read("{job}/out") ?? "")
    code: contents("{job}/rc").to_int() ?? 1
    files.remove_all(job) ?? ()
    process.exit(code)
}
```

### streamb10

The Lander's streamB10.sh and its Jet twin StreamB10.jet (#5072 c15): wait for batch A's stage-1 compiler, make it a seed, and hand batch B's staged SUBs to integrate10.sh; the fixtures hold the batches, the admitted seed and a stand-in integrate10.sh.

- Original: `Tools/agent/orchestrator/streamB10.sh` (shell (bash), 24 lines)
- Jet twin: `Tools/agent/orchestrator/StreamB10.jet` (123 lines)
- Fixture cases: 2 (args per case: `b1 b1-next subs/Lane-1 subs/Lane-9`; `b2 b2-next` exit 2)

| Bar | Result | Measurement |
|---|---|---|
| parity | PASS | 2/2 cases identical |
| speed | FAIL | wall Jet/original 1.28 (40.8 ms against 31.9 ms; lost: every original round below every Jet round); peak memory Jet/original 1.71 (17.9 MB against 10.5 MB; lost: every original round below every Jet round); medians of 5 interleaved rounds |
| tokens | FAIL | Jet 873, original 166 (+707, Jet/original 5.26) |
| reasoning | FAIL | no review record `Tools/gauntlet/entries/streamb10/review.json`; branches Jet 37, original 4 (+33); control nesting Jet 2, original 2 (0) |
| verdict | FAIL | loses speed, tokens, reasoning; the original keeps running |

#### Original: Tools/agent/orchestrator/streamB10.sh

```sh
#!/usr/bin/env bash
# streamB10.sh A_BATCH B_BATCH SUB... : as soon as A_BATCH's census has a stage-1 compiler with its runtime, run
# B_BATCH (the SUBs that need a seed carrying A's rows) through integrate10.sh with that compiler as its seed.
# The original of gauntlet tool entry streamb10; StreamB10.jet beside it is its Jet twin. The batches live in
# $JET_SCRATCH_ROOT/lanes/Main/integ and the seed's IDENTITY and jetpack come from $JET_SEED (Tools/agent/roots.sh);
# integrate10.sh is the one in $JET_ORCHESTRATOR_DIR, this directory unless set.
set -u
A=$1; B=$2; shift 2
. "$(dirname "$0")/../roots.sh"
M=$JET_SCRATCH_ROOT/lanes/Main; G=${JET_ORCHESTRATOR_DIR:-$(dirname "$0")}
CO=$M/integ/$A/census.out
until awk '/^runtime rc=0/{f=1} END{exit !f}' "$CO" 2>/dev/null; do
  awk '/^runtime rc=1|^build rc=[1-9]|^check rc=[1-9]/{f=1} END{exit !f}' "$CO" 2>/dev/null && { echo "streamB: $A has no compiler"; exit 2; }
  sleep 30
done
S=$M/integ/seed-$A; rm -rf "$S"; mkdir -p "$S"
cp "$M/integ/$A/tree/Compiler/JetCli/.jet/build/jet" "$S/jet"
cp -rL "$M/integ/$A/tree/Compiler/JetCli/.jet/build/jet-runtime" "$S/jet-runtime"
C0=$(readlink -f "$JET_SEED")
cp "$C0/IDENTITY" "$S/IDENTITY"; ln -sfn "$C0/jetpack" "$S/jetpack"
echo "streamB: seed $S ready"
subs=(); for s in "$@"; do if [ -d "$s" ]; then subs+=("$s"); else echo "streamB: not staged, skipped: $s"; fi; done
CENSUS_JET_DIR=$S "$G/integrate10.sh" "$B" "${subs[@]}"
```

#### Jet twin: Tools/agent/orchestrator/StreamB10.jet

```jet
// StreamB10.jet A_BATCH B_BATCH SUB...: as soon as A_BATCH's census has a
// stage-1 compiler with its runtime, runs B_BATCH (the SUBs that need a seed
// carrying A's rows) through integrate10.sh with that compiler as its seed.
// The Jet twin of streamB10.sh (gauntlet tool entry streamb10): the batches
// live in $JET_SCRATCH_ROOT/lanes/Main/integ, the seed's IDENTITY and jetpack
// come from $JET_SEED (as Tools/agent/roots.sh sets them), and integrate10.sh
// is the one in $JET_ORCHESTRATOR_DIR, this program's directory unless set.
use {
    files: core.files
    process: core.process
    sys: core.sys
    term: core.term
    time: core.time
}

env_or: fn(name: String, fallback: String) -[Env]> String Never! {
    value: process.env_get(name) ?? return fallback.copy()
    if value == "" -> fallback.copy() else -> value
}

// What the machine's jet/env file sets `name` to (the default of its
// `${NAME:-default}` form), None when it sets nothing.
env_file_value: fn(name: String) -[Env, FS]> String? Never! {
    home: env_or("HOME", "")
    text: files.read("{env_or("XDG_CONFIG_HOME", "{home}/.config")}/jet/env") ?? return None
    defaulted: "${{{name}:-"
    found := ""
    seen := false
    loop line in text.lines() {
        parts: line.trim().removeprefix("export ").partition("=")
        if parts.sep == "" || parts.head != name -> next
        value := parts.tail.removeprefix("\"").removesuffix("\"").removeprefix("'").removesuffix("'")
        if value.starts_with(defaulted) && value.ends_with("}}") {
            value = value.removeprefix(defaulted).removesuffix("}}")
        }
        found = value.replace("$HOME", home)
        seen = true
    }
    if seen -> Val(found) else -> None
}

// A root as Tools/agent/roots.sh sets it: the environment, else the jet/env
// file, else `fallback`.
root: fn(name: String, fallback: String) -[Env, FS]> String Never! {
    found: env_or(name, "")
    if found != "" -> return found
    env_file_value(name) ?? fallback.copy()
}

scratch_root: fn() -[Env, FS]> String Never! {
    root("JET_SCRATCH_ROOT", root("JET_STATE_ROOT", "{env_or("XDG_CACHE_HOME", "{env_or("HOME", "")}/.cache")}/jet-dev"))
}

seed_root: fn() -[Env, FS]> String Never! {
    seeds: root("JET_SEEDS", "{env_or("XDG_DATA_HOME", "{env_or("HOME", "")}/.local/share")}/jet-seeds")
    root("JET_SEED", "{seeds}/current")
}

// Whether `line` reports `step` failing: `step rc=` and a nonzero code.
step_failed: fn(line: String, step: String) -> Bool Never! {
    rest: line.removeprefix("{step} rc=")
    if rest.len() == line.len() || rest.len() == 0 -> return false
    first: Int.from_u8(rest.bytes()[0])
    first >= 49 && first <= 57
}

// "ready" once the census output has a runtime, "failed" when it never will,
// else "".
census_state: fn(text: String) -> String Never! {
    failed := false
    loop line in text.lines() {
        if line.starts_with("runtime rc=0") -> return "ready"
        if line.starts_with("runtime rc=1") || step_failed(line, "build") || step_failed(line, "check") {
            failed = true
        }
    }
    if failed -> "failed" else -> ""
}

fail: fn(message: String, code: Int) -[Exec, IO]> Never {
    term.eprint(message)
    process.exit(code)
}

run: fn() {
    args: process.args()
    if args.len() < 2 {
        fail("usage: jet Tools/agent/orchestrator/StreamB10.jet A_BATCH B_BATCH SUB...", 1)
    }
    batch: args[0]
    main: "{scratch_root()}/lanes/Main"
    orchestrator: env_or("JET_ORCHESTRATOR_DIR", files.dirname(sys.executable()))
    census: "{main}/integ/{batch}/census.out"
    loop {
        state: census_state(files.read(census) ?? "")
        if state == "ready" -> break
        if state == "failed" {
            print("streamB: {batch} has no compiler")
            process.exit(2)
        }
        time.sleep(30s)
    }
    seed: "{main}/integ/seed-{batch}"
    files.remove_all(seed) ?? ()
    files.create_dir_all(seed) ?? term.eprint("streamB: {seed}: {err}")
    build: "{main}/integ/{batch}/tree/Compiler/JetCli/.jet/build"
    files.copy("{build}/jet", "{seed}/jet") ?? term.eprint("streamB: {build}/jet: {err}")
    files.copy_dir("{build}/jet-runtime", "{seed}/jet-runtime") ?? term.eprint("streamB: {build}/jet-runtime: {err}")
    current: files.canonicalize(seed_root()) ?? seed_root()
    files.copy("{current}/IDENTITY", "{seed}/IDENTITY") ?? term.eprint("streamB: {current}/IDENTITY: {err}")
    files.symlink("{current}/jetpack", "{seed}/jetpack") ?? term.eprint("streamB: {seed}/jetpack: {err}")
    print("streamB: seed {seed} ready")
    argv := [String]{"{orchestrator}/integrate10.sh", args[1].copy()}
    loop sub in args.slice(2, args.len()) {
        if {
            files.is_dir(sub) -> argv.push(sub.copy())
            else -> print("streamB: not staged, skipped: {sub}")
        }
    }
    receipt: process.cmd(argv).env("CENSUS_JET_DIR", seed).stdin(.Inherit).stdout(.Inherit).stderr(.Inherit).run() ?? return fail("streamB: {argv[0]}: {err}", 127)
    process.exit(if receipt.signal != None -> 128 + (receipt.signal ?? 0) else -> receipt.code)
}
```

### tower-spec-apply

Main's Tower spec apply tower-spec-apply.mjs and its Jet twin TowerSpecApply.jet (#5072 c15): for each card in a spec, read the card, rewrite, replace (one criterion or every open one), remove and add criteria, change the title, body and plan, and print the changes; with --go, write the card patch, the log line and the closing evidence through the Tower writer. The fixtures hold a stand-in Tower checkout whose reader prints cards from cards/N.json (one wrapped in {card}, one bare with non-ASCII text, one for the failures) and a stand-in writer that prints its arguments and the patch. The dry spec exercises every change, the Object.entries order of rewrite keys, UTF-16 cuts through a surrogate pair, JavaScript's whitespace in the removal note and each failure the original throws, an unknown card included; the go spec writes them; a missing and a broken spec exit 1.

- Original: `Tools/agent/orchestrator/tower-spec-apply.mjs` (JavaScript (node), 80 lines)
- Jet twin: `Tools/agent/orchestrator/TowerSpecApply.jet` (428 lines)
- Fixture cases: 4 (args per case: `specs/dry.json`; `specs/go.json --go`; `specs/none.json` exit 1; `specs/bad.json` exit 1)

| Bar | Result | Measurement |
|---|---|---|
| parity | PASS | 4/4 cases identical |
| speed | FAIL | wall Jet/original 0.80 (697.8 ms against 875.5 ms; inconclusive: the rounds overlap); peak memory Jet/original 0.87 (47.3 MB against 54.5 MB; every Jet round below every original round); medians of 5 interleaved rounds |
| tokens | FAIL | Jet 3314, original 1273 (+2041, Jet/original 2.60) |
| reasoning | FAIL | no review record `Tools/gauntlet/entries/tower-spec-apply/review.json`; branches Jet 144, original 53 (+91); control nesting Jet 4, original 4 (0) |
| verdict | FAIL | loses speed, tokens, reasoning; the original keeps running |

#### Original: Tools/agent/orchestrator/tower-spec-apply.mjs

```js
// tower-spec-apply.mjs SPEC.json [--go]: applies a spec of Tower card edits.
// Spec: [{num, title?, bodyReplace?:[[from,to]], planReplace?:[[from,to]], body?, plan?,
//         rewrite?:{n:text}, replace?:[[n,from,to]] (n="*" = every open criterion),
//         remove?:[n], add?:[text], log:text, close?:evidence, closeKind?, addTags?:[t]}]
// Without --go: dry run printing the changes.
//
// The original of gauntlet tool entry tower-spec-apply; TowerSpecApply.jet beside
// it is its Jet twin. Cards are read with `node Tools/tower/tower.mjs` in
// $TOWER_SPEC_REPO (this checkout unless set) and written through Main's Tower
// writer, $TOWER_SPEC_WRITER ($JET_STATE_ROOT/scratch/Main/tower-dev.mjs unless
// set), which takes the card patches from $TOWER_SPEC_PATCHES
// ($JET_SCRATCH_ROOT/lanes/Main unless set). A relative root names a path in the
// caller's directory.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { stateRoot, scratchRoot } from "../roots.mjs";
const [specPath, go] = [process.argv[2], process.argv.includes("--go")];
const spec = JSON.parse(fs.readFileSync(specPath, "utf8"));
const REPO = path.resolve(process.env.TOWER_SPEC_REPO || fileURLToPath(new URL("../../..", import.meta.url)));
const WRITER = path.resolve(process.env.TOWER_SPEC_WRITER || path.join(stateRoot(), "scratch/Main/tower-dev.mjs"));
const PATCHES = path.resolve(process.env.TOWER_SPEC_PATCHES || path.join(scratchRoot(), "lanes/Main"));
const env = { ...process.env }; delete env.GIT_INDEX_FILE;
const towerRead = (...a) => execFileSync("node", ["Tools/tower/tower.mjs", ...a], { cwd: REPO, env, maxBuffer: 1 << 26 }).toString();
const towerWrite = (...a) => execFileSync("node", [WRITER, ...a, "--by", "Main"], { cwd: REPO, env, maxBuffer: 1 << 26 }).toString().trim();
const sub = (t, from, to, where) => {
  if (!t.includes(from)) throw new Error(`${where}: text not found: ${from.slice(0, 80)}`);
  return t.split(from).join(to);
};
let failed = 0;
for (const s of spec) {
  try {
    const j = JSON.parse(towerRead("card", "show", String(s.num), "--json"));
    const c = j.card || j;
    const patch = {};
    const crit = (c.criteria || []).map(k => ({ ...k }));
    const byN = n => { const k = crit.find(k => k.n === Number(n)); if (!k) throw new Error(`#${s.num} no c${n}`); return k; };
    const changes = [];
    for (const [n, text] of Object.entries(s.rewrite || {})) { const k = byN(n); if (k.status !== "open") throw new Error(`#${s.num} c${n} is ${k.status}`); changes.push(`c${n} REWRITE\n   old: ${k.text.slice(0, 400)}\n   new: ${text.slice(0, 400)}`); k.text = text; }
    for (const [n, from, to] of s.replace || []) {
      const ks = n === "*" ? crit.filter(k => k.status === "open" && k.text.includes(from)) : [byN(n)];
      if (n === "*" && !ks.length) throw new Error(`#${s.num} *: no open criterion has ${from.slice(0, 60)}`);
      for (const k of ks) { k.text = sub(k.text, from, to, `#${s.num} c${k.n}`); changes.push(`c${k.n} REPLACE '${from.slice(0, 70)}' -> '${to.slice(0, 70)}'`); }
    }
    const removed = [];
    for (const n of s.remove || []) { const k = byN(n); if (k.status !== "open") throw new Error(`#${s.num} c${n} is ${k.status}, not removable`); removed.push(k); }
    let next = Math.max(0, ...crit.map(k => k.n)) + 1;
    const kept = crit.filter(k => !removed.includes(k));
    for (const text of s.add || []) { kept.push({ n: next, text, status: "open" }); changes.push(`c${next} ADD ${text.slice(0, 300)}`); next++; }
    for (const k of removed) changes.push(`c${k.n} REMOVE ${k.text.slice(0, 160)}`);
    if (changes.length) patch.criteria = kept;
    if (s.title) { patch.title = s.title; changes.push(`TITLE ${c.title}\n   -> ${s.title}`); }
    let body = c.body || "", plan = c.plan || "";
    for (const [f, t] of s.bodyReplace || []) body = sub(body, f, t, `#${s.num} body`);
    for (const [f, t] of s.planReplace || []) plan = sub(plan, f, t, `#${s.num} plan`);
    if (s.body !== undefined) body = s.body;
    if (s.plan !== undefined) plan = s.plan;
    if (body !== (c.body || "")) { patch.body = body; changes.push(`BODY changed (${(c.body || "").length} -> ${body.length} chars)`); }
    if (plan !== (c.plan || "")) { patch.plan = plan; changes.push(`PLAN changed (${(c.plan || "").length} -> ${plan.length} chars)`); }
    if (s.addTags) patch.addTags = s.addTags;
    const removedNote = removed.length ? ` Removed as superseded: ${removed.map(k => `c${k.n} "${k.text.slice(0, 140).replace(/\s+/g, " ")}${k.text.length > 140 ? "…" : ""}"`).join("; ")}.` : "";
    const logText = (s.log || "") + removedNote;
    console.log(`\n#### #${s.num} ${c.title.slice(0, 90)} [${c.phase}]\n  ` + changes.join("\n  ") + (s.close ? `\n  CLOSE: ${s.close.slice(0, 200)}` : "") + `\n  LOG: ${logText.slice(0, 600)}`);
    if (!go) continue;
    if (Object.keys(patch).length) {
      const f = path.join(PATCHES, `obe-patch-${s.num}.json`);
      fs.writeFileSync(f, JSON.stringify(patch));
      console.log("  " + towerWrite("card", "update", String(s.num), "--file", f));
    }
    if (logText.trim()) console.log("  " + towerWrite("card", "log", String(s.num), "--text", logText.trim()));
    if (s.close) {
      const cur = JSON.parse(towerRead("card", "show", String(s.num), "--json")); const cc = cur.card || cur;
      for (const k of cc.criteria || []) if (k.status === "open") console.log("  " + towerWrite("card", "criteria", String(s.num), "--meet", String(k.n), "--evidence", s.close, ...(s.closeKind ? ["--kind", s.closeKind] : [])));
      console.log("  " + towerWrite("card", "update", String(s.num), "--phase", "done"));
    }
  } catch (e) { failed++; console.log(`!!!! #${s.num}: ${e.message.split("\n")[0]}`); }
}
console.log(failed ? `\nFAILED ${failed}` : "\nALL OK");
```

#### Jet twin: Tools/agent/orchestrator/TowerSpecApply.jet

```jet
#!/usr/bin/env jet
// TowerSpecApply.jet SPEC.json [--go]: applies a spec of Tower card edits.
// Spec: [{num, title?, bodyReplace?:[[from,to]], planReplace?:[[from,to]], body?, plan?,
//         rewrite?:{n:text}, replace?:[[n,from,to]] (n="*" = every open criterion),
//         remove?:[n], add?:[text], log:text, close?:evidence, closeKind?, addTags?:[t]}]
// Without --go: dry run printing the changes.
//
// The Jet twin of tower-spec-apply.mjs (gauntlet tool entry tower-spec-apply):
// cards are read with `node Tools/tower/tower.mjs` in $TOWER_SPEC_REPO (this
// checkout unless set) and written through Main's Tower writer,
// $TOWER_SPEC_WRITER ($JET_STATE_ROOT/scratch/Main/tower-dev.mjs unless set),
// which takes the card patches from $TOWER_SPEC_PATCHES
// ($JET_SCRATCH_ROOT/lanes/Main unless set), as Tools/agent/roots.mjs sets the
// roots. A relative root names a path in the caller's directory. Text is
// measured and cut in UTF-16 code units and its blanks are JavaScript's `\s`,
// as the original's string methods count them; the spec's texts are strings.
use {
    files: core.files
    json: core.encoding.json
    process: core.process
    sys: core.sys
    term: core.term
}

TOOL: "/Tools/agent/orchestrator/TowerSpecApply.jet"

env_or: fn(name: String, fallback: String) -[Env]> String Never! {
    value: process.env_get(name) ?? return fallback.copy()
    if value == "" -> fallback.copy() else -> value
}

state_root: fn() -[Env]> String Never! { env_or("JET_STATE_ROOT", "{env_or("XDG_CACHE_HOME", "{env_or("HOME", "")}/.cache")}/jet-dev") }

scratch_root: fn() -[Env]> String Never! { env_or("JET_SCRATCH_ROOT", state_root()) }

// `path` as an absolute path: a relative one names a path in `cwd`.
absolute: fn(cwd: String, path: String) -> String Never! {
    if path.starts_with("/") -> path.copy() else -> "{cwd}/{path}"
}

// The scalars of `text`, each as text.
scalars: fn(text: String) -> [String] Never! {
    out := [String]{}
    loop ch in text.chars() -> out.push("{ch}")
    out
}

// The scalar's length in UTF-16 code units, the unit JavaScript strings count.
units: fn(c: String) -> Int Never! {
    if c.count_bytes() == 4 -> 2 else -> 1
}

js_length: fn(text: String) -> Int Never! {
    n := 0
    loop c in scalars(text) -> n += units(c)
    n
}

// `text.slice(0, limit)` as JavaScript prints it: a cut through a surrogate
// pair leaves its first half, which prints as U+FFFD.
clip: fn(text: String, limit: Int) -> String Never! {
    out := [U8]{}
    used := 0
    loop c in scalars(text) {
        width: units(c)
        if used + width > limit {
            if used < limit -> out.extend([U8]{U8{239}, U8{191}, U8{189}})
            return String.from_bytes(out) ?? ""
        }
        out.extend(c.bytes())
        used += width
    }
    String.from_bytes(out) ?? ""
}

// JavaScript's `\s`: Unicode White_Space without U+0085, and U+FEFF.
blank: fn(c: String) -> Bool Never! {
    bytes: c.bytes()
    if bytes == ([U8]{U8{194}, U8{133}}) -> return false
    if bytes == ([U8]{U8{239}, U8{187}, U8{191}}) -> return true
    c.is_whitespace()
}

// `text.replace(/\s+/g, " ")`.
collapse: fn(text: String) -> String Never! {
    out := [U8]{}
    run := false
    loop c in scalars(text) {
        if blank(c) {
            if !run -> out.push(U8{32})
            run = true
            next
        }
        out.extend(c.bytes())
        run = false
    }
    String.from_bytes(out) ?? ""
}

// `text.trim()`.
js_trim: fn(text: String) -> String Never! {
    chars: scalars(text)
    start := 0
    end := chars.len()
    loop start < end && blank(chars[start]) -> start += 1
    loop end > start && blank(chars[end - 1]) -> end -= 1
    out := [U8]{}
    loop i in start..<end -> out.extend(chars[i].bytes())
    String.from_bytes(out) ?? ""
}

// A spec or card value as a JavaScript template prints it; a missing one is
// `undefined`.
shown: fn(value: Dyn?) -> String Never! {
    v: value ?? return "undefined"
    if v == {
        .Text(t) -> t.copy()
        .Int(n) -> "{n}"
        .Number(t) -> t.copy()
        .Bool(b) -> if b -> "true" else -> "false"
        .Null -> "null"
        else -> json.dumps(v) ?? ""
    }
}

// Whether JavaScript takes the value as true.
truthy: fn(value: Dyn?) -> Bool Never! {
    v: value ?? return false
    if v == {
        .Null -> false
        .Bool(b) -> b
        .Int(n) -> n != 0
        .Text(t) -> t != ""
        .Number(t) -> t != "0" && t != "-0"
        else -> true
    }
}

// `value || ""` for a text value.
text_or_empty: fn(value: Dyn?) -> String Never! {
    if value == .Val(.Text(t)) -> return t.copy()
    ""
}

// `value || []`.
items: fn(value: Dyn?) -> [Dyn] Never! {
    if value == .Val(.Array(list)) -> return list.copy()
    [Dyn]{}
}

// `Number(value)` where it is an integer; None for NaN.
number_of: fn(value: Dyn) -> Int? Never! {
    if value == {
        .Int(n) -> Val(n)
        .Number(t) -> Val(t.to_int() ?? return None)
        .Bool(b) -> Val(if b -> 1 else -> 0)
        .Null -> Val(0)
        .Text(t) -> {
            trimmed: js_trim(t)
            if trimmed == "" -> return Val(0)
            n: trimmed.to_int() ?? return None
            Val(n)
        }
        else -> None
    }
}

// A criterion's number where it is an integer.
int_of: fn(value: Dyn?) -> Int? Never! {
    if value == .Val(.Int(n)) -> return Val(n)
    if value == .Val(.Number(t)) -> return Val(t.to_int() ?? return None)
    None
}

// Whether `key` is an array index, which `Object.entries` lists first.
index_key: fn(key: String) -> Int? Never! {
    if key == "" || key.count_bytes() > 10 || !key.isdigit() -> return None
    if key.count_bytes() > 1 && key.starts_with("0") -> return None
    n: key.to_int() ?? return None
    if n >= 4294967295 -> return None
    Val(n)
}

// The keys of an object in `Object.entries` order: array indexes ascending,
// then the rest as written.
entry_keys: fn(value: Dyn?) -> [String] Never! {
    keys := [String]{}
    object: value ?? return keys
    if object == .Object(_) {
        indexes := [Int]{}
        loop key in object.keys() {
            if index_key(key) == .Val(n) -> indexes.push(n)
        }
        indexes.sort()
        loop n in indexes -> keys.push("{n}")
        loop key in object.keys() {
            if index_key(key) == .None -> keys.push(key.copy())
        }
    }
    keys
}

// Why a card's spec failed: the message of the error the original throws.
#Error
Failure: struct {
    message: String
}

fail: fn(message: String) -> Failure Never! { Failure{message: message.copy()} }

// Where the cards are read and written.
Tower: struct {
    repo: String
    writer: String
    patches: String
    // --go: write the changes, not only print them.
    go: Bool
}

// `argv` in the Tower checkout with the caller's git index unset; its stdout,
// or the first line of the error JavaScript's execFileSync throws.
run_node: fn(t: Tower, argv: [String]) -[Exec, IO, Time.Wait]> String Failure! {
    receipt: process.cmd(argv.copy()).cwd(t.repo).env_remove("GIT_INDEX_FILE").stdin(.Inherit).stdout(.Capture).stderr(.Inherit).output_limit(67108864).run() ?? return Err(fail("Command failed: {argv.join(" ")}"))
    if receipt.code != 0 || receipt.signal != .None -> return Err(fail("Command failed: {argv.join(" ")}"))
    Ok(receipt.output)
}

read_card: fn(t: Tower, num: String) -[Exec, IO, Time.Wait]> Dyn Failure! {
    out: run_node(t, [String]{"node", "Tools/tower/tower.mjs", "card", "show", num.copy(), "--json"}) ?? return Err(err)
    j: json.parse(out) ?? return Err(fail("{err}"))
    card: j.get("card")
    if truthy(card) -> return Ok(card ?? j)
    Ok(j)
}

write_card: fn(t: Tower, args: [String]) -[Exec, IO, Time.Wait]> String Failure! {
    argv := [String]{"node", t.writer.copy()}
    argv.extend(args.copy())
    argv.extend([String]{"--by", "Main"})
    out: run_node(t, argv) ?? return Err(err)
    Ok(js_trim(out))
}

// `t` with every `from` replaced by `to`; `where` names it when `from` is absent.
sub: fn(t: String, from: String, to: String, where: String) -> String Failure! {
    if !t.contains(from) -> return Err(fail("{where}: text not found: {clip(from, 80)}"))
    Ok(t.replace(from, to))
}

// The index of criterion `n` (`Number(n)`) in `crit`.
by_n: fn(crit: [Dyn], n: Dyn, num: String) -> Int Failure! {
    number: number_of(n) ?? return Err(fail("#{num} no c{shown(Val(n))}"))
    loop i in 0..<crit.len() {
        if int_of(crit[i].get("n")) == .Val(found) && found == number -> return Ok(i)
    }
    Err(fail("#{num} no c{shown(Val(n))}"))
}

field_text: fn(row: Dyn, name: String) -> String Never! { text_or_empty(row.get(name)) }

apply_spec: fn(t: Tower, s: Dyn) Failure! -[Exec, FS, IO, Time.Wait]> {
    num: shown(s.get("num"))
    c: read_card(t, num) ?? return Err(err)
    crit := items(c.get("criteria"))
    changes := [String]{}

    rewrite: s.get("rewrite")
    loop key in entry_keys(rewrite) {
        at: by_n(crit, Dyn.Text(key), num) ?? return Err(err)
        status: shown(crit[at].get("status"))
        if status != "open" -> return Err(fail("#{num} c{key} is {status}"))
        text: text_or_empty((rewrite ?? Dyn.Null).get(key))
        changes.push("c{key} REWRITE\n   old: {clip(field_text(crit[at], "text"), 400)}\n   new: {clip(text, 400)}")
        row := crit[at].copy()
        row["text"] = Dyn.Text(text)
        crit[at] = row
    }

    loop step in items(s.get("replace")) {
        parts: items(Val(step))
        if parts.len() < 3 -> return Err(fail("#{num} replace needs [n, from, to]"))
        from: text_or_empty(Val(parts[1]))
        to: text_or_empty(Val(parts[2]))
        targets := [Int]{}
        star: parts[0] == Dyn.Text("*")
        if !star -> targets.push(by_n(crit, parts[0], num) ?? return Err(err))
        loop i in 0..<crit.len() {
            if star && shown(crit[i].get("status")) == "open" && field_text(crit[i], "text").contains(from) -> targets.push(i)
        }
        if star && targets.len() == 0 -> return Err(fail("#{num} *: no open criterion has {clip(from, 60)}"))
        loop i in targets {
            label: "c{shown(crit[i].get("n"))}"
            replaced: sub(field_text(crit[i], "text"), from, to, "#{num} {label}") ?? return Err(err)
            row := crit[i].copy()
            row["text"] = Dyn.Text(replaced)
            crit[i] = row
            changes.push("{label} REPLACE '{clip(from, 70)}' -> '{clip(to, 70)}'")
        }
    }

    removed := [Int]{}
    loop n in items(s.get("remove")) {
        at: by_n(crit, n, num) ?? return Err(err)
        status: shown(crit[at].get("status"))
        if status != "open" -> return Err(fail("#{num} c{shown(Val(n))} is {status}, not removable"))
        removed.push(at)
    }
    next_n := 0
    loop row in crit {
        if int_of(row.get("n")) == .Val(n) && n > next_n -> next_n = n
    }
    next_n += 1
    kept := [Dyn]{}
    loop i in 0..<crit.len() {
        if !removed.contains(i) -> kept.push(crit[i].copy())
    }
    loop text in items(s.get("add")) {
        added := Dyn.object()
        added["n"] = Dyn.Int(next_n)
        added["text"] = text.copy()
        added["status"] = Dyn.Text("open")
        kept.push(added)
        changes.push("c{next_n} ADD {clip(text_or_empty(Val(text)), 300)}")
        next_n += 1
    }
    loop i in removed -> changes.push("c{shown(crit[i].get("n"))} REMOVE {clip(field_text(crit[i], "text"), 160)}")

    patch := Dyn.object()
    if changes.len() > 0 -> patch["criteria"] = Dyn.Array(kept)
    title: s.get("title")
    if truthy(title) {
        patch["title"] = title ?? Dyn.Null
        changes.push("TITLE {shown(c.get("title"))}\n   -> {shown(title)}")
    }
    old_body: text_or_empty(c.get("body"))
    old_plan: text_or_empty(c.get("plan"))
    body := old_body.copy()
    plan := old_plan.copy()
    loop pair in items(s.get("bodyReplace")) {
        parts: items(Val(pair))
        if parts.len() < 2 -> return Err(fail("#{num} bodyReplace needs [from, to]"))
        body = sub(body, text_or_empty(Val(parts[0])), text_or_empty(Val(parts[1])), "#{num} body") ?? return Err(err)
    }
    loop pair in items(s.get("planReplace")) {
        parts: items(Val(pair))
        if parts.len() < 2 -> return Err(fail("#{num} planReplace needs [from, to]"))
        plan = sub(plan, text_or_empty(Val(parts[0])), text_or_empty(Val(parts[1])), "#{num} plan") ?? return Err(err)
    }
    if s.has("body") -> body = text_or_empty(s.get("body"))
    if s.has("plan") -> plan = text_or_empty(s.get("plan"))
    if body != old_body {
        patch["body"] = Dyn.Text(body.copy())
        changes.push("BODY changed ({js_length(old_body)} -> {js_length(body)} chars)")
    }
    if plan != old_plan {
        patch["plan"] = Dyn.Text(plan.copy())
        changes.push("PLAN changed ({js_length(old_plan)} -> {js_length(plan)} chars)")
    }
    tags: s.get("addTags")
    if truthy(tags) -> patch["addTags"] = tags ?? Dyn.Null

    note := ""
    if removed.len() > 0 {
        quoted := [String]{}
        loop i in removed {
            text: field_text(crit[i], "text")
            more: if js_length(text) > 140 -> "…" else -> ""
            quoted.push("c{shown(crit[i].get("n"))} \"{collapse(clip(text, 140))}{more}\"")
        }
        note = " Removed as superseded: {quoted.join("; ")}."
    }
    log_text: "{text_or_empty(s.get("log"))}{note}"
    close: s.get("close")
    close_line: if truthy(close) -> "\n  CLOSE: {clip(shown(close), 200)}" else -> ""
    print("\n#### #{num} {clip(shown(c.get("title")), 90)} [{shown(c.get("phase"))}]\n  {changes.join("\n  ")}{close_line}\n  LOG: {clip(log_text, 600)}")
    if !t.go -> return Ok()

    if patch.len() > 0 {
        file: "{t.patches}/obe-patch-{num}.json"
        files.write(file, json.dumps(patch) ?? "") ?? return Err(fail("{err}"))
        print("  {write_card(t, [String]{"card", "update", num.copy(), "--file", file.copy()}) ?? return Err(err)}")
    }
    trimmed: js_trim(log_text)
    if trimmed != "" -> print("  {write_card(t, [String]{"card", "log", num.copy(), "--text", trimmed.copy()}) ?? return Err(err)}")
    if !truthy(close) -> return Ok()
    current: read_card(t, num) ?? return Err(err)
    kind: s.get("closeKind")
    loop k in items(current.get("criteria")) {
        if shown(k.get("status")) != "open" -> next
        args := [String]{"card", "criteria", num.copy(), "--meet", shown(k.get("n")), "--evidence", shown(close)}
        if truthy(kind) -> args.extend([String]{"--kind", shown(kind)})
        print("  {write_card(t, args) ?? return Err(err)}")
    }
    print("  {write_card(t, [String]{"card", "update", num.copy(), "--phase", "done"}) ?? return Err(err)}")
    Ok()
}

// Ends the run with exit code 1 after `message` on stderr, as an uncaught
// error ends the original.
quit: fn(message: String) -[Exec, IO]> Never {
    term.eprint(message)
    process.exit(1)
}

run: fn() {
    args: process.args()
    if args.len() < 1 -> quit("tower-spec-apply: SPEC.json [--go]")
    text: files.read(args[0]) ?? quit("tower-spec-apply: {args[0]}: {err}")
    spec: json.parse(text) ?? quit("tower-spec-apply: {args[0]}: {err}")
    cwd: files.cwd() ?? "."
    script: sys.executable()
    checkout: if script.ends_with(TOOL) -> script.removesuffix(TOOL) else -> cwd.copy()
    t: Tower{
        repo: absolute(cwd, env_or("TOWER_SPEC_REPO", checkout)),
        writer: absolute(cwd, env_or("TOWER_SPEC_WRITER", "{state_root()}/scratch/Main/tower-dev.mjs")),
        patches: absolute(cwd, env_or("TOWER_SPEC_PATCHES", "{scratch_root()}/lanes/Main")),
        go: args.contains("--go"),
    }
    failed := 0
    loop s in items(Val(spec)) {
        if apply_spec(t, s) == .Err(failure) {
            failed += 1
            print("!!!! #{shown(s.get("num"))}: {failure.message.partition("\n").head}")
        }
    }
    if failed > 0 -> print("\nFAILED {failed}") else -> print("\nALL OK")
}
```

### verify-rows

Main orchestrator helper: run each criterion<TAB>profile<TAB>filter row as jet test and print its PASS/FAIL counts and rc in sort -V order (#5072 c15).

- Original: `Tools/agent/orchestrator/verify-rows.sh` (shell (bash), 26 lines)
- Jet twin: `Tools/agent/orchestrator/VerifyRows.jet` (156 lines)
- Fixture cases: 1 (args per case: `rows.tsv out ./fake-jet.jet`)

| Bar | Result | Measurement |
|---|---|---|
| parity | PASS | 1/1 cases identical |
| speed | FAIL | wall Jet/original 0.49 (37.6 ms against 76.7 ms; inconclusive: the rounds overlap); peak memory Jet/original 1.25 (13.7 MB against 11.0 MB; lost: every original round below every Jet round); medians of 5 interleaved rounds |
| tokens | FAIL | Jet 1325, original 181 (+1144, Jet/original 7.32) |
| reasoning | FAIL | no review record `Tools/gauntlet/entries/verify-rows/review.json`; branches Jet 59, original 3 (+56); control nesting Jet 2, original 1 (+1) |
| verdict | FAIL | loses speed, tokens, reasoning; the original keeps running |

#### Original: Tools/agent/orchestrator/verify-rows.sh

```sh
#!/usr/bin/env bash
# verify-rows.sh ROWS OUTDIR [JET]: runs every "criterion<TAB>profile<TAB>filter" row of ROWS as
# `jet test Compiler --profile P --filter F` from the dev worktree, six at a time on CPUs 0-11, and prints
# one line per row: criterion, profile, filter, PASS count, FAIL count, rc. Used to prove "already met" claims.
# The original of gauntlet tool entry verify-rows; its Jet twin is VerifyRows.jet beside it.
set -u
ROWS=${1:?rows}; OUT=${2:?outdir}; JETBIN=${3:-jet}
. "$(dirname "$0")/../roots.sh"
W=$JET_DEV_WORKTREE
# A JET given as a relative path still names it after the cd into the worktree.
case $JETBIN in /*) ;; */*) JETBIN=$PWD/$JETBIN ;; esac
mkdir -p "$OUT"
export OUT JETBIN W
one() {
  c=$1; p=$2; f=$3
  o="$OUT/$c.$p.$f.out"
  ( cd "$W" && timeout 600 "$JETBIN" test Compiler --profile "$p" --filter "$f" ) > "$o" 2>&1
  rc=$?
  line=$(awk '/^\{"PASS"/' "$o" | awk 'END{print}')
  pass=$(printf '%s' "$line" | awk -F'"PASS":' '{split($2,a,","); print a[1]+0}')
  fail=$(printf '%s' "$line" | awk -F'"FAIL":' '{split($2,a,","); print a[1]+0}')
  printf '%s\t%s\t%s\tPASS=%s\tFAIL=%s\trc=%s\n' "$c" "$p" "$f" "${pass:-0}" "${fail:-0}" "$rc"
}
export -f one
awk -F'\t' 'NF>=3{print $1, $2, $3}' "$ROWS" | taskset -c 0-11 xargs -P 6 -L 1 bash -c 'one "$0" "$1" "$2"' | sort -V
```

#### Jet twin: Tools/agent/orchestrator/VerifyRows.jet

```jet
#!/usr/bin/env jet
// VerifyRows.jet ROWS OUTDIR [JET]: runs every "criterion<TAB>profile<TAB>filter"
// row of ROWS as `jet test Compiler --profile P --filter F` from the dev
// worktree, six at a time on CPUs 0-11, each under a 600 s timeout with its
// output in OUTDIR/C.P.F.out, and prints one line per row in `sort -V` order:
// criterion, profile, filter, PASS count, FAIL count, rc. The Jet twin of
// verify-rows.sh (gauntlet tool entry verify-rows).
use {
    files: core.files
    process: core.process
    sys: core.sys
    term: core.term
}

Row: struct {
    criterion: String
    profile: String
    filter: String
}

env_or: fn(name: String, fallback: String) -[Env]> String Never! {
    value: process.env_get(name) ?? return fallback.copy()
    if value == "" -> fallback.copy() else -> value
}

// What the machine's jet/env file sets `name` to (the default of its
// `${NAME:-default}` form), None when it sets nothing.
env_file_value: fn(name: String) -[Env, FS]> String? Never! {
    home: env_or("HOME", "")
    text: files.read("{env_or("XDG_CONFIG_HOME", "{home}/.config")}/jet/env") ?? return None
    defaulted: "${{{name}:-"
    found := ""
    seen := false
    loop line in text.lines() {
        parts: line.trim().removeprefix("export ").partition("=")
        if parts.sep == "" || parts.head != name -> next
        value := parts.tail.removeprefix("\"").removesuffix("\"").removeprefix("'").removesuffix("'")
        if value.starts_with(defaulted) && value.ends_with("}}") -> value = value.removeprefix(defaulted).removesuffix("}}")
        found = value.replace("$HOME", home)
        seen = true
    }
    if seen -> Val(found) else -> None
}

// A root as Tools/agent/roots.sh sets it: the environment, else the jet/env
// file, else `fallback`.
root: fn(name: String, fallback: String) -[Env, FS]> String Never! {
    found: env_or(name, "")
    if found != "" -> return found
    env_file_value(name) ?? fallback.copy()
}

// The number after `"KEY":` in a testkit summary line, 0 when it has none.
count: fn(summary: String, key: String) -> Int Never! {
    summary.partition("\"{key}\":").tail.partition(",").head.partition("}}").head.trim().to_int() ?? 0
}

row_line: fn(row: Row, result: Result<ProcessReceipt, IOError>, out: String) -[FS, IO]> String Never! {
    output := ""
    rc := 127
    if result == {
        .Ok(receipt) -> {
            output = receipt.output.copy()
            rc = if {
                receipt.timed_out -> 124
                receipt.signal != None -> 128 + (receipt.signal ?? 0)
                else -> receipt.code
            }
        }
        .Err(error) -> output = "{error}\n"
    }
    files.write("{out}/{row.criterion}.{row.profile}.{row.filter}.out", output) ?? term.eprint("{out}: {err}")
    summary := ""
    loop line in output.lines() {
        if line.starts_with("{{\"PASS\"") -> summary = line.copy()
    }
    "{row.criterion}\t{row.profile}\t{row.filter}\tPASS={count(summary, "PASS")}\tFAIL={count(summary, "FAIL")}\trc={rc}"
}

is_digit: fn(byte: Int) -> Bool Never! { byte >= 48 && byte <= 57 }

// GNU `sort -V` weights outside digit runs: the end and digits 0, letters
// themselves, `~` below everything, other bytes after letters.
weight: fn(bytes: [U8], at: Int) -> Int Never! {
    if at >= bytes.len() -> return 0
    byte: Int.from_u8(bytes[at])
    if {
        is_digit(byte) -> 0
        (byte >= 65 && byte <= 90) || (byte >= 97 && byte <= 122) -> byte
        byte == 126 -> -1
        else -> byte + 256
    }
}

digit_at: fn(bytes: [U8], at: Int) -> Bool Never! { at < bytes.len() && is_digit(Int.from_u8(bytes[at])) }

text_at: fn(bytes: [U8], at: Int) -> Bool Never! { at < bytes.len() && !is_digit(Int.from_u8(bytes[at])) }

// `sort -V` order: non-digit runs by weight, digit runs as numbers, then
// plain byte order to break a tie.
version_order: fn(left: String, right: String) -> Ordering Never! {
    a: left.bytes()
    b: right.bytes()
    i := 0
    j := 0
    loop i < a.len() || j < b.len() {
        loop text_at(a, i) || text_at(b, j) {
            difference: weight(a, i) - weight(b, j)
            if difference != 0 -> return difference <=> 0
            i += 1
            j += 1
        }
        loop digit_at(a, i) && Int.from_u8(a[i]) == 48 -> i += 1
        loop digit_at(b, j) && Int.from_u8(b[j]) == 48 -> j += 1
        first := 0
        loop digit_at(a, i) && digit_at(b, j) {
            if first == 0 -> first = Int.from_u8(a[i]) - Int.from_u8(b[j])
            i += 1
            j += 1
        }
        if digit_at(a, i) -> return Ordering.Greater
        if digit_at(b, j) -> return Ordering.Less
        if first != 0 -> return first <=> 0
    }
    left <=> right
}

fail: fn(message: String, code: Int) -[Exec, IO]> Never {
    term.eprint(message)
    process.exit(code)
}

run: fn() {
    args: process.args()
    if args.len() < 2 || args.len() > 3 -> fail("usage: jet Tools/agent/orchestrator/VerifyRows.jet ROWS OUTDIR [JET]", 2)
    out: args[1]
    given: args.get(2) ?? "jet"
    // A JET given as a relative path still names it from the worktree.
    jet: if given.contains("/") && !given.starts_with("/") -> "{files.cwd() ?? ""}/{given}" else -> given.copy()
    text: files.read(args[0]) ?? return fail("{args[0]}: {err}", 2)
    files.create_dir_all(out) ?? term.eprint("{out}: {err}")
    rows := [Row]{}
    loop line in text.lines() {
        cells: line.split("\t").to_list()
        if cells.len() >= 3 -> rows.push(Row{criterion: cells[0].copy(), profile: cells[1].copy(), filter: cells[2].copy()})
    }
    // As roots.sh: JET_DEV_WORKTREE, else the checkout holding this program.
    worktree: root("JET_DEV_WORKTREE", files.dirname(files.dirname(files.dirname(sys.executable()))))
    cpus: loop cpu in 0..<12 -> cpu
    specs: loop row in rows -> process.cmd([String]{jet, "test", "Compiler", "--profile", row.profile, "--filter", row.filter}).cwd(worktree).stderr(.Stdout).output_limit(268435456).timeout(600s).cpu_affinity(cpus)
    results: process.run_all(specs, 6) ?? return fail("verify-rows: {err}", 1)
    lines := loop at in 0..<rows.len() -> row_line(rows[at], results[at], out)
    lines.sort_by((left: String, right: String) -> version_order(left, right))
    loop line in lines -> print(line)
}
```

## How to reproduce

```sh
jet Tools/gauntlet/PortBars.jet --runs 5            # every tool entry
jet Tools/gauntlet/PortBars.jet --entry jetc-trace  # one entry
```
