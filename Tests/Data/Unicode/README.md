# Unicode 17.0.0 conformance data

Jet pins Unicode 17.0.0. Files under `ucd/` come from
`https://www.unicode.org/Public/17.0.0/ucd/`; the four `*Test.txt` files come
from that release's normalization and `Ucd/Auxiliary/` directories. The
license is the Unicode Data Files and Software License downloaded from
`https://www.unicode.org/license.txt`.

Files under `uca/` are the CLDR 48 root collation data for Unicode 17.0.0,
from `https://raw.githubusercontent.com/unicode-org/cldr/release-48/common/uca/`:
`allkeys_CLDR.txt` (the DUCET with CLDR root's changes) and
`CollationTest_CLDR_NON_IGNORABLE_SHORT.txt` (its UTS #10 conformance test,
run by `Tests/Cases/TextUnicode/collation-conformance.case`).

`SHA256SUMS` records every vendored official input. `Tools/Agent/UnicodeTables.jet`
verifies its required UCD inputs again before it emits the runtime tables and
the Core text assets:

```sh
jet Tools/Agent/UnicodeTables.jet          # rewrite every stale output
jet Tools/Agent/UnicodeTables.jet --check  # fail naming every stale output
```

`Tools/Agent/CollationTable.jet` verifies `Uca/allkeys_CLDR.txt` and
`Ucd/PropList.txt` against `SHA256SUMS` and generates
`Core/Text/CollationTable.jet`, the table behind `text.collate`:

```sh
jet Tools/Agent/CollationTable.jet          # rewrite the table
jet Tools/Agent/CollationTable.jet --check  # fail when the table is stale
```

Generated Jet is checked in. Jet programs never read these files or use the
network at build time or runtime.
