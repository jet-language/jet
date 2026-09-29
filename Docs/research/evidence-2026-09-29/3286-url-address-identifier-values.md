# #3286 — Canonical URL, address and identifier values (evidence, 2026-09-29)

## Question

Do the typed owners `Core/net/url.jet`, `Core/net/ip.jet`, `Core/crypto/uuid.jet`,
`Core/net/mime.jet` and the header/cookie helpers in `Core/http/http.jet`
keep normalization separate from the original form, reject invalid input with a
typed error, respect percent/Unicode boundaries, and keep case and multiplicity,
all without DNS or network IO?

## Method

- Binary: `~/.cache/jet-test-scratch/jet-current` → `jet-debug-snapshot14`, run through
  `~/.cache/jet-luna/safe-jet.sh`.
- Witness: `~/.cache/jet-test-scratch/Closer05/t3286/value_boundaries.jet`. It sits in a
  scratch package whose `package.jet` grants only `[IO, Mem.Alloc]` (no `Net`), so the
  compiler proves that no cell reaches DNS or sockets.
- Tiers: `safe-jet.sh run`, `safe-jet.sh run --interpret` and `safe-jet.sh build` plus the
  produced binary (`~/.cache/jet-test-scratch/Closer05/tiers.sh`).
- The witness was not landed as `Examples/features/net/value_boundaries.jet`. Several cells
  print wrong results (below), and a golden may only hold correct observed output.

## Evidence

`run` and `run --interpret` printed byte-identical output (rc=0 for both). The
`build` tier failed with rc=101:
`error[E0609]: no field 'fields' on type 'JetHTTPHeaders'` (generated Rust, header cells).

Observed output from `run`, annotated:

| Cell | Observed | Verdict |
|---|---|---|
| dots `HTTPS://Example.COM:443/a/./b/../c?x=1&x=2#frag` | original kept in `raw`; canonical `https://example.com/a/c?x=1&x=2#frag` | OK: normalized separately from the original; repeated `x` keeps its order |
| idna `https://Bücher.example/` | canonical `https://bücher.example/`, host `bücher.example` | **DEFECT**: no IDNA/punycode. The `url_mime` golden expects `xn--bcher-kva.example`, and no punycode exists in Core |
| encoded slash `https://example.com/a%2Fb?q=a%2Fb` | path `/a/b`, canonical `https://example.com/a/b?q=a%2Fb` | **DEFECT**: `%2F` in the path is decoded into a segment separator (path context swap). The query is kept raw |
| bad percent `/a%zz` | error | OK (typed `URLError`, but its variant can't be observed: see the defects) |
| bad label `exa mple.com`, empty label `a..b` | error | OK |
| `https:/nohost` | error | OK |
| file `file:///tmp/a%20b.txt` | error | **DEFECT**: an empty file authority is rejected (`url.jet:49`), although `url.file()` produces exactly this form |
| data `data:text/plain,a%20b` | kept verbatim | OK |
| `parse_qsl("b=2&a=1&b=3")` / `query(...)` | `b=2, a=1, b=3` / `b=2&a=1&b=3` | OK: order and multiplicity kept |
| `percent_encode("a b%")` twice | `a%20b%25`, then `a%2520b%2525` | OK: each call encodes exactly once; nothing is double-encoded implicitly |
| `percent_decode` `%E2%82%AC` / `%zz` / truncated `%E2%82` | `€` / error / error | OK: a truncated UTF-8 sequence is rejected |
| IPv4 `192.168.001.010`, `256.1.1.1`, `1.2.3` | errors | OK: strict. Wording "invalid input during resolve" suggests DNS for a pure parse (see follow-ups) |
| IPv6 `2001:0DB8:…:0001`, `::1` | `2001:db8::1`, `::1` | OK: canonical RFC 5952 text |
| IPv6 zone id `fe80::1%eth0` | error `bad IPv6 group` | UNSUPPORTED: no scoped-address value |
| IPv6 `1::2::3` | error | OK |
| UUID uppercase | lowercased canonical | OK |
| UUID braces / 32-hex simple form | error | Strict policy. Recorded, not a defect |
| UUID invalid hex digit | error | OK |
| headers `Set-Cookie` ×2 + `Content-Type`, lookup `SET-COOKIE` | first `a=1`; all `a=1\|b=2` | OK: case-insensitive, multiplicity and order kept |
| header with an invalid name `Bad Name` | silently dropped (3 fields remain) | No typed error. `headers_append` returns `Headers Never!` |
| cookie with a bad name or bad value | `cookie_encode` returns `""` | No typed error. Silently refused |
| MIME `Text/HTML; charset=UTF-8` | `text/html` | The essence is case-normalized. Parameters are dropped (#3125) |
| MIME `text`, `text/` | error | OK |

## Defects found

1. IDNA is missing from the Jet `core.net.url` owner (non-ASCII host kept raw). This regresses the `url_mime` golden.
2. `%2F` in the path decodes into `/` and changes the path structure. `unparse` cannot restore it.
3. `url.parse("file:///…")` fails (Syntax), so `url.file()` output doesn't round-trip.
4. `URLError` can't be observed by user code. `{err}` gives E0915 (no Display), whose fix suggests `{err:Debug}`, and that gives E0112. A `.Syntax` pattern gives E0305 "is a struct, not an enum". `.message` gives E0302 "only works on struct and tuple values". `use core.net.url.[URLError]` then naming the type gives E0119 "no type called URLError".
5. AOT: `build` of the witness is an ICE (`no field 'fields' on type JetHTTPHeaders`).
6. `Examples/features/net/url_mime.jet` no longer compiles on snapshot14: E2402 for URLError/MIMEError→Err, and E0102 `URL has no method to_string` (plus host/path/query_pairs/join and the other methods the golden uses).

## Verdict

PARTIAL. Criteria 1 to 4 are met by this document. The table above exercises
normalization versus the original, invalid input, the percent and Unicode boundaries,
and case and multiplicity (criteria 1 and 2). Parsing is proven free of `Net` authority
(criterion 3). The gate list below covers criterion 4. The defects found are recorded
separately. Criterion 5 (a landed golden that passes on `run` and `--interpret`) is unmet
until defects 1 to 3 are fixed, and AOT is additionally blocked by defect 5.

## Follow-ups / owner gates (criterion 4)

- Each of these is a new typed value or control and would need its own ballot. None was
  implemented here: a scoped IPv6 address (zone id), typed errors for header/cookie
  construction (today they are silent drops), and lenient UUID input forms (braces and
  simple form).
- The IPv4/IPv6 parse errors say "during resolve". They should name a parse failure so that
  parsing isn't conflated with DNS (wording fix in `Core/net/ip.jet` `net_err`).
