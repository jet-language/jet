# Effect root structure: consumers and fields

Date: 2026-08-20. Card: #2047 (`c0g5m31z`).

## Result

`Effects.jet` needs no new parsed fields. The current root file is a closed
name list. Leaf declarations, bounds, and prohibitions already live in the
fact and policy paths that consume them. One internal defect is real:
`BuildEffect` is a second, stale effect table. It must read the canonical root
source or disappear. This report does not fix that defect.

No ballot is raised. No adopted field is surface-visible.

## Canonical source

`crates/jet-codegen/src/Prelude/Effects.jet:1-6` states the one-home rule.
The file contains 41 bare roots at `:8-41`, including the 17 foreign-runtime
roots at `:22-38`. `crates/jet-foundation/src/Authority.rs:11-21` embeds and
parses that file into `EFFECT_ROOTS`. `Authority::parse_root` accepts a dotted
name only for its root and leaves leaf handling to sema (`Authority.rs:23-36`).

## Consumer table

| Consumer | Reads today | Sub-scoping (`Net.Http`) | Implication (`Py => FFI`) | Prohibition / requirement |
|---|---|---|---|---|
| Sema effect parser and solver | `Sema/Effects.rs:60-75` validates the closed root and preserves the dotted path. `:134-156` applies ancestor coverage. `:191-199` records inferred paths. | Already works. A root bound covers descendants; a leaf bound stays narrow. Root metadata would duplicate this. | No implication closure. An inferred effect stays the exact root or path recorded by the call. | Prohibited sets already use `effects_covered` (`:152-156`). No root declaration field is read. |
| Sema effect fact registration | `Sema/Bundle.rs:413-425` declares every canonical root, then adds built-in leaf members. `:426-455` adds user `effect Root.Leaf` members. | Existing fact rows hold members. Root-side child lists would create a second home. | `FactDeclaration` has `members`, `deny`, and `from` (`jet-foundation/src/Facts.rs:169-180`), but effect registration leaves `deny` and `from` empty. No implication row exists. | Existing `deny` storage is tag-oriented, not an effect-root schema. |
| `#Caps` / marker arguments | `Policy.rs:918-920` defines `EffectRoots`; `Policy.rs:947-951` gets `Capability` variants from `Authority::EFFECT_ROOTS`. `Policy/MarkerSource.rs:128-137` maps `[Effect]`; `CheckerMarkers.rs:168-216` requires a list. | Test fault paths preserve dotted input and validate only the root (`CheckerMarkers.rs:335-379`). A root field is not needed for marker parsing. | Marker arguments do not compute implications. `Capability` exposes roots from the canonical source only. | `#Caps` checks the supplied set against inferred effects in `Sema/Effects.rs:277-304`; no root prohibition/requirement metadata is read. |
| Facts and reflection | `FactRegistry` stores effect rows and members (`Facts.rs:182-256`). Reflection appends `reflected_facts` (`jet-comptime/src/Comptime/Reflect.rs:2200-2204`, `:2240-2242`, `:2262-2265`). `jet inspect facts --json` renders registered registry rows (`jet-cli/src/Explain.rs:507-545`). | Reflection can show existing root/member rows. A root field would need a new row and renderer, but no consumer asks for it. | Reflection exposes no implication closure. | Reflection exposes registered fact data, not a root-level prohibition or requirement. |
| Package budget parser | `jet-pkg-model/src/Package/Blocks.rs:740-790` parses `effects.allow`, `effects.deny`, and `grants`, then validates each name against the canonical effect parser. | Dotted names already pass root validation. Budget enforcement is tree-aware. | No implication expansion. A grant for `Py` does not add `FFI`. | `allow`, `deny`, and `grants` are the real package prohibition/permission consumers; they belong to `package.jet`, not `Effects.jet`. |
| Transitive package budget checker | `jet-pkg-model/src/EffectBudget.rs:266-331` compares dependency effects with allow, deny, and per-dependency grants using `effect_covers`. It emits E1220 at `:359-371`. | A root or leaf budget already has the requested ancestor behavior. | A root implication field could make the checker close a grant over implied roots, but current code does not do so. No package contract says that `Py` implies `FFI`. | Package `deny` is already the prohibition consumer. A root requirement field would add no current check. |
| Diagnostics | Prelude rows E1220/E1221 are at `Prelude/Diagnostics.jet:618-619`. E1221 names the closed root vocabulary. E1220 is emitted by `EffectBudget.rs:359-371`. E1803 is at `Diagnostics.jet:456`; its runtime copy formats `{Root}.{Operation}` at `jet-repl/src/lib.rs:174-180`. | E1803 proves operation-level messages exist. It does not prove that root definitions need fixed children. | E1803 reports the requested root only. It does not report implied roots. | E1220/E1221 report package budget denial or malformed budget. No diagnostic consumes root requirements. |
| CLI allow/deny flags | `crates/jet-cli/src/CLI.rs:933-945` generates flags from `BuildEffect::ALL`; `Source/main.rs:1180-1188` and `:2076-2090` parse them from the same copy. | The flag path is root-only. It cannot express a leaf operation today. | No implication expansion. It uses only the stale enum. | `--deny-*` is a root prohibition at the CLI layer, but it is not read from root metadata. |
| REPL authorization | `jet-comptime/src/Comptime/Methods/repl_process.rs:13-50` maps Core calls to `(root, operation, resource)`. `jet-repl/src/lib.rs:217-250` applies deny, then allow, then prompt. `Authority::covers` makes the policy ancestor-aware. | Operation/resource data already exists in the request; policy matching remains root-based. A root field would not add operation authorization. | No implication closure. `Py` is not emitted by the REPL request mapper, and `FFI` is not inferred from a foreign-runtime root. | Deny wins, allow follows, prompt is last (`lib.rs:220-249`). This is a real prohibition consumer outside `Effects.jet`. |
| `jet inspect guarantees` / `jet audit` | Guarantees renders component safety rows (`Source/CmdInspect.rs:775-797`). The facts report renders registry rows (`jet-cli/src/Explain.rs:464-505`). Audit commands cover copies, memory, advisories, and dependencies (`Source/CmdInspect.rs` and `Source/Publish/Advisory.rs`); no effect-root read appears in their effect paths. | No current effect consumer. Adding root fields would require a new report contract. | No current effect consumer. | No current effect consumer. Keep this out until a report needs it. |
| FFI effect family | `Effects.jet:21-38` lists foreign-runtime roots. Sema copies an explicit foreign root from the extern signature (`Sema/mod.rs:698-702`; `CheckerInfer/calls/direct_calls.rs:1093-1106`). If no explicit root exists, `Effects.rs:177-192` records `FFI`; it does not add `FFI` when a root such as `Py` is explicit. | Explicit foreign roots can be dotted by the general path parser, but no fixed children are declared. | Evidence says **no**: `Py` does not imply `FFI` today. The code has two mutually exclusive paths: explicit foreign root, or bare `FFI` fallback. | No foreign-family prohibition or requirement is read. |

## Candidate fields

| Candidate | Concrete consumer and use case | Spelling | Verdict |
|---|---|---|---|
| Root subeffects / fixed leaves | Possible consumer: package budgets or REPL operation authorization. Existing consumers already accept dotted paths and existing fact rows already hold members (`Effects.rs:77-106`; `Bundle.rs:421-441`; `EffectBudget.rs:275-317`). | No new spelling. Keep `effect Root.Leaf` declarations and package-side paths. | **Reject root field.** It duplicates D-EFFECT-DECL1=A and would make two child vocabularies. Defer only a future operation registry if a consumer first requires fixed operation names. |
| Root implication (`Root => Root`) | Possible consumer: package budget closure and REPL deny closure. A package that grants `Py` could then cover `FFI`; `--deny-ffi` could deny `Py`. | If ever adopted, use comment-level metadata in the one `Effects.jet` source, for example `// implies FFI` beside `effect Py`; do not add parsed syntax after `effect Name` without a ballot. | **Defer / reject for now.** The proposed consumers are real use cases, but no current consumer reads implication data and current FFI behavior does not establish `Py => FFI`. First raise a semantic decision that defines whether explicit foreign roots also carry `FFI`; only then add the smallest metadata form. No ballot from this research card. |
| Root prohibition | Possible consumer: package `effects.deny`, REPL `--deny-*`, or `#Caps` denial. | No new spelling. Existing policy and package fields already carry denials. | **Reject.** Three current consumers already own prohibition behavior. A root declaration prohibition would duplicate policy and could make a package unable to request a root that the package policy intentionally controls. |
| Root requirement / prerequisite | Possible consumer: sema could reject a use unless another root is declared. | No current spelling. | **Reject.** No checker, budget, REPL, audit, or reflection path reads prerequisites. No use case clears the YAGNI bar. |
| Root display / documentation label | Possible consumer: `jet inspect facts` or editor completion. | Comment-only metadata could label a row. | **Reject.** Current root names already render, and no consumer needs a second label. |

## Drift finding

`BuildEffect` is an independent ten-variant enum (`crates/jet-foundation/src/BuildEffects.rs:5-31`). Its `parse` calls `Authority::parse_root` but then filters through its own `ALL` list (`:63-70`). The canonical source has 41 roots. The enum therefore cannot represent `Panic`, `FFI`, `Go`, `Java`, `DotNet`, `Fortran`, `Cobol`, `Tcl`, `Lua`, `Ada`, `Pascal`, `Dart`, `PowerShell`, `Perl`, `Ruby`, `Php`, `R`, `Com`, `Py`, `Browser`, or `Secret`.

This is a one-fact-written-twice defect. It is not a field proposal and needs a follow-up bug card. This card does not edit it.

## Rejected fields

The following fields have no current consumer and stay out: root
requirements/prerequisites; root display labels; root prohibition metadata;
fixed root child lists; foreign-family membership; and an implicit `FFI` flag
on every foreign-runtime root. The last item has a live negative result: the
explicit-foreign-root path records only the chosen root, while the unannotated
path records `FFI`.

## Decision and follow-up

Adopt: none. Defer: implication metadata only after an owner decision defines
its semantic closure. Reject: root children, prohibitions, requirements,
labels, and implicit foreign-family membership. Therefore no surface ballot is
raised.

The internal `BuildEffect` drift needs a task card. The implementation should
make CLI and REPL flags derive from `Authority::EFFECT_ROOTS`, preserving one
home. That task is outside this research card.

