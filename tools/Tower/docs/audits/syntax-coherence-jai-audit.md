# Jet syntax coherence audit: Jai-leaning pass

Date: 2026-06-30

Scope: current ratified syntax, active Tower ideas, and the owner's request to
steer slightly toward Jai where refactoring up the hierarchy is easy and
consistent, without weakening Jet's safety model.

## Executive finding

Jet is strongest where it follows one visible family:

- `fn Type.method(self)` / inline / `impl Type {}` are structural placements for
  the same method.
- `comptime {}` computes at build time; `$name` splices a comptime value.
- `#PascalCase` marks declarations/scopes/type facts; `#[A, B]` groups markers.
- Capability sigils are compact and mirrored at the call site.

The main risk is not lack of power. The risk is **symbol overload**: `#`, `.`,
`@`, `$`, `~`, `^`, `&`, and `*` are all meaningful. Some choices are excellent
expert ergonomics, but beginners may see sigil soup unless the taxonomy is strict
and docs/LSP group it by job.

Recommendation: keep moving Jai-ward on **structural flexibility and comptime
build power**, not on unrestricted macros or unsafe defaults. Use Jai's best
lesson: refactors should preserve shape. Reject Jai's weakest defaults:
ambient build authority, loose `Any`, and compact unsafe pointer chains outside
audited expert code.

## Current symbol taxonomy

| Family | Current Jet meaning | Coherence grade | Notes |
| --- | --- | --- | --- |
| `#Marker` | declaration/scope/type metadata: `#Test`, `#Unsafe`, `#Ref`, `#PubFile` | strong | Best family. Make it the only marker namespace. |
| `#[A, B]` | multiple item/field/type markers | strong | Good Rust familiarity without `derive(...)` ceremony. |
| `#(...)` | effect set / capability row | medium | Looks related to markers; document as "effect marker." |
| `#=` | immutable binding | weak | Physically okay, but conflicts with `#` marker meaning. Existing law; treat as debt. |
| `$name` | comptime value splice | strong if narrow | Do not use `$` for attrs or control flow. |
| `comptime` | build-time evaluation block/condition | strong | Keyword beats `$if`/`$for` duplication. |
| `.` | member access, method call, enum inference, construction `T.{}`, fan-out `f.[...]` | medium | Powerful one-family story: dot selects/constructs/projects. Risk: typo-looking novelty. |
| `~` | edit capability | medium | Good expert shorthand; physically awkward on many keyboards. Use sparingly, never for stored refs. |
| `^` | take/move capability | strong | Distinct and physically reachable. |
| `&` | shared/escaping capability | medium | Familiar to systems users; still expert-heavy. |
| `*` | raw pointer | strong | Matches C/Rust mental model, gated by `#Unsafe`. |
| `@` | loop labels in older docs / suffix labels | weak | Avoid new `@` surface. It creates a second marker namespace. |

## Physical ergonomics

US keyboard rough pass:

- Easy: `.`, `,`, `:`, `;`, `/`, `-`, `=`, letters.
- Medium: `#`, `$`, `^`, `&`, `*`, `(`, `)`, `{`, `}` because they require
  Shift-number or right-hand Shift.
- Hard/awkward: `~` because it is a shifted far-left/backtick key on many
  layouts; also varies on international keyboards.

Implications:

- `~T` should remain an expert capability, not become a general-purpose stored
  reference spelling.
- Do not add more `@` or `$` prefixes for source-level markers. Each new sigil
  is another physical and mental mode.
- Prefer word markers where the concept is rare and high-impact:
  `#Unsafe("reason")`, `#Impure("reason")`, `#Build("reason")`, `#Uninit`.
- Avoid punctuation sentinels like `---` for safety-sensitive states; they are
  terse but visually fragile near `->`, `--`, `-=`, and ranges.

## Casing

Current rule is mostly right:

- Types: `PascalCase`.
- Functions/values/fields/modules: lower snake or lower names by local convention.
- Markers: `#PascalCase`.
- Enum leading-dot variants: `.Variant`.
- Standard acronyms currently lean all-caps (`JSON`, `YAML`, `IOError`).

Better fit: keep `#PascalCase` markers. Reopen only acronym style:

- `Json`, `Yaml`, `Csv`, `IoError`, `Utf8Error` scan better in mixed names.
- All-caps acronyms are defensible for protocol names and file format literals,
  but type names with stacked acronyms get loud.

Candidate ballot: `D-CASING2`:

- A. Keep all-caps standard acronyms.
- B. PascalCase acronyms in type names (`JsonError`, `IoError`) and all-caps only
  for literal protocol/file-format constants.
- Recommendation: B for scanability, unless migration churn outweighs casing
  cleanup this epoch.

## Grouping rules

Recommended canonical grouping:

1. **Declaration metadata:** `#Marker` or `#[Marker, Other]`.
   Example: `#Test`, `#[Codable, Debug]`, `#PubFile`.
2. **Audited scope gates:** `#Marker("reason") { ... }`.
   Example: `#Unsafe("MMIO write") { ptr.* = value }`.
3. **Function/type facts:** marker before declaration.
   Example: `#Pure fn`, `#Build("pack assets") fn`.
4. **Effects:** `#(Fs, Net)` on the signature.
5. **Comptime execution:** keyword `comptime { ... }`.
6. **Comptime splice:** `$name` only where a computed value is inserted.
7. **Runtime attributes:** same marker family unless the attribute is data inside
   a value (`Style.{}`, `Build.{}`), not syntax.

Do not introduce:

- `@Build`, `@embed`, `@test`: second marker namespace.
- `$Build`, `$if`, `$for`: duplicates `#` markers and `comptime` control flow.
- punctuation sentinels for safety facts (`---`, `!!!`, etc.).

## Jai comparison

Good Jai imports:

- Build code is language code.
- Fast compile-time execution is fun and productive.
- Method/refactor shape is direct.
- Type and value inference reduce ceremony.
- Allocator/arena/context ideas are first-class.
- Pointer and layout power exists for experts.

Jet should diverge on:

- Ambient build I/O: Jet must lock/hash/gate it.
- General `Any`: keep dynamic power behind `reflect.Value` / `DataTree` handles.
- Macro methods / reader macros: keep source syntax owned by Jet.
- Unsafe pointer chains: keep casts/deref inside `#Unsafe`.
- Punctuation-only expert states: prefer searchable markers.

## Adversarial pass

Objection: Jet now violates "one way to mean it" because method placement has
three forms.

Answer: this is structural flexibility, not mechanical duplication. Inline,
`impl`, and external `fn Type.method(self)` lower to the same semantic item.
This is exactly the right Jai-leaning direction: refactor up/down hierarchy
without changing call behavior.

Objection: `#=` immutable binding makes `#` a junk drawer.

Answer: true. It is the weakest part of the surface. Reopening it would be a big
migration, but the audit should name it. If the owner wants a cleaner Jai/Odin
feel, `::` is the coherent immutable binding. If not, freeze `#=` and forbid new
non-marker `#` uses.

Objection: `if` as both branch and multi-arm dispatch hides a match construct.

Answer: one keyword reduces vocabulary, but multi-arm errors must be exceptional.
If users struggle, reopen `when` as the readability option. Jai-like consistency
does not always mean fewer keywords; it means each form is obvious.

Objection: `++/--` adds a second increment path.

Answer: yes. Owner ratified C-style forms. Contain the damage: no user overloads,
integer lvalues only, index targets rejected, diagnostics teach `+= 1` when the
operator cannot apply.

Objection: `T.{}` and `.Variant` look odd.

Answer: the dot family is defensible: dot selects a member or constructs through
an expected type. It helps LSP completion. Keep it; explain it as one rule.

Objection: `~` is physically bad.

Answer: keep it for expert edit capability only. Do not expand `~` to stored refs
or metadata.

## New Tower ballots raised

From the live Ideas binder:

- `D-METHODMACRO1`: keep methods ordinary; no macro-method rewrite model.
- `D-MARKER-FAMILY1`: `#` for metadata, `$` only splice, avoid new `@`.
- `D-CLIFLAG1`: typed CLI parser vs Go-style flags.
- `D-ANY-JAI1`: no general Jai-style `Any`; reserve reflection handles.
- `D-DYNARRAY1`: keep `[T]` / `[T#N]`; reject `[..]$T`.
- `D-SHIFT1`: parser cursor methods instead of a shift operator.
- `D-POINTERCHAIN1`: explicit unsafe cast + postfix deref; no Jai chain syntax.
- `D-UNINIT-SENTINEL1`: `#Uninit`, not `---`.
- `D-REF-SHORTHAND1`: keep `#Ref(Label)`; do not overload `~T`.
- `D-BUILDENTRY1`: build-time main shape.
- `D-BUILDPOLICY1`: enterprise build authority defaults.

## Additional ballot candidates

- `D-BIND4`: reconsider immutable binding `#=` vs `::`.
- `D-CASING2`: acronym casing in type names.
- `D-LABEL3`: loop label spelling; avoid `@` expansion unless labels own it.
- `D-FANOUT3`: review `f.[...]` after real use.
- `D-IFMATCH1`: review multi-arm `if` versus `when`.
- `D-DOCDEBT1`: split current syntax from historical ratification notes so
  docs show the live language first.

## Recommended next move

Do not churn syntax broadly. Lock the taxonomy first:

1. Ratify `D-MARKER-FAMILY1`.
2. Keep `$` narrow and `comptime` keyword-owned.
3. Keep `#Ref`, `#Uninit`, `#Build`, `#Unsafe`, and `#Impure` as marker-family
   expert gates.
4. Use the build-entrypoint work to import Jai's power safely.
5. Reopen `#=` only if the owner wants one deliberate cleanup wave, not as a
   side effect of other work.
