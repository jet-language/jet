# 3283 — INI configuration profile: shipped codec vs ratified D-CORE-INI1=A

Date: 2026-09-29. Closer02. Binary: jet-debug-snapshot14 via `~/.cache/jet-luna/safe-jet.sh`.

## Question

Which INI dialect does `core.encoding.ini` ship today, and how far is it from the ratified D-CORE-INI1=A profile? The ratified profile requires strict raw parsing onto `DataTree`, no `IniTree`, case-sensitive names, rejection of duplicate sections and options, whole-line comments only, indentation continuation, no valueless options, `DEFAULT` inherited at lookup and typed decode, typed conversions for Int/Float/Bool/String, explicit Basic/Extended interpolation with bounded cycles and expansion, and a pure text-in codec.

## Method

- Read `Core/encoding/ini.jet:1-147`, `crates/jet-codegen/src/Prelude/Core.jet:92-93` and the D-CORE-INI1 record (`node Tools/tower/tower.mjs card show 3283`).
- Ran `~/.cache/jet-test-scratch/Closer02/ini_probe.jet` with `safe-jet.sh run --interpret`. It parses 15 inputs and exercises the getters and `to_string`.

## Evidence (observed output, blank lines elided)

```
basic: sections [server]                     [server] count=<2>  host=<example.org>   (both `=` and `:` delimiters)
case: sections [Server]                      [Server] Count=<1>  count=<2>            (case-sensitive keys)
duplicate option: sections [s]               [s] a=<2>                                (last wins; ratified: reject)
duplicate section: sections [s, t]           [s] a=<1> c=<3>  [t] b=<2>               (merged; ratified: reject)
inline comment: sections [s]                 [s] a=<1>  b=<x>                         (inline `;`/`#` stripped; ratified: no inline comments)
whole-line comments: sections [s]            [s] a=<1>
key before section: sections [DEFAULT, s]    [DEFAULT] a=<1> [s] b=<2>                (sectionless key goes to DEFAULT; ratified: sections must be named)
valueless option: rejected line 2: ini line is not a key=value pair                    (matches ratified)
empty value: sections [s]                    [s] a=<>
multiline continuation: rejected line 3: ini line is not a key=value pair              (ratified: indentation continuation is supported)
quoted value: sections [s]                   [s] a=<x y>                              (quotes stripped: a value-domain rewrite)
interpolation text: [s] path=<%(base)s/bin>  ext=<${s:base}/x>                         (raw; no interpolation operation exists)
DEFAULT section: sections [DEFAULT, web]
unclosed section: rejected line 1: ini section is missing a closing bracket
empty section name: rejected line 1: ini section name is empty
web.port via get: null                                                                (DEFAULT not inherited; ratified: inherited at lookup)
web.n get_int: 42   web.bad get_int: null   web.b get_bool: true   web.f get_float: 1.5
WEB.host (case): null
---- to_string (comments dropped?)
[b] z = 1 / y = 2 ; [a] x = 3                                                         (order kept; `# top comment` dropped silently)
```

Surface check: `Core.jet:92` exports `Pair, Section, Ini, empty, parse, to_string, sections, has_section, has_option, get, get_or, get_int, get_bool, get_float, items, options, set, remove_option, remove_section, defaults, INIError`. `parse(text) -> Ini INIError!` (ini.jet:47). No `DataTree`, `EncodingError`, `decode<T>`, `decode_interpolated`, `decode_interpolated_extended` or `IniLimits` exists.

## Criteria mapping

1. Format, precedence, interpolation and secrets: the shipped dialect is "Python configparser without interpolation", last-duplicate-wins, with inline comments stripped and quotes removed (ini.jet:12-17, observed above). The selected dialect is D-CORE-INI1=A (ratified). The codec is pure, and nothing in it reads secrets or the environment. **Recorded.**
2. Owners: `core.encoding.ini` is the owner (a Core source module, `Core.jet:93`), with the encoding family's `DataTree`/`EncodingError` as the target rails. File IO belongs to the caller (`core.files`). **Recorded.**
3. No new parser or interpolation semantics without a gate: D-CORE-INI1=A is that gate (ratified 2026-09-12), so this card's cutover is authorised. No further ballot is needed. **Met.**
4. Section/default/typed-getter case and duplicate policy: **not met.** The shipped codec is case-sensitive (matches), but it merges duplicate sections, keeps the last duplicate option, does not inherit `DEFAULT`, and puts sectionless keys into `DEFAULT`.
5. Strict raw versus Basic/Extended interpolation with bounded cycles and expansion: **not met.** No interpolation operation exists, and `%(x)s`/`${s:k}` stay raw text.
6. Round-trip, comment loss, unsupported domains: partially. Order is kept and comments are dropped (silently, with no disclosure API). Quoted values are rewritten, continuation lines are rejected, and valueless options are rejected (matches).
7. Unopenable files and ambient lookup: the codec takes text only (`parse(text)`), so a missing file is the caller's `IOError` and there is no ambient lookup. **Met by construction**, though no example proves it on this surface.
8. Runnable proof plus removal of `Ini`/`INIError`: **not met.** No `Examples/features/serde/encoding_ini.jet` exists. The cutover is a rewrite of a ~400-line Core module plus `CoreCallRows.jet:1934`, `Core.jet:92-93` and the `tests/conformance/corpus/core/encoding/ini/*.jet` seeds. That is implementation work outside the evidence-closer contract.

## Verdict

**PARTIAL.** Criteria 1-3 are met, and criterion 7 is met by construction. Criteria 4-6 and 8 require the ratified D-CORE-INI1=A cutover, which is unimplemented. What the implementer should use: the gap list in criteria 4-6 above and the observed outputs as regression inputs.
