# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file (enforced — a card missing any of these is
not ballot-ready; Tower v2 Focus Mode renders these as labeled facets, so use the
exact bold labels):** every full decision card carries `**Gist:**` (one VERY short
plain sentence — the headline), `**Story.**` (a real person with an
American-traditional name and what they're doing), `**In the wild:**` (a fenced
```jet block of realistic project code where this bites), `**Other languages:**`
(short fenced blocks for Rust/TS/Swift/etc. when a cross-language compare helps),
`**Tradeoffs:**` (a compact table, one row per option, columns that actually differ —
subagent-reviewed), and a **worked example of every option** (each
`- **Option X — <name>.**` bullet with its own fenced ```jet/```shell block; mark the
recommended one `(recommended)`). Close with `**Recommendation:**` + a one-line why.
Put Owner Q&A in `**Owner Q …**` blocks — Tower routes those to a separate Q&A facet,
so keep them out of the recommendation. Decisions not yet drafted to that bar are
listed below as one-liners with a recommendation; expand one into a full card when
it's time to decide it.

---

## Open decisions

These cards surface the **naming/spelling layer** that ratified D-codes blessed in
*principle* but left for an agent to fill into a plan as an assumption. Each is a real
user-facing choice the owner never made. Queued 2026-06-24 (missing-decision audit).
Cards A1–A7 are genuine open decisions; the three "reconcile" cards (D-NOSTD1/D-JSONVERB1/
D-NUMOPS2) plus the two **Clarification ratifications** at the bottom are corrections of
plan assumptions that contradict an already-ratified rule.

---

### D-DBG3 — Debugger interactive command surface

**Gist:** Pick the one in-session style for the `jet debug` prompt, its step commands, and the breakpoint/locals text a user reads at every pause.

**Story.** Pearl, learning Jet, hits a breakpoint in a loop. She faces a prompt and has to guess what to type to advance one line, and read a `locals:` dump to see why her counter is wrong. D-DBG1 ratified only the launch verb `jet debug <file>`; D-DBG2 only the `--raw-frames` expert flag. The interactive vocabulary, prompt string, and frame/locals layout were never decided — `dap-debugger.md:42-49,64` quietly bakes in `(jet-dbg)`, `step`/`next`/`continue`/`stepIn`/`stepOut`, and a one-line `locals:` format. This decides all three as one coherent style (they're read together every session).

**In the wild:**
```jet
fn main() {
    total := 0
    loop i in 1..6 {
        total += i        // breakpoint set here
    }
    print("sum {total}")
}
```

**Other languages:** lldb uses `s`/`n`/`c`/`finish` at `(lldb)`; gdb mirrors it. Both assume debugger fluency; no mainstream debugger spells the words out for beginners — that space is open for Jet.

**Frame text stays I2-safe in every option:** the prompt only ever shows Jet files, Jet line numbers, and safe Jet locals (D-OBS2). A frame with no Jet line is stepped over transparently (`dap-debugger.md:93-96`). The choice here is the *words and layout*, not what's behind them.

**Tradeoffs:**
| Option | Step words | Prompt | Aliases | Beginner-readable | Expert speed |
|--------|---|---|---|---|---|
| A — debugger-familiar | `step`/`next`/`continue`/`finish` | `(jet)` | `s`/`n`/`c`/`f` | good | high |
| B — spelled-out | `step-into`/`step-over`/`resume` | `jet debug ▸` | none | best | low (more typing) |
| C — single-letter | `s`/`n`/`c`/`b` | `(jet)` | words are the aliases | poor | highest |

**Options:**
- **Option A — debugger-familiar (recommended).** Full words `step`/`next`/`continue`/`finish` + single-letter aliases `s`/`n`/`c`/`f`; prompt `(jet)`; `help` lists everything.
  ```shell
  $ jet debug debug_sum.jet
  breakpoint hit  debug_sum.jet:4  in main()
     3 |   loop i in 1..6 {
     4 |     total += i              <- here
     5 |   }
  locals:  total = 0   i = 1
  (jet) next
     5 |   }
  locals:  total = 1   i = 1
  (jet) help
  step (s) into   next (n) over   continue (c)   finish (f) out   break <line>   list   quit
  ```
- **Option B — spelled-out beginner-first.** Hyphenated `step-into`/`step-over`/`resume`, no aliases, prompt `jet debug ▸`. Self-describing; costs experts keystrokes every step.
  ```shell
  $ jet debug debug_sum.jet
  breakpoint hit at debug_sum.jet line 4, in main()
        3    loop i in 1..6 {
     →  4      total += i
     locals
        total = 0
        i     = 1
  jet debug ▸ step-over
  ```
- **Option C — terse single-letter primary.** `s`/`n`/`c`/`b` are canonical; full names are the alias. Fast for experts, opaque for a first-timer.
  ```shell
  $ jet debug debug_sum.jet
  brk debug_sum.jet:4 main()
   4| total += i      <-
   loc: total=0 i=1
  (jet) n
  ```

**Recommendation:** Option A — serves both tiers: readable words + `<- here`/`locals:` layout for beginners, `s`/`n`/`c`/`f` + `(jet)` muscle memory for experts. One surface that needs no second style later (I8).

**Owner Q — single-letter aliases in v1?** Ship `s`/`n`/`c`/`f` from day one, or ship only full words first and add aliases once the verb list settles? Recommend shipping aliases — they're the expert half of A's pitch and lldb's letters are safe to copy.

---

### D-LINALG1 — Linear-algebra type & method names

**Gist:** Decide the user-typed type and method names for `jet.linalg` (`Vec3`/`Matrix`/`.dot`/`.matmul`).

**Story.** Walter, a game-engine programmer porting his renderer, opens `jet.linalg` to build a view matrix and a lighting dot-product. D-MATHLIB1 (`syntax-decisions.md:2190`) blessed the package's existence but not one name he'll type — `Vec3`/`Matrix`/`dot`/`cross`/`matmul` are an agent assumption (`math-linalg-simd.md:49-54`), never balloted.

**Other languages:** glam (Rust gamedev) `Vec3::new`, `a.dot(b)`, `Mat4`, `a * b`; nalgebra `Vector3`/`Matrix4`; GLSL/Unity `vec3`/`Vector3`, `mat4`, `dot(a,b)`.

**Tradeoffs:**
| Option | Type names | Method spelling | Scales past fixed dims | Reads native to |
|--------|---|---|---|---|
| A — fixed-dim family | `Vec2/3/4`, `Mat3/4` | `.dot` `.cross` `.matmul` | no | graphics/game devs |
| B — spelled-out | `Vector3`, `Matrix4` | `.dot` `.cross` | no | readers/scientists |
| C — generic sized | `Vec<3>`, `Matrix<3,3>` | `.dot` `.matmul` | yes (N-dim) | expert/numerics |
| D — operator-forward | A's types + `+ - *` ops | ops + `.dot`/`.matmul` | follows host | math-heavy code |

**Options:**
- **Option A — fixed-dim named family (recommended).** `Vec2/3/4`, `Mat3/4`; `.dot()`, `.cross()`, `.matmul()`. Mirrors GLSL/glam/Unity.
  ```jet
  use jet.linalg as la
  fn main() {
      a @= la.Mat2 {r0: la.Vec2 {x: 1.0, y: 2.0}, r1: la.Vec2 {x: 3.0, y: 4.0}}
      b @= la.Mat2 {r0: la.Vec2 {x: 5.0, y: 6.0}, r1: la.Vec2 {x: 7.0, y: 8.0}}
      print(a.matmul(b).row(0).x)          // 19.0
      light @= la.Vec3 {x: 0.0, y: 1.0, z: 0.0}
      print(light.dot(la.Vec3 {x: 0.0, y: 0.7, z: 0.7}))   // 0.7
  }
  ```
- **Option B — spelled-out.** `Vector3`/`Matrix4`, same methods. Most readable, most keystrokes; diverges from the graphics dialect.
  ```jet
  light @= la.Vector3 {x: 0.0, y: 1.0, z: 0.0}
  print(light.dot(surface))                // 0.7
  ```
- **Option C — generic comptime-sized.** `Vec<3>`/`Matrix<3,3>` riding D-FIXARR1/S76; `Vec3` etc. are aliases. One type scales to any N; heavier signatures.
  ```jet
  a @= la.Matrix<2,2>(1.0, 2.0, 3.0, 4.0)
  light @= la.Vec3 {x: 0.0, y: 1.0, z: 0.0}   // alias for Vec<3>
  ```
- **Option D — operator-forward.** A's type names, common ops overloaded (`a * 2.0` scale, `a + b`); `.dot`/`.matmul`/`.cross` stay named (no ASCII `·`/`×`). Raises the operator-overloading question.
  ```jet
  scaled @= a * 2.0           // elementwise; matmul stays a.matmul(b)
  ```

**Recommendation:** Option A names, **with C's `Vec<N>`/`Matrix<M,N>` as the underlying generic** and A as aliases over it. Beginners/graphics devs get the short familiar names; the expert reaching for 7-dim uses the generic. One implementation, two reading levels (I8). *Note: the const-generic `<N>` spelling is not yet ratified (today's fixed sizes use `[T#N]`, S76) — picking C also blesses value args in `<…>`, so that spelling is part of this decision, not an assumption.*

**Owner Q — `·`/`×` sigils:** the mathematician's `a · b`/`a × b` are non-ASCII. All options keep `.dot`/`.cross`. Introducing `·`/`×` operators is a separate sigil decision — flag if you want it queued.

---

### D-SIMD2 — SIMD lane construction & access surface

**Gist:** Decide how a user builds a SIMD lane vector, reads one lane, and spells lane ops (`F32x4.splat`/`v[0]`/`+` vs `.lane`/`.add`).

**Story.** Dolores, writing a vectorized audio mixer, reaches for `F32x4` to add two 4-lane buffers and take a horizontal sum. D-SIMD1 (`syntax-decisions.md:2196`) gave her the type names and safe-by-default ops, but spelled neither how she constructs a lane vector nor how she reads a lane. The `simd_kernel.jet` example assumes an API never decided (`math-linalg-simd.md:33,57-59`).

**Other languages:** `std::simd` `f32x4::splat`, `from_array`, `a + b`, `v[0]`, `reduce_sum`; Zig `@Vector(4,f32)`, `a + b`, `v[0]`; Swift `SIMD4<Float>(1,2,3,4)`, `a + b`, `v.sum()`.

**Tradeoffs:**
| Option | Build | Lane read | Op spelling | Ties to `[T#N]` |
|--------|---|---|---|---|
| A — constructor + index | `F32x4(1,2,3,4)`, `.splat(0)` | `v[0]` | `a + b` | via widen |
| B — array adapter | `[F32#4].simd()` | `v.lane(0)` | `v.add(w)` | directly |
| C — method-only | `F32x4.of(1,2,3,4)` | `v.get(0)` | `v.mul(w)` | via `.of` |

**Options:**
- **Option A — constructor + index (recommended).** `F32x4(1,2,3,4)`/`F32x4.splat(0.0)`; lane read `v[0]`; ops via `+`/`*`.
  ```jet
  fn main() {
      a @= F32x4(1.0, 2.0, 3.0, 4.0)
      b @= F32x4(10.0, 20.0, 30.0, 40.0)
      sum @= a + b                         // 11,22,33,44
      print(sum[0] + sum[1] + sum[2] + sum[3])   // 110.0
  }
  ```
- **Option B — adapter over fixed arrays.** Build from `[F32#4]` via `.simd()`; lane read `v.lane(0)`; method ops. Cleanest tie to D-FIXARR1; more verbose.
  ```jet
  a: [F32#4] @= [1.0, 2.0, 3.0, 4.0]
  sum @= a.simd().add(b.simd())
  print(sum.lane(0) + sum.lane(1) + sum.lane(2) + sum.lane(3))
  ```
- **Option C — explicit method-only.** `F32x4.of(1,2,3,4)`; lane read `v.get(0)`; named ops `v.add(w)`. Most explicit, most verbose; no operator question to settle.
  ```jet
  sum @= F32x4.of(1.0, 2.0, 3.0, 4.0).add(b)
  print(sum.get(0))
  ```

**Recommendation:** Option A — `splat` + tuple constructor + `v[0]` + `+`/`*` reads like the math, safe-by-default per D-SIMD1. Keep **B's `[F32#4]` interop as the named bridge** (`arr.simd()`/`v.to_array()`) so lane vectors round-trip with the ratified fixed-array type.

**Owner Q — operator overloading on lane types?** Option A makes `+`/`*` work on `F32x4`. Jet has no general operator-overloading stance; blessing it here (even built-in lane types only) sets precedent. OK for built-in SIMD types only, or hold to named methods (C) until a broader decision? Same question gates D-LINALG1 Option D.

---

### D-SUPPLY1 — Supply-chain command surface (vendor / audit / SBOM)

**Gist:** Decide the user-facing verbs and flags for vendoring deps, auditing for advisories, and emitting an SBOM.

**Story.** Walter, a release engineer, is wiring an airgapped CI job: pull every locked dependency into the repo, scan for advisories before publishing, and hand compliance a machine-readable bill of materials. None of those three verbs has a ratified spelling. The plan `package-ecosystem-trust.md:116-150` mints `jet vendor`, `jet audit`, `--sbom`, `--vendor-dir` as if ratified, but the only authority is a roadmap label "D-PKGS1" that **is never defined** in `syntax-decisions.md`. The parent D-PKGSIGN1 (`:2418`) ratified only signing/checksum — not these verbs.

**Tradeoffs:**
| Option | Top-level verbs added | Discoverable in `jet --help` | Scriptable in CI |
|--------|---|---|---|
| A — dedicated verbs + flags | 2 (`vendor`, `audit`) + flags | yes, flat | yes |
| B — one `supply` umbrella | 1 (`supply`) | grouped | yes, more typing |
| C — manifest-driven | 1 (`audit`) | minimal | weakest |

**Options:**
- **Option A — dedicated verbs + flags (recommended).** Each task its own top-level verb, mirroring `jet test`/`jet debug`; SBOM rides as a flag on build/publish.
  ```shell
  $ jet vendor --vendor-dir vendor/
  vendored 2 packages into vendor/ (acme/billing@1.7.2, core.http@3.0.1)
  $ jet audit
  acme/billing  1.7.2  CRITICAL  JSA-2026-0044  auth bypass in token parse
  1 critical advisory found      # exit status 1
  $ jet build --sbom
  wrote target/checkout-service.spdx
  ```
- **Option B — one `supply` umbrella.** `jet supply vendor` / `audit` / `sbom`. Fewer top-level verbs (I8), more typing.
  ```shell
  $ jet supply audit
  acme/billing 1.7.2  CRITICAL  JSA-2026-0044
  ```
- **Option C — manifest-driven + minimal verbs.** Vendoring/SBOM become `pack.jet` settings; only `jet audit` stays a verb. Smallest CLI surface, least scriptable.
  ```jet
  // pack.jet (fields illustrative): vendor: true / sbom: true on every build
  ```

**Recommendation:** Option A — flat, discoverable verbs mirroring the existing surface, scriptable for CI (nonzero exit on CRITICAL), SBOM-as-flag keeps the common build path clean. Pick B only if holding down top-level verb count (I8) outranks discoverability. Supporting diagnostics: **E1204** (store tamper, `diagnostics.md:325`) backs `vendor` integrity today; **E1217**/**E1218** are minted by this same plan.

**Owner Q — SBOM flag home.** `--sbom` on both `jet build` and `jet publish`, or only `build` (publish always emits to the registry index)? Defaulting to "flag on `build`, always-on for `publish`" unless you want it suppressible.

---

### D-TXN3 — Deferred post-commit effects (`on_commit`)

**Gist:** Decide how a `#Transact { }` block schedules an irreversible effect (email, network call) to run only after the transaction commits.

**Story.** Dolores, a backend dev, writes an order handler: inside `#Transact { }` she updates the DB, but also needs a confirmation email — which can't be rolled back, so D-TXN2 forbids it inside the block. She needs it to fire *only if* the transaction commits, written next to the DB work. D-TXN2's fix-it string (`syntax-decisions.md:2472`) names `on_commit { }` as the escape hatch — but that construct was never balloted: no keyword, scoping, or ordering semantics. Meanwhile D-DEFER1 already chose a **library** form (`scope.guard(() => {…})`, `67_scope_guard.jet`) over a `defer` keyword. That precedent should govern.

**Tradeoffs:**
| Option | New keyword (I7) | Consistent with D-DEFER1 | Runs only on commit | In-block locality |
|--------|---|---|---|---|
| A — `scope.on_commit(…)` | none | yes (library, Drop-backed) | yes | yes |
| B — `#OnCommit { }` block | yes | no (2nd cleanup spelling) | yes | yes |
| C — no construct | none | n/a | n/a | no (move after block) |

**Options:**
- **Option A — library registration (recommended).** `scope.on_commit(() => {…})` inside the block stores the lambda; runs only after a clean commit, dropped on rollback. Same shape as ratified `scope.guard`.
  ```jet
  fn place_order(order: Order) -> OrderId ? OrderErr {
      #Transact {
          id @= db.insert("orders", order)?
          db.decrement_stock(order.sku, order.qty)?
          scope.on_commit(() => { mail.send(order.email, "Order confirmed") })
          return ok(id)
      }
  }
  ```
- **Option B — `#OnCommit { }` block.** A nested PascalCase block (D-CASING1) whose body runs post-commit. Reads clearly; a second cleanup spelling alongside `scope.guard`.
  ```jet
  #Transact {
      id @= db.insert("orders", order)?
      #OnCommit { mail.send(order.email, "Order confirmed") }
      return ok(id)
  }
  ```
- **Option C — no construct.** Drop `on_commit`; move the effect after the block. Simplest (I8); loses in-transaction locality.
  ```jet
  id @= #Transact { /* … */ return ok(inner) }?
  mail.send(order.email, "Order confirmed")   // only if commit succeeded
  ```

**Recommendation:** Option A — reuses the ratified D-DEFER1 `scope.*` model (no new keyword, I7 untouched), same Drop-backed lowering, keeps intent local. Whichever wins, the **D-TXN2 fix-it string at `syntax-decisions.md:2472`** must be rewritten to match.

---

### D-SERDE2 — Serde hand-impl names (method, value-tree type, error)

**Gist:** Name the trait methods, abstract value-tree type, and error type a user types when hand-writing a serializer (the non-derive path that ships now).

**Story.** Walter has a `Point` struct and no derives yet (S56 is Epoch 3). To get JSON out today he hand-writes the serialize impl, so he types the value-tree type, its variants, the method, and the error by hand — names D-SERDE1 never balloted (`serde-model.md:81-93,171-180`).

**Anchor — what Core already calls this:** the shipped JSON tree (`Source/Prelude/CoreLib.rs:40`) is type `JSON` with variants `.Text/.Boolean/.Number/.Object/.Array/.Null`; verbs `json.parse`/`json.render`. The abstract tree should rhyme with that.

**Tradeoffs:**
| Option | Method verbs | Tree type + variants | Error | Consistency w/ Core `JSON` |
|--------|---|---|---|---|
| A — Jet-short | `to_data`/`from_data` | `DataValue` · `.Map/.Seq/.Int/.Float/.Text/.Bytes/.Bool/.Null` | `SerdeError` | partial |
| B — serde-rs familiar | `serialize`/`deserialize` | `Value` · `.Null/.Bool/.Number/.Text/.Seq/.Map` | `DecodeError` | weak (`Value` too generic) |
| C — plain-English | `encode`/`decode` | `DataNode` | `SerdeFault` | clashes (`decode` = `json.decode`) |

**Options:**
- **Option A — Jet-short (recommended).** `to_data`/`from_data`; `DataValue`; `SerdeError`; variants aligned to Core's `JSON`.
  ```jet
  struct Point { x: Int, y: Int }
  impl Point: Serialize {
      fn to_data(self) -> DataValue {
          return DataValue.Map([("x", DataValue.Int(self.x)), ("y", DataValue.Int(self.y))])
      }
  }
  // json.render(p.to_data())  ->  {"x":1,"y":2}
  ```
- **Option B — serde-rs familiar.** `serialize`/`deserialize`; `Value`; `DecodeError`. Lowest surprise for Rust refugees, but `Value` is too generic for a Core public type.
  ```jet
  impl Point: Serialize { fn serialize(self) -> Value { return Value.Map([…]) } }
  ```
- **Option C — plain-English.** `encode`/`decode`; `DataNode`; `SerdeFault`. Rejected: `decode` already names the lenient JSON entry point (`json.decode`, spec:954).

**Recommendation:** Option A — short teachable verb pair, self-explaining type, `SerdeError` matches the model name, variants echo Core's `JSON.Text`, no clash with `json.decode`.

**Owner Q — variant spelling alignment:** Core's `JSON` spells scalars `.Boolean/.Number/.Text`; the abstract tree above uses shorter `.Bool/.Int/.Float/.Text`. Identical to `JSON`, or shorter on the lower type? (Recommend short — `.Int` vs `.Float` is a real distinction `JSON` collapses into `.Number`.)

---

### D-SERDE3 — `rename_all` casing-style menu

**Gist:** Decide the set — and the typed-vs-stringly spelling — of casing styles in `#[rename_all(...)]`.

**Story.** Dolores ships a JSON API. Her Jet fields are `snake_case` (house style), but the wire contract is `camelCase`. She reaches for `rename_all` (D-SERDE1 ratified the attribute name) and must type the style — but which spelling, from which menu, is unratified (`serde-model.md:124,129`; E2409 fires on anything off-menu).

**Other languages:** serde-rs `#[serde(rename_all = "camelCase")]` — magic string, full set incl. `SCREAMING_SNAKE_CASE`, `kebab-case`.

**Tradeoffs:**
| Option | Form | Accepted menu | Typed vs stringly |
|--------|---|---|---|
| A — serde full strings | `rename_all("camelCase")` | 5 styles | stringly |
| B — curated strings | `rename_all("camelCase")` | camel/snake/pascal | stringly |
| C — typed keyword | `rename_all(camel)` | camel/snake/pascal | typed, tab-completable |

**Options:**
- **Option A — serde full string set.** All five styles. Familiar; magic string, typo-prone; ships two styles JSON/CSV almost never use.
  ```jet
  #[Serialize, rename_all("camelCase")]
  struct UserAccount { first_name: String }   // {"firstName": …}
  ```
- **Option B — curated string subset.** Only `"camelCase"`/`"snake_case"`/`"PascalCase"`. Simpler menu (I8); still a magic string.
  ```jet
  #[Serialize, rename_all("camelCase")]
  struct UserAccount { first_name: String }
  ```
- **Option C — typed keyword arg (recommended).** `#[rename_all(camel)]`/`(snake)`/`(pascal)` — a closed keyword vocabulary, tab-completable in the LSP, no quoting, E2409 shows the full closed list.
  ```jet
  #[Serialize, rename_all(camel)]
  struct UserAccount { first_name: String }   // {"firstName": …}
  ```

**Recommendation:** Option C — the owner has repeatedly chosen typed values over magic strings; a closed keyword set gives beginners LSP completion and a self-contained error menu, and stays minimal (I8 — add `screaming`/`kebab` only when a wire format proves it needs one). If Rust-refugee familiarity outweighs the typed-pin win, fall back to **B** over A.

**Owner Q — keyword vocabulary, if C:** short `camel`/`snake`/`pascal`, or fuller `camel_case`/`snake_case`/`pascal_case`? (Recommend short — `rename_all` already supplies "case" context.)

---

### D-NOSTD1 — `no_std`/freestanding as a user-facing manifest field? *(reconcile)*

**Gist:** Should `pack.jet` get a real `no_std: true` opt-in, or does freestanding stay an internal build mode only?

**Story.** Walter, a firmware engineer, wants to ship a sensor loop with no stdlib and opens `pack.jet` to declare it — the freestanding flagship slice (`flagship-vertical-slices.md:82`) told him to write `no_std: true`. But `philosophy.md:127` lists `no_std`/sub-std as **not pursued in v1**; `:118` says we accept the std baseline. The plan assumed a manifest field the stated v1 stance rules out.

**Tradeoffs:**
| Option | v1 scope | User surface | Demo still possible? |
|--------|---|---|---|
| A — drop the field | honors `philosophy.md:127` | none (internal build mode) | yes, via internal flag |
| B — ratify the field | reverses the stance | `no_std: true` opt-in | yes, plus an opt-in |

**Options:**
- **Option A — drop the field, freestanding stays internal (recommended).** No user-facing manifest opt-in in v1; the showcase compiles under an internal build mode.
  ```jet
  // pack.jet — unchanged, no special field. The freestanding DEMO still ships,
  // built with an internal flag, proving @unsafe + #Layout(c) + [U8#N] + no-alloc.
  ```
- **Option B — ratify the field, reverse the v1 stance.** Add `no_std:`/`freestanding:`/`core_only:`, accepting v1 now supports a sub-std target.
  ```jet
  // pack.jet (field illustrative)
  no_std: true   // opts the whole package out of the std baseline
  ```

**Recommendation:** Option A — `philosophy.md:127` already settled `no_std` out of v1; a manifest field would silently reverse a ranked decision and add surface for a capability we don't commit to (I8). The demo loses nothing.

---

### D-JSONVERB1 — struct→JSON verb name (`json.render` vs `json.to_string`) *(reconcile)*

**Gist:** One spelling for value→JSON string — keep ratified `json.render`, don't add `json.to_string`.

**Story.** Dolores serializes a struct to send over the wire. The shipped example (`30_json.jet:5`) and spec (`spec.md:953`) say `json.render(data)`. The serde plan (`serde-model.md:179`) invents a third spelling `json.to_string(p)` for the identical operation — two names for one verb.

**Options:**
- **Option A — keep ratified `json.render`, drop `to_string` (recommended).** One verb; the plan example is the drift and gets fixed.
  ```jet
  #[Serialize]
  struct Point { x: Int, y: Int }
  fn main() {
      p @= Point {x: 1, y: 2}
      print(json.render(p))         // {"x":1,"y":2}
      print(json.render_pretty(p))  // multi-line, already ratified
  }
  ```
- **Option B — add `json.to_string` as a distinct verb.** Only if there's a real semantic difference worth a second name.
  ```jet
  print(json.to_string(p))    // same bytes as render(p)
  ```

**Recommendation:** Option A — D-JSONOUT1 ratified `json.render`; a synonym producing identical output is the redundant surface I8 rejects. Confirm `render`, fix the plan line.

---

### D-NUMOPS2 — overflow default for sized & unsigned integers *(reconcile)*

**Gist:** Do `U8`/`I16`/etc. trap on overflow like `Int` (D-NUMOPS1), or do unsigned/sized types silently wrap?

**Story.** Hank, writing byte-level packet math, increments a `U8` holding `255`. The sized-ints plan (`dsg9-sized-integers-impl.md:23`) says it will "document `U8` wrap/overflow behaviour" — phrased as if `U8` might **wrap** to `0`. But D-NUMOPS1 (`syntax-decisions.md:2285`) ratified **trap-on-overflow by default**, with `wrapping(…)`/`saturating(…)`/`checked(…)` as the visible opt-ins. The plan reads as a divergent default the ratified rule doesn't grant.

**Other languages:** Rust debug panics, release wraps; C: unsigned wraps by spec, signed is UB.

**Options:**
- **Option A — every width inherits the D-NUMOPS1 trap default (recommended).** `U8`/`I16`/… trap exactly like `Int`; opt-ins are the only way to wrap.
  ```jet
  b: U8 := 255
  b = b + 1            // TRAP: U8 overflow
  b = wrapping(b + 1)  // 0 — wrap is opt-in, visible at the use site
  ```
- **Option B — unsigned/sized types wrap (C-like) by default.** Matches C/Rust-release intuition for byte math; a silent divergent default.
  ```jet
  b2: U8 := 255
  b2 = b2 + 1          // 0, silently, no signal
  ```

**Recommendation:** Option A — one overflow rule for all widths is simplest to teach (I8) and keeps the safe-by-default trap rail; a width-dependent silent default is the "no silent bugs" footgun philosophy rejects. The plan should document that `U8` *traps* and `wrapping(…)` gives the C behavior.

---

## Value-tag application surface — board card c62

### D-QUAL3 — How a unit-tagged number is written in a type annotation

**Gist:** D-UNIT1 ratified the unit *tag* `#Unit(usd)`, the `#UnitFamily` declaration, the `9.99.usd` literal, and the mismatch errors — but never how you write the *type* of a dollar amount in a signature. Pick that one spelling. (Single pick; coercion and the plain-marker-tag case are out of scope — see below.)

**Story.** Della is porting an invoicing module to Jet. `9.99.usd` literals already work and unit-mismatched arithmetic already errors (E0128 unit-vs-unit, E0129 unit-vs-bare). But the moment she writes a function — `fn subtotal(price: ???, qty: Int) -> ???` — she has to name the *type* of a US-dollar amount, and no ratified decision says how that `???` is spelled.

**In the wild:**
```jet
#UnitFamily(currency) { usd, eur, gbp }    // D-UNIT1, ratified

fn subtotal(price: ???, qty: Int) -> ??? {  // <- D-QUAL3 decides the `???`
    price * qty                              // unit-matching arithmetic already pinned
}

let p = 9.99.usd        // literal carries the unit (ratified)
let s = subtotal(p, 3)  // s is a usd amount; how is that type written down?
```

**Other languages:** F# units-of-measure annotate as `float<usd>` (angle param on the base type). Rust uses a newtype — you annotate a distinct `Usd` (`struct Usd(f64)`), no tag surface. Haskell's `tagged` writes `Tagged USD Double` (prefix wrapper).

**Tradeoffs:**

| Option | A usd amount is written | Sigil in everyday signatures | Matches an existing Jet idiom | Beginner readability |
|---|---|---|---|---|
| A — postfix marker | `Float #Unit(usd)` | yes, every annotation | no (Jet markers all prefix) | medium |
| B — prefix marker | `#Unit(usd) Float` | yes, every annotation | yes (`#Test fn`, `#Numeric distinct`) | medium |
| C — family mints a type | `Usd` | none | yes (D-DIST2 distinct types) | high — reads like English |
| D — angle param | `Float<usd>` | yes, but light | partial (generics already use `<>`) | medium-high (F#-familiar) |

- **Option A — postfix marker `Float #Unit(usd)`.** The tag rides the base type at the end.
  ```jet
  fn subtotal(price: Float #Unit(usd), qty: Int) -> Float #Unit(usd) { price * qty }
  ```
- **Option B — prefix marker `#Unit(usd) Float`.** The marker leads, like every other Jet `#Marker`.
  ```jet
  fn subtotal(price: #Unit(usd) Float, qty: Int) -> #Unit(usd) Float { price * qty }
  ```
- **Option C — the family mints a named type; annotate the name (recommended).** `#UnitFamily(currency) { usd, eur, gbp }` mints one distinct type per member (`usd`→`Usd`), so signatures are plain type names and the `#Unit` machinery stays in the family declaration. This is D-UNIT1's own framing — "the upgrade to D-DIST2," the hand-written `distinct` newtype as the fallback.
  ```jet
  #UnitFamily(currency) { usd, eur, gbp }   // mints Usd, Eur, Gbp (distinct, erase to Float)
  fn subtotal(price: Usd, qty: Int) -> Usd { price * qty }
  ```
- **Option D — angle param `Float<usd>` (F#-style).** The unit is a bracketed parameter on the base numeric type.
  ```jet
  fn subtotal(price: Float<usd>, qty: Int) -> Float<usd> { price * qty }
  ```

**Recommendation:** Option C. It makes signatures read like plain English (`price: Usd`), honors D-UNIT1's explicit "upgrade to D-DIST2" intent, keeps the `#Unit` sigil out of everyday code, and reuses the distinct-type machinery already shipped — the `#Unit`/`.usd`/arithmetic surface from D-UNIT1 is unchanged underneath. This unblocks c68. Coercion is already pinned by D-UNIT1 (unit-vs-bare E0129; `.raw()` strips), so this card decides only the annotation spelling. *(Drafted 2026-06-24 after the c62 tag foundation shipped; agent-reviewed — split from the original plain-tag axis, which is deferred as D-QUAL4 below; F# option added.)*

---

## Clarification ratifications — confirm the correction (no vote; pure drift vs a ratified rule)

**C-CASING — Tag casing in plans must reconcile to D-CASING1.** `units-tag.md:14` writes `#unit(usd)`, `transact-rollback-semantics.md` writes `#transact`, `c71-typestate-impl.md` writes `#no_copy`. D-CASING1 (`syntax-decisions.md:2040`, ratified, owner-directed) makes all tags PascalCase. **Correction:** reconcile the plans to `#Unit(usd)`, `#Transact`, `#NoCopy`. Nothing user-facing changes.

**C-MANIFEST — `pkg.jet` references must reconcile to `pack.jet`.** `package-ecosystem-trust.md:99` and `flagship-vertical-slices.md` (lines 17/31/58/82/107/127/153/155) write `pkg.jet`; the ratified manifest filename is **`pack.jet`** — a clean break, no alias (`syntax-decisions.md:790`). **Correction:** reconcile all `pkg.jet` references to `pack.jet`.

---

> **Drained 2026-06-24.** The owner ratified the last two open cards: **D-BENCH1 = A**
> (`#Bench "name" { … }` region-benchmark block, sibling of `#Test`, run by the existing
> `jet bench` verb) and **D-PKGSIGN1 = B + A opt-in** (SHA-256 checksum is the always-on
> integrity floor; Ed25519 author signing is an opt-in, non-blocking layer — `require_signed`
> off by default). Both recorded in `syntax-decisions.md`, cards stripped, plans unblocked
> (epoch-3/testing-docs-ergonomics.md §4; sidequests/package-ecosystem-trust.md §4).

---

> **Memory-model gate CLOSED — ratified 2026-06-23.** The owner decided all three gate
> cards: **D-CAP8 = C** (infer in bodies, freeze at `api: explicit`), **D-CAP9 = D** (`*x`
> = raw-of, dereference becomes postfix `p.*`, `*T` replaces `Ptr<T>`), **D-CAP10 = A**
> (overloads out of scope; call-site-sigil disambiguation on a single definition). Recorded
> in `syntax-decisions.md`; cards stripped. The whole access-capability model
> (`docs/prompt-memory-model-final.md`) is now unblocked — see
> `docs/research/memory-model-implementation-plan.md` for the build order.

---

> **Drained 2026-06-22.** The owner's 2026-06-22 batch ratified every open full card —
> D-UNSAFE2, D-FIXARR1, D-CAP2/3, D-EFF2/3, D-MIGRATE2A/B/C/D/E/F, D-JSONOUT1, D-ARGS1,
> D-MATHLIB1, D-SIMD1, D-REACT1, D-FANOUT2, D-STRPARSE1, D-CTCORE1, D-JIT1, D-HOTSWAP1,
> D-DEVMODE1, D-SOA2A/B/C/D, D-TEST1, D-TEST4, D-BIND2, D-NUMOPS1, D-SERDE1, D-ITER1 (plus
> the earlier batch D-EFF1/D-QUAL1/D-TXN1/D-MIGRATE1/D-SOA1 and D-DBG2). All are recorded
> in `syntax-decisions.md` and their cards stripped from this file. The effect-system
> surface is now fully decided (D-EFF1+D-QUAL1+D-EFF2+D-EFF3). **D-MUTSELF1** (self-mutation
> in `mut self` methods) was opened and ratified 2026-06-23 (option A) — recorded in
> `syntax-decisions.md`, card stripped. The memory-model gate (D-CAP8/9/10) was opened and
> ratified 2026-06-23 — see the note above. **No full decision cards remain open.** What's left
> below is informational only: the **deferred-ballots list**
> (stubs to promote when their prerequisites land), the **B6 `defer`** note, and the
> **Coverage / D-COV1** tooling note. Cards **c25** (range sugar) and **c55** (REPL v2) are
> implement-only. Submitting a decision records it in `syntax-decisions.md` and removes it
> from this file.

---

## Deferred ballots — promote when reached

The items below are not ready for owner decision. Each has a real user story
and a clear reason to wait. Promote a stub to a full card when its
prerequisite is ratified or its milestone is reached.

---

**D-PUBLISH1 — `jet publish` command shape + semver/resolver policy (board card c96).**
*User story:* Saoirse cuts a release of her Jet library and Amara pins a semver range to it.
*Decision (when promoted):* the `jet publish` command surface, version-immutability /
re-publish-refusal policy, and the resolver default (highest-compatible vs exact pins +
explicit update; lockfile default). *Why deferred:* rides **c50** (build-from-source) and
**c56** (registry upload) infra, both unverified/soft-blocked on dep approvals. Promote to a
full card with worked `jet publish` shell examples once M12.2 infra is verified.
Rec direction: `jet publish` infers version from `pkg.jet`, refuses re-publish + a dirty
tree, resolver defaults to highest-compatible with a committed lockfile. From the 2026-06-20
persona run (Saoirse, Amara).

---

**D-QUAL4 — Plain marker-tag type-position spelling (prefix vs postfix).**
*User story:* A web dev marks a value `#Tainted` at its source and needs to write
the *type* of a tainted string in a function signature — `flagged: #Tainted String`
vs `String #Tainted`. Same question for `#SingleUse`, `#NoCopy`, and the typestate
markers — the plain (non-parameterized) value-tags that attach to an existing type
rather than minting a new one (so D-QUAL3's "mint a type" Option C doesn't apply).
*Decision (when promoted):* prefix `#Tag Type` (matches every other Jet `#Marker`:
`#Test fn`, `#Numeric distinct`) vs postfix `Type #Tag`. Rec direction: **prefix**, for
one consistent marker idiom. *Why deferred:* no ready consumer — units (c68) ride D-QUAL3
and mint types; the first plain value-tag consumer is taint (D-TAINT1, gated on D-EFF1)
or single-use (D-LIN1, c71). Promote to a full card when c71 or the taint work starts.
Split from D-QUAL3 on 2026-06-24 (a single card can't pick both axes).

---

**D-PROP1 — Effect prohibitions: implicit propagation of `#(no_…)`.**
*User story:* A security engineer wants to know, by reading the root call
site, that a call graph never touches the network — without auditing every
callee. He writes `#(no_net)` on a function and the compiler traces every
reachable call for a net effect, naming the violating path.
*Why deferred:* Rides **D-EFF1** (the effect-propagation engine itself) plus
D-QUAL1's surface (`#(…)`); prohibition is the inverse-lattice follow-on once
positive effects propagate. Sequencing: D-EFF1 → D-PROP1. Board items #24/#4.

---

**D-ROLE1 — Time-varying roles: typestate + time.**
*User story:* A hotel booking system dev wants to express that a `Reservation`
is `#pending` before payment and `#confirmed` after — and that calling
`check_in` on a `#pending` reservation is a compile error.
*Why deferred:* Requires the typestate machinery from **D-STATE1** (gated on
D-QUAL2) to be ratified first; "time-varying" adds a temporal ordering
constraint on top of static typestate, a separate design question. Board item #13.

---

**D-REFINE1 — Refinement types.**
*User story:* A numeric processing library author wants `PositiveInt` to be a
type the compiler can prove is always > 0, so she doesn't pepper every
function with `require(n > 0)`.
*Why deferred:* Refinement types require a proof/SMT layer that is not in the
roadmap for v1; the simplicity ratchet (I8) requires a concrete milestone slot
and owner sign-off before any work begins. Board item #19.

---

**D-BUDGET1 — Budgets as types.**
*User story:* A systems developer writing a real-time renderer wants to express
that `render_frame` has a 16ms CPU budget and have the compiler warn if a
called function is known to exceed it.
*Why deferred:* Requires comptime cost-bound inference, which is not in the
v1 roadmap; no prior-art consensus on how to make it ergonomic without macros
(I8 / no macros). Board item #22.

---

**D-IFC1 — Information-flow and compliance tracking.**
*User story:* A fintech dev wants to annotate a value as `#pii` (personally
identifiable information) and have the compiler refuse to let it flow into a
logging call or a non-encrypted storage write without an explicit sanitize
step — enforced at compile time, not by code review.
*Why deferred:* This is **D-TAINT1 Option B** (full information-flow control —
security-label lattice, principals, `declassify`), which the **owner explicitly
deferred to post-Epoch-3 on 2026-06-21** when ratifying D-TAINT1 Option A
(`#tainted` + sanitizers). Captured here so it is not lost. Generalizes D-TAINT1
and requires the full effect/tag propagation model from D-EFF1 and D-QUAL1 to be
ratified first; the compliance dimension (what counts as a legal sink) is a policy
question that also interacts with the manifest capability model (D-QUAL1 Option A,
manifest surface). Board items #30/#33.

---

**D-REPLAY1 — Opt-in record and replay.**
*User story:* A game developer wants to record a session's inputs, replay
them deterministically to reproduce a bug, and have the compiler ensure no
hidden state (system clock, random, I/O) is read during replay without being
mocked.
*Why deferred:* Requires the effect system (D-EFF1) to tag non-deterministic
effects and a runtime record/replay harness; neither is in the v1 roadmap.
Board item #7.

---

**D-REVERSE1 — Opt-in reversible computation and solver integration.**
*User story:* A constraint-based UI layout author wants to write the forward
constraint (`width = parent.width - padding * 2`) and have Jet automatically
solve for `padding` given a target `width` — without writing the inverse by
hand.
*Why deferred:* Requires a reversibility annotation on functions and a
solver/SMT backend; no prior-art consensus on making this ergonomic without
macros or dependent types. Board item #36.

---

**D-PROTO1 — Protocol and session type generation.**
*User story:* A network protocol implementer wants to declare a
request/response handshake sequence as a type and have the compiler generate
both the client and server stubs, rejecting code that sends messages out of
order.
*Why deferred:* Session types require linear types (used exactly once, in
order) and typestate; **D-LIN1** (linear tag) and **D-STATE1** (typestate),
both gated on D-QUAL2, are prerequisites, and the code-generation surface for
protocol stubs is a separate design. Board item #9.

---

**D-VERIFY1 — Formal verification and proof integration.**
*User story:* A cryptography library author wants to attach a machine-checked
proof that her `constant_time_eq` function runs in time independent of its
inputs, and have the Jet toolchain refuse to ship the library if the proof
doesn't hold.
*Why deferred:* Requires a proof-carrying-code or SMT integration layer that
is explicitly post-v1; the simplicity ratchet (I8) bars this without a
concrete roadmap slot and owner sign-off. Board items #15/#17.

---

## B6 `defer` — already decided, no ballot

`defer` is solved; nothing to vote on. **D-DEFER1 (ratified + implemented 2026-06-20)** shipped `core.scope.guard(() => {…})` — a stdlib value whose `Drop` runs the stored lambda LIFO on every exit path including `?`. `defer`-as-primary stays rejected (S63); the `defer` keyword stays declined (D-SUGAR5).

```jet
use core.scope

fn copy_file(src: String, dst: String) -> () ? Error {
    f :: core.fs.open(src)?
    g1 :: scope.guard(() => { core.fs.close(f) })   // replaces `defer close(f)`
    g :: core.fs.create(dst)?
    g2 :: scope.guard(() => { core.fs.close(g) })   // fires before g1, even on early return
    core.fs.copy(f, g)?
}
```

**Reopen (owner-only):** you could later add `defer expr` as sugar over `scope.guard` (same Drop-backed lowering, zero runtime cost). For: it's the spelling Jai/Go/Swift/Odin/Zig converge on. Against: D-SUGAR5 declined it; it adds a second cleanup spelling and reintroduces Go's leak-by-omission class. No agent reopens this without your instruction.

---

## Coverage — D-COV1 (deferred, no ballot needed)

The epoch-3 plan scopes coverage as "tooling only — no new syntax; couples to the
test runner in `Source/main.rs` (`run_test`)." There is no user-facing surface
decision: `jet test --coverage` is the spelled-out verb and the output format (LCOV
/ HTML / stdout summary) is an implementation choice, not a syntax choice.

**Prior art:**
- **Rust tarpaulin** — `cargo tarpaulin --out Html`; produces HTML + lcov. No new
  Rust syntax. Jet takeaway: a `--coverage` flag on `jet test` is the right shape.
- **llvm-cov / cargo llvm-cov** — output: `--json`, `--lcov`, `--html`, `--text`.
  Jet takeaway: multiple formats are useful but can be deferred to a `--format`
  flag.
- **Python coverage.py** — `coverage run`; then `coverage report` / `coverage html`.
  Two-step. Jet takeaway: a single `jet test --coverage` that prints a summary to
  stdout (and optionally writes a report) is simpler than a two-step model.

**Deferred note:** if coverage ever needs a source annotation (e.g. `// @no_cover`
to exclude a line from the report), that is a syntax decision requiring a ballot.
Until then, coverage is tooling-only and can land without owner ratification. The
implementation milestone (exit criterion: `jet test --coverage` reports per-line /
per-function coverage) can proceed independently of D-TEST1 and D-TEST4.

---

