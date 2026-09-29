# #3284 — Property-list (plist) and platform data interchange (CORE-F063, D-CORE-PLIST1=A)

Date: 2026-09-29. Binary: `jet-debug-snapshot14` via `~/.cache/jet-luna/safe-jet.sh`,
source rev `e9c708fa7`. Author: Closer03 (evidence closer). No compiler, runtime or
Core change was made.

## Question

Is `core.encoding.plist` (ratified D-CORE-PLIST1=A: typed XML and binary property
lists over the existing XML kernel) available, and can the existing XML kernel carry
the ratified fixture and a real Apple plist?

## Method

- Probe `plist_absent.jet` (the decision's own example: `plist.encode(CountRecord{count: 2})`,
  `plist.decode<CountRecord>(payload)`), `run --interpret`.
- Probe `plist_baseline.jet`: `xml.parse` of the ratified fixture, of the same document
  with the Apple `<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" ...>` line, and of
  an internal-entity expansion document. Run on default `jet run` and `--interpret`.
- Source read: `Core/encoding/` (no `plist.jet`), `Core/encoding/xml.jet` (DOCTYPE
  rejection, no entity definitions), `Docs/spec/reference/core-library.md:2214-2220`
  ("parses well-formed XML 1.0 without DTDs").
- Probes live in `~/.cache/jet-test-scratch/Closer03/`.

## Evidence

`plist_absent.jet` →

```
Error [E1001]: There is no core module `core.encoding.plist`
  --> plist_absent.jet:1:5
```

`plist_baseline.jet` → default run and interpreter both stop before output:

```
internal compiler error: checked TIR cannot lower to MIR at 0..0:
<corelib>/Core/encoding::Core/encoding/xml.jet::XmlParser::parse_element:
checked arithmetic has no resolved fixed numeric type (source bytes 0..0)
```

So on this binary the XML kernel the plan reuses cannot be lowered at all when a
program calls `xml.parse` (defect below); the fixture/DOCTYPE behaviour could not be
observed at runtime. By source, `xml.jet` rejects any DOCTYPE and defines no named
entities, so a real Apple plist (which carries the DOCTYPE) would be a Syntax error
without the card's planned exact-literal DOCTYPE strip.

## Criterion status

| # | Criterion | Status |
|---|---|---|
| 1 | Distinguish XML and binary plist, date/data/number fidelity, target support | Stated by the ratified decision (XML text via `to_string`, `bplist00` bytes via `encode`; `[U8]`, `Instant` (UTC), `PlistUID` U64 typed; `data`/`date`/UID `Unsupported` in untyped `DataTree`; no platform dependency). Not executable: no module. |
| 2 | Keep an exact fixture | The ratified fixture text is recorded in `plist_baseline.jet`; no golden can exist without the module. |
| 3 | Adapter/platform dependency has its own gate | Met: D-CORE-PLIST1 ratified A (2026-09-12), pure Jet, no platform dependency. |
| 4-7 | Cross-read/write, typed preservation, limits/hostile input, root buffering | Not met: no implementation (E1001). |
| 8 | `serde/encoding_plist` golden | Not met: no example, no module. |

## Verdict

FAIL. This is an implementation card (new `Core/encoding/plist.jet`, binary reader and
writer, registration rows, golden), outside an evidence closer's remit. A prerequisite
defect blocks the XML half: `xml.parse` ICEs on this binary.

## Defects

- `xml.parse` ICE (`XmlParser::parse_element: checked arithmetic has no resolved fixed
  numeric type`) on default run and interpreter; repro `plist_baseline.jet` (any call to
  `xml.parse`).
