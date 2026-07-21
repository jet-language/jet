# Public Suffix List snapshot

- Source: https://publicsuffix.org/list/public_suffix_list.dat
- Retrieved: 2026-07-21
- License: Mozilla Public License 2.0 (https://www.mozilla.org/MPL/2.0/)
- Upstream SHA-256: `bc29842a9ffd0b804db0094ba649d2365224f6b65cd415271dc90fa6005f2856`
- Compact SHA-256: `05b565386cfae75e414f1d4d3039e496615d937c977b049c60d423dc10c5090d`

`public_suffix_list.dat` preserves every non-comment, non-blank upstream rule in
source order, separated by one ASCII space and terminated by one newline. Update
by downloading source, verifying its license, removing comment/blank lines,
trimming rules, joining them with one space, and refreshing both hashes/date.
