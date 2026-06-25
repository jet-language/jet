# c159 — Build profiles: named + flag-selected (D-BUILDPROFILE1=A)

**Status:** Ratified, not started. Gated on c157 (D-CTEFFECT1) and c158 (D-DOTCTOR1). Build only after both land.

## Goal

Let a package's `build { }` surface define named build profiles — `release`/`debug`/`ci` as `Build.{ optimize: …, targets: […] }` — selected by an **explicit flag** (`--release` = sugar for `--profile=release`; general `--profile=<name>`). Never by ambient environment. Same commit + same flag ⇒ byte-identical binary on every machine.

## Current state (verified, file:line)

- **No named-profile concept exists.** The only build "profile" today is the rustc optimization preset enum `BuildProfile { Default, Small, Freestanding }` — `Source/main.rs:80-87`. It is resolved purely from flags `--small` / `--freestanding` in `Source/CmdCompile.rs:25-31`; the default leans speed. (This enum is an *opt preset*, a different axis from D-BUILDPROFILE1's named profiles — naming collision to resolve below.)
- **rustc flags per preset** are emitted in `Source/CmdCompile.rs:638` (`small` bool) and `:683-707`: `opt-level`, `lto=thin|fat`, `panic=abort`, strip. This is the single place a profile's `optimize:` setting would flow into.
- **CLI flag parsing is flat `iter().any()`** over `jet_argv` — `Source/main.rs:301-321`. Value flags use `strip_prefix`, e.g. `--target=` at `:316`. Unknown flags are caught by `check_flags` against `is_known_flag` / `closest_flag` (`Source/main.rs:209-237`, registry in `Source/CLISpec.rs`). A new `--profile=<name>` and `--release` must be registered there.
- **Build targets already parse** in the package manifest: `targets: [ … ]` (D-TGT1/D-TGT2) in `Source/Jetpack/PackageManifest/mod.rs:72-113,224`, with diagnostics `E1210`–`E1216` (`docs/spec/diagnostics.md:357-363`). Profiles' `targets:` reuses this `Target` vocabulary.
- **No `build { }` block surface exists.** `env.jet` is the dev-shell only (`examples/jetpack/env.jet` — `fn shell()`), and `pack.jet`/manifest carries `packages:`/`targets:`. There is nowhere a profile block lives today; it must be added.
- **`Build.{ }` dot-construct does not exist yet** — it is c158 (D-DOTCTOR1). Until c158 lands, the `Build.{ optimize:…, targets:[…] }` literal cannot be parsed.
- Diagnostics namespace: manifest/jet-driver errors live in `E12xx`; last used is `E1216`. A new "unknown profile name" code slots at `E1217`.

## Decision (ratified, verbatim)

A package's `build { }` surface defines named profiles (`release`/`debug`/`ci` as `Build.{ optimize: …, targets: […] }`); the active profile is chosen by an EXPLICIT FLAG (`--release` sugar for `--profile=release`, general `--profile=<name>`), never by ambient environment — same commit + same flag ⇒ byte-identical binary on every machine. Blessed names `release`/`debug` carry built-in defaults; others user-defined. Ambient-env selection (the `CMAKE_BUILD_TYPE` footgun) is rejected as it contradicts D-CTEFFECT1.

## Implementation (staged)

1. **`build { }` surface — parse + named profiles.**
   - Decide where the block lives: it is a *package* surface, so it belongs in the package manifest (`pack.jet`), not `env.jet` (dev-shell only — see payload/manifest split). Add a `build { … }` block whose entries are `name: Build.{ optimize: <preset>, targets: [<Target>…] }`.
   - Reuse the existing `Target` vocabulary (`PackageManifest/mod.rs:72`). `optimize:` is a closed menu mapping to the rustc preset axis (see step 3): at minimum `speed` (default), `size` (today's `Small`). Keep the menu small (I8) — extend only with owner sign-off.
   - Blessed names `release` and `debug` carry built-in defaults even when the block omits them; user-defined names (`ci`, …) must be fully specified. An empty/absent `build { }` ⇒ `debug` default for bare `jet build`, `release` for `--release`.
   - Parsing reuses the `Build.{ }` dot-construct from c158; do **not** invent a second literal form (I8).

2. **`--profile=<name>` / `--release` flag wiring.**
   - Register both in `Source/CLISpec.rs` (`is_known_flag`) so `check_flags` accepts them and `closest_flag` suggests them. Document under `flags:` in `Source/main.rs` usage.
   - Parse `--profile=` via the `strip_prefix` pattern (`Source/main.rs:316` is the template). `--release` desugars to `--profile=release` at parse time (single canonical internal path — I8). Reject passing both with a clear usage error (exit 2).
   - Selection is flag-only. **No env var is read** to pick a profile (no `JET_PROFILE`, no `CMAKE_BUILD_TYPE` analog). This is the c157 hermeticity tie-in.

3. **Profile application to codegen/optimize/targets.**
   - Resolve the selected profile name → its `Build.{ optimize, targets }` record → feed `optimize:` into the existing rustc-flag site (`Source/CmdCompile.rs:638,683-707`). Map the `optimize:` menu onto/through the existing `BuildProfile` opt-preset enum. **Resolve the naming collision:** rename the existing `BuildProfile` enum (e.g. `OptPreset`) so "profile" means the user-named D-BUILDPROFILE1 profile and "opt preset" means the rustc knob; `--small`/`--freestanding` keep selecting opt presets, orthogonal to `--profile`.
   - `targets:` from the active profile filters which targets are built.
   - Codegen stays dumb (I3): the profile only parameterizes rustc invocation flags + target selection; it makes no semantic decision.

4. **Byte-identical guarantee (no ambient reads).**
   - Audit the build path for ambient inputs that leak into output: env vars, wall-clock, cwd-dependent absolute paths in the generated `.rs` or rustc args, `$HOME`/store-path embedding. The profile + commit must fully determine rustc flags.
   - This is where the c157 effect model applies: any ambient read on the default build path is an ungated effect and must be removed or gated. Add a test that runs the same source under the same `--profile` twice (and, if feasible in CI, in two cwds) and asserts byte-identical output binaries (or identical generated `.rs` + identical rustc arg vector as a cheaper proxy).

5. **Diagnostics + snapshots.**
   - `E1217` (new, `E12xx` driver/manifest range): unknown profile name. what/why/fix with a did-you-mean over the defined + blessed names (mirror `closest_flag`). Add the `tests/ui` snapshot (I4 — no snapshot, no diagnostic).
   - Reuse `E1216`-style shape for an unknown field inside `Build.{ }` / bad `optimize:` value.

6. **Example + golden (I5).**
   - Add an `examples/` package (or jetpack example) with a `build { }` defining `release`/`debug`/`ci`, plus expected output showing `--profile=ci` selecting it. Golden-enforced.

7. **Tests.**
   - Manifest parse of `build { }`; flag parse of `--profile=`/`--release` incl. desugaring and the both-given error; profile→rustc-flag mapping; the byte-identical/determinism test; `E1217` ui snapshot; formatter round-trip for the new `build { }` block (new syntax requires formatter emission + a fmt stability test — see formatter-roundtrip-required-for-new-syntax).

8. **Docs.**
   - `docs/spec/spec.md`: document `build { }`, the profile menu, blessed defaults, and flag selection. `docs/spec/diagnostics.md`: register `E1217`. Cross-ref D-CTEFFECT1 for the no-ambient-read guarantee. Record `--release`/`--profile` keyword/flag with decision id in `Source/Syntax.rs` (I7).

## Sequencing/gates

- **c157 (D-CTEFFECT1) first** — the "never by ambient environment / byte-identical" guarantee is exactly the effect model's hermeticity property. Profile selection must be provably free of ungated ambient reads (stage 4); build on c157's tier-0/gated machinery rather than re-deriving it.
- **c158 (D-DOTCTOR1) first** — `Build.{ … }` is the dot-construct literal. Cannot parse the profile records until `.{ }` / `T.{ }` exists.
- Within this card: stage 1 (surface) → 2 (flag) → 3 (apply) → 4 (hermeticity) → 5–8.

## Open Owner-Q

1. **Where does `build { }` live** — in `pack.jet` (package manifest, recommended: profiles are a package property) or a top-level block in another file? Confirm placement before parser work.
2. **`optimize:` menu.** Is the value menu exactly the existing opt-preset axis (`speed`/`size`, plus `freestanding`?), or a profile-specific richer set (debug-assertions on/off, `panic=abort|unwind`)? The ratified card lists `optimize:` abstractly. A small closed menu is the I8-safe default; confirm its members.
3. **`ci` as a third blessed name?** The card names `release`/`debug`/`ci` in the example but blesses only `release`/`debug` with built-in defaults. Confirm `ci` is user-defined (no built-in default), or bless it too.
