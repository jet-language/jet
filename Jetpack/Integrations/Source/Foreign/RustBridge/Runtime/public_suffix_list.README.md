# Public Suffix List snapshot

- Source: https://publicsuffix.org/list/public_suffix_list.dat
- Retrieved: 2026-07-21
- License: Mozilla Public License 2.0 (https://www.mozilla.org/MPL/2.0/)
- Upstream SHA-256: `bc29842a9ffd0b804db0094ba649d2365224f6b65cd415271dc90fa6005f2856`
- Canonicalizer: Python 3.14.6 standard-library `idna` codec (IDNA 2003)
- Compact SHA-256: `1f47883b1e6a9a502f93fbe054437e69eb088933a94b416a1d89f973da242811`

`public_suffix_list.dat` preserves every non-comment, non-blank upstream rule in
source order. Each label is canonicalized with IDNA ToASCII and lowercased while
`*.` / `!` rule markers remain unchanged. Rules are separated by one ASCII space
and terminated by one newline. This keeps runtime matching entirely ASCII, so a
Unicode rule such as `公司.cn` protects its URL-host form `xn--55qx5d.cn`.

Update by downloading source, verifying its license and upstream hash, applying
the stated canonicalizer label-by-label, removing comments/blank lines, joining
rules with one space, then refreshing retrieval date, canonicalizer version,
both hashes, byte-count assertion, and focused wildcard/exception/IDN tests.
