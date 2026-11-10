# Ten one-liners in Jet, Perl and awk (2026-11-09)

For #6067 c5: the side-by-side table the owner reviews. The owner may override the character target.

Source: `tests/cases/terminal/oneliners_match_perl_and_awk_output` (ScriptLead-28). Each row's three commands print
byte-identical output on the same input (c4). Counts are characters as typed, whole command included
(`count.jet`, pinned in `count.out`).

| # | Task | Jet | Perl | awk | Jet / Perl / awk chars |
|---|------|-----|------|-----|------------------------|
| 1 | print field 2 | `jet -e 'input.map((l) -> l.words()[1])'` | `perl -lane 'print $F[1]'` | `gawk '{ print $2 }'` | 39 / 24 / 19 |
| 2 | sum column 3 | `jet -e 'input.map((l) -> l.words()[2].to_int() ?? 0).sum()'` | `perl -lane '$s += $F[2]; END { print $s }'` | `gawk '{ s += $3 } END { print s }'` | 59 / 42 / 34 |
| 3 | count matching lines | `jet -e 'input.count_where((l) -> l.contains("error"))'` | `perl -lne '$n++ if /error/; END { print $n + 0 }'` | `gawk '/error/ { n++ } END { print n + 0 }'` | 54 / 49 / 42 |
| 4 | unique lines in order | `jet -e 's := [String]{}.to_set(); input.filter((l) -> s.add(l.copy()))'` | `perl -ne 'print unless $seen{$_}++'` | `gawk '!seen[$0]++'` | 71 / 35 / 18 |
| 5 | in-place replace | `jet -e 'input.edit((l) -> l.replace("web", "www"))'` | `perl -pi -e 's/web/www/g'` | `gawk -i inplace '{ gsub(/web/, "www") } 1'` | 51 / 25 / 42 |
| 6 | number lines | `jet -e 'input.indexed().map((p) -> "{p.idx + 1} {p.item}")'` | `perl -ne 'print "$. $_"'` | `gawk '{ print NR, $0 }'` | 59 / 24 / 23 |
| 7 | longest line | `jet -e 'input.max_by((l) -> l.len()) ?? ""'` | `perl -ne '$m = $_ if length > length $m; END { print $m }'` | `gawk 'length > length(m) { m = $0 } END { print m }'` | 43 / 58 / 52 |
| 8 | swap two fields | `jet -e 'input.map((l) -> l.words()).map((w) -> "{w[1]} {w[0]}")'` | `perl -lane 'print "$F[1] $F[0]"'` | `gawk '{ print $2, $1 }'` | 64 / 32 / 23 |
| 9 | keep lines longer than 80 | `jet -e 'input.filter((l) -> l.len() > 80)'` | `perl -lne 'print if length > 80'` | `gawk 'length > 80'` | 42 / 32 / 18 |
| 10 | sum a named CSV column | `jet -e 'Sale: struct { amount: Float }; input.csv<Sale>().map((s) -> s.amount).sum()'` | `perl -F, -lane 'if ($. == 1) { ($c) = grep { $F[$_] eq "amount" } 0 .. $#F } else { $s += $F[$c] } END { print $s }'` | `gawk -F, 'NR == 1 { for (i = 1; i <= NF; i++) if ($i == "amount") c = i; next } { s += $c } END { print s }'` | 85 / 116 / 108 |
| | **Total** | | | | **567 / 437 / 379** |

## Against the soft target (Jet at most Perl)

Jet totals 567 characters, against 437 for Perl and 379 for awk. That is 130 characters (30 %) over the target.

- **Where Jet wins:** rows 7 and 10. On row 10, the typed CSV read is also the only version where a misspelled field is
  a compile error; Perl and awk print 0 and exit 0 (c6).
- **Where the gap comes from:** the largest part is the lambda header `(l) -> `: 11 lambdas at 7 characters each,
  77 characters in all. Then:
  - the explicit `words()` call that Perl's `-a` and awk's `$N` avoid (rows 1, 2 and 8);
  - the seen-set spelled out by hand on row 4 (+36 over Perl);
  - the record number spelled `p.idx + 1` on row 6 (+35 over Perl).
- **What would close it:** an implicit lambda parameter, a field accessor on `input`, `unique()` on Iter and a
  line-number accessor. Each is new syntax or public API and needs its own ballot, so none is built here.

The owner reviews these side-by-sides and may override the count (c5).
