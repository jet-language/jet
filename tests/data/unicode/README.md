# Unicode 16.0.0 conformance data

Jet pins Unicode 16.0.0. Files under `ucd/` come from
`https://www.unicode.org/Public/16.0.0/ucd/`; the four `*Test.txt` files come
from that release's normalization and `ucd/auxiliary/` directories. The
license is the Unicode Data Files and Software License downloaded from
`https://www.unicode.org/license.txt`.

`SHA256SUMS` records every vendored official input. The std-only generator
verifies its required UCD inputs again before emitting either table copy:

```sh
node scripts/agent/gen-unicode-tables.mjs --check tests/data/unicode/ucd
```

Generated Rust is checked in. Jet programs never read these files or use the
network at build time or runtime.
