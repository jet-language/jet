# Unicode 17.0.0 conformance data

Jet pins Unicode 17.0.0. Files under `ucd/` come from
`https://www.unicode.org/Public/17.0.0/ucd/`; the four `*Test.txt` files come
from that release's normalization and `ucd/auxiliary/` directories. The
license is the Unicode Data Files and Software License downloaded from
`https://www.unicode.org/license.txt`.

Files under `uca/` are the CLDR 48 root collation data for Unicode 17.0.0,
from `https://raw.githubusercontent.com/unicode-org/cldr/release-48/common/uca/`:
`allkeys_CLDR.txt` (the DUCET with CLDR root's changes).

`SHA256SUMS` records every vendored official input. The std-only generator
verifies its required UCD inputs again before emitting either table copy:

```sh
node Tools/agent/gen-unicode-tables.mjs --check tests/data/unicode/ucd
```

`Tools/agent/CollationTable.jet` verifies `uca/allkeys_CLDR.txt` and
`ucd/PropList.txt` against `SHA256SUMS` and generates
`Core/Text/CollationTable.jet`, the table behind `text.collate`:

```sh
jet Tools/agent/CollationTable.jet          # rewrite the table
jet Tools/agent/CollationTable.jet --check  # fail when the table is stale
```

Generated Rust is checked in. Jet programs never read these files or use the
network at build time or runtime.
