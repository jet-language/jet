# Making Nix obsolete — the plan, the gaps, the decisions

> **STATUS (2026-06-12): DRAFT for owner steering. Not ratified. Decides
> no syntax or semantics.** This is the umbrella over three existing
> docs: docs/package-manager-decisions.md (the store + package manager,
> awaiting D-PM1–8), docs/jetpack.md and docs/jetos.md (unratified
> explorations). Where this file and those conflict, this file states
> the *intended direction*; nothing becomes spec until balloted.

**How to read this:** §1 is the one-paragraph claim. §2 is the scorecard
— what Nix is, where it wins, where it loses, and what Jet does about
each. §3 is the genuinely new feature (imperative commands that keep
the system declarative). §4 is the performance case. §5 is the single
dependency path from today to a bootable jetos. §6 is the decisions
only you can make, each with worked examples.

Vocabulary (store, fingerprint, lockfile, generation, sandbox, binary
cache) is defined once in docs/package-manager-decisions.md §1 — read
that first if any term is unfamiliar. New terms used here:

- **Module** — one `.jet` config file that contributes settings.
- **Option** — one declared, typed setting (`sys.desktop.environment`).
- **Switch** — build the complete new system in the store, then
  atomically re-point one link and add a boot entry.
- **Drift** — when what's running no longer matches what's written down.

---

## 1. The claim

Nix is three products wearing one trench coat: a **language** (nixlang),
a **package manager** (the store + derivations + caches), and an **OS**
(NixOS modules). The engine underneath is the best idea in software
deployment — content-addressed, immutable, rollback-able. Everything
*around* the engine is why Nix, after 20 years, is still niche: an
untyped lazy language with the worst error messages in mainstream use,
five overlapping CLIs, a root-owned daemon, and a culture where the
answer to every question is "read the source of nixpkgs."

Jet keeps Nix's engine and replaces everything users actually touch.
The replacement language already exists — it's Jet, evaluated in pure
mode (S60, ratified). The package manager plan already exists —
docs/package-manager-decisions.md ("Nix's engine under Cargo's steering
wheel"). What this file adds: the OS layer direction, the **imperative
front door** that NixOS refuses to build, the ecosystem bootstrap
strategy, and the order of operations.

This is post-v1 flagship work. Nothing here jumps the M9.5→M14 queue;
a half-finished language under a whole OS helps nobody.

---

## 2. The scorecard — three battlegrounds

### 2.1 The language: nixlang vs `jet eval --pure`

What nixlang gets right (we keep): purity — same inputs, same outputs —
is the keystone that makes caching, reproducibility, and rollback sound.

Where it fails (we fix): it is dynamically typed, lazily evaluated, and
its errors are infamous. The same mistake, side by side:

```
# Nix — misspell one attribute, get this:
error: attribute 'enviroment' missing
       at /etc/nixos/configuration.nix:14:3:
        13|   services.xserver.enable = true;
        14|   enviroment.systemPackages = [ pkgs.firefox ];
       (no suggestion, no list of valid names; with deeper mistakes
        you get "infinite recursion encountered" and a 40-line trace
        through nixpkgs internals you've never seen)
```

```
# Jet — the docs/04 voice, because options are declared and typed:
error[J-M010]: unknown option `sys.enviroment.packages`
  did you mean `sys.environment.packages`?
  declared in: std/environment.jet (option list: `jetos options sys.environment`)
```

The structural difference: in nixlang, the config "schema" is whatever
nixpkgs happens to compute at runtime — so nothing can be checked until
evaluation collides with it. In Jet, options are **declared, typed, and
documented before use**, so misspellings, type errors, and conflicts are
ordinary compile-class diagnostics with did-you-mean. Every error gets
a code, what/why/fix, and a snapshot (invariant I4 applies unchanged).

What exists already: `pure fn` is ratified (S60); the M9.5 comptime
interpreter is the evaluation engine `jet eval --pure` extends. Purity
violations are sema errors in Jet's own voice, not runtime surprises.

### 2.2 The package manager: nix vs jet

Covered in full by docs/package-manager-decisions.md; the one-screen
version of what we keep and what we refuse:

| | Nix | Jet |
|---|---|---|
| Store model | content-addressed, immutable — **keep, identical idea** | same, `~/.jet/store` |
| Install | multi-user daemon, root, `/nix` at filesystem root | untar one binary, zero root, zero daemon |
| Daily CLI | `nix-env` vs `nix profile` vs flakes; "experimental" flags for 5+ years | `jet add / build / run / test` — one tool, Cargo-shaped |
| Writing a package | a derivation in nixlang, learned by reading nixpkgs | a typed recipe in Jet with checked fields and real errors |
| Errors | hash mismatch dumps, eval traces | E-coded, what/why/fix, snapshot-pinned |

The strategic point repeated from that doc because it is load-bearing:
**the store ships in M12 phase 1**, under the ordinary package manager.
The Nix-class machinery (sandboxed builds, signed caches, generations)
is layer 3 on the *same* store — an upgrade, never a migration.

### 2.3 The OS: NixOS vs jetos

What NixOS gets right (we keep): the whole machine as one build —
atomic switch, boot-menu generations, "my computer is a text file."
The module system *idea*: many small files merging into one settings
tree.

Where it fails (we fix):

- **Priorities are magic numbers.** `mkForce` is priority 50,
  `mkDefault` 1000, `mkOverride 900` when those fight. Jet: exactly
  three words — `default x = v`, `x = v`, `force x = v` — and a
  same-priority conflict is a loud error naming both files and lines.
- **Options are undiscoverable.** Finding a NixOS option means
  search.nixos.org or reading nixpkgs source. Jet: options are typed
  declarations in std modules; the LSP autocompletes them, `jetos
  options <prefix>` lists them, unknown ones get did-you-mean.
- **Sharing config is folklore.** Copying someone's flake module drags
  hidden dependencies. Jet: modules communicate *only* through declared
  options, which makes `jetos lift <repo>#<module>` statically
  checkable — it works, or it tells you exactly which option
  declaration you're missing.
- **Two config ecosystems.** NixOS + home-manager are separate worlds
  with separate docs and update cycles. Jet: one tree — `sys.*`,
  `user.<name>.*`, `apps.*` — one feature file may touch all three.
- **No imperative path at all.** This is §3; it's the headline.

---

## 3. The evolution: an imperative front door on a declarative house

This is the feature NixOS philosophically refuses, and it is the #1
reason normal users bounce. On NixOS, "install Firefox" means: open an
editor, find the right file, know the option name, edit, rebuild. The
imperative command that does exist (`nix-env -i` / `nix profile
install`) **bypasses the config entirely** — now your system and your
config disagree forever. NixOS makes you choose: friction or drift.

Jet refuses the choice. The rule:

> **Imperative commands never act on the system. They edit the
> declarative config — the same files you could edit by hand — then
> apply it.** The config repo remains the single source of truth at
> every instant; the CLI is just a very good editor for it.

What that looks like:

```
$ jetos add firefox
  modules/apps/firefox.jet not present — enabling via host file:
    hosts/laptop.jet  + apps.firefox.enable = true
  building… 1 substituted ↓ · generation 24 → 25
  recorded: git commit a3f91c "jetos: add firefox"

$ jetos set sys.desktop.environment plasma
  hosts/laptop.jet  ~ sys.desktop.environment = plasma   (was cinnamon)
  building… generation 25 → 26

$ jetos remove firefox
  hosts/laptop.jet  - apps.firefox.enable = true
  generation 26 → 27
```

Three properties make this sound where `nix-env` is not:

1. **Drift is structurally impossible.** There is no second database of
   "imperatively installed things" — the command's only write target is
   the config. Hand-edits and CLI edits are the same kind of change;
   `git log` is the complete, honest audit history of the machine.
2. **It's bidirectional.** Power users edit files and run
   `jetos switch`; beginners type commands and get clean file edits
   they can read later. Both produce identical repos. A beginner
   *becomes* a power user by reading their own git history.
3. **Ephemeral wants are served separately, and honestly.** "I need
   ripgrep for ten minutes" shouldn't edit your config — but it also
   must not silently mutate the system:

```
$ jetos try ripgrep imagemagick
  available in this shell only — nothing recorded, gone on exit
  (to keep: jetos add ripgrep)
$ rg --version
ripgrep 14.1.1
```

This is also the enterprise story, not just the beginner story: a
helpdesk runs `jetos add vpn-client --host sales-laptop-14`, the change
lands as a reviewable commit in the fleet repo, CI runs `jetos check`,
and the machine applies it. Every machine change is a diff with an
author. Compliance teams currently pay vendors a lot of money for a
worse version of "the machine is a git repo."

---

## 4. The performance case

Honest framing: Nix's *builds* are as fast as builds are; the engine
isn't the slow part. Nix is slow in two places users feel every day:

1. **Evaluation.** nixlang is untyped, lazy, and single-threaded.
   Evaluating a NixOS system config takes tens of seconds and gigabytes
   of RAM, *every rebuild*, before any building starts.
2. **Resolution-by-evaluation.** Nix has no resolver — "what versions
   exist" is answered by evaluating all of nixpkgs.

Jet's structural advantages (claims to be proven by benchmark, not
asserted):

- **Typed + strict beats untyped + lazy.** Option types are known
  statically; merge is mechanical tree-combination, not thunk forcing
  through 100k attribute sets.
- **Parallel by construction.** Modules are independent until merge
  (they only communicate through options), so per-file evaluation
  fans out across cores. Nix's evaluator is single-threaded by design.
- **Incremental by construction.** Plans are canonical and hashed;
  unchanged module files mean unchanged partial plans — re-switch after
  a one-line edit re-evaluates one file, not the world.
- **Resolution from metadata** (PM-I4): version picking reads a tiny
  git index, never evaluates packages.

Exit criteria worth pinning when this work starts (measured, in CI):
evaluate + merge a realistic desktop host config in **under 1 second**;
no-op `jetos switch` in **under 3 seconds** end-to-end. Baseline: the
same config expressed in NixOS on the same machine, numbers recorded.

---

## 5. The path — one dependency chain

Everything below the line "v1 ships first" is sequenced after M14.
The chain, with what each step unblocks:

```
TODAY ── M9.5 comptime interpreter ──────────── the evaluator engine
      ── M10 stdlib · M13 LSP · M14 v1.0 ────── the language is real
─────────────────────────────────────────────────────────────────────
PM-1  ── M12.1: jet.toml + THE STORE ────────── fingerprint store exists
PM-2  ── M12.2: registry + resolver + audit ─── ecosystem mechanics
NX-0  ── `jet eval --pure --json` (S60) ─────── recipes/configs evaluable
NX-1  ── layer 3a: sandboxed builds + recipes ─ build non-Jet software
NX-2  ── layer 3b: signed caches + generations ─ substitution + rollback
NX-3  ── module system: options + merge engine ─ (pure library on NX-0;
                                                  can prototype early)
NX-4  ── recipe generator + activation ───────── `jetos switch` on a VM
NX-5  ── std option tree v0 + imperative layer ─ boot a real machine;
                                                  jetos add/set/try
NX-6  ── bootstrap the package set (D-NX1) ───── enough software to live on
```

Notes on the chain:

- NX-3 (the merge engine) has **no build-system dependency** — it's
  pure evaluation. It can be prototyped the day NX-0 works, in parallel
  with NX-1/2. It is also the highest-risk design (merge semantics),
  so prototyping it early is cheap insurance.
- NX-6 is not a milestone, it's a strategy (D-NX1 below) that starts
  feeding in from NX-1 onward.
- jetos.md's OS0–OS4 milestones and exit criteria (shuffle-order
  determinism, power-cut VM test, conflict snapshots) remain the right
  tests; they re-home onto this chain as NX-3…NX-5.

---

## 6. The decisions (D-NX1…D-NX5)

Prerequisite zero: **ratify D-PM1…D-PM8** in
docs/package-manager-decisions.md. Every row below assumes that file's
recommendations (one store, one tool, `jet.toml`). Then:

### D-NX1 — How do we get 10,000 packages? (the existential one)

*Plain words: nixpkgs is ~100,000 packages and 20 years of contributor
work. A Nix replacement with 50 packages replaces nothing. Where does
jetos's software come from in years one and two?*

**Option A — Tap Nix's caches during bootstrap (recommended), write
Jet recipes only for what we must.** A read-only substituter understands
cache.nixos.org's format; a generated shim recipe wraps each needed Nix
package by its store hash. Native Jet recipes are written (or
auto-translated, then human-cleaned) starting from the bootstrap chain
(kernel, libc, coreutils, desktop) outward, replacing shims over time.

```
$ jetos switch
  37 native recipes built/substituted (jet cache)
  204 bootstrap packages substituted (nixpkgs cache, read-only) ▒▒▒ 78%
  note: `jetos bootstrap-status` tracks the native-recipe migration
```

- Strengths: usable system in months, not years; migration pressure is
  gradual and measurable; NixOS users can switch without losing
  software.
- Weaknesses: a compatibility layer to maintain; jetos's quality story
  partially depends on nixpkgs's until migration completes; risk of
  the shim layer becoming permanent (needs a stated sunset metric).

**Option B — Greenfield only.** Every package gets a native Jet recipe
from day one.
- Strengths: total quality control; no foreign formats anywhere.
- Weaknesses: years of packaging labor before a usable desktop exists;
  this is the strategy graveyard of every "Nix but better" predecessor.

**Option C — Automated nixpkgs translation as the primary source.**
Mechanically convert derivations to Jet recipes wholesale.
- Strengths: big numbers fast.
- Weaknesses: generated recipes inherit nixpkgs's complexity *and* its
  idioms in worse form — the error-message quality that justifies Jet's
  existence can't survive 100k machine-translated files.

Recommendation: **A.** Greenfield the spine, tap the caches for the
long tail, measure the migration.

### D-NX2 — What exactly does `jetos add` edit?

*Plain words: the imperative command must write somewhere a human would
have written. Where?*

**Option A — the host file, smallest possible edit (recommended).**
`jetos add firefox` appends `apps.firefox.enable = true` to
`hosts/<current>.jet`. Settings via `jetos set` edit the same file.
- Strengths: one predictable location; the diff is one line; matches
  what a human teaching themselves would write.
- Weaknesses: long-lived machines accumulate a long host file (mitigate
  later with `jetos tidy` suggesting moves into modules — a tool, not a
  second mechanism).

**Option B — generate a per-feature module file** (`modules/apps/
firefox.jet`) on every add.
- Strengths: dendritic layout from day one.
- Weaknesses: machine-generated module files mix with hand-written
  ones; removal means deleting files the user may have edited; more
  magic, less readable diff.

Sub-decision, same ballot: does the command **git-commit automatically**
(recommended: yes, with `--no-commit` absent and a dirty-tree refusal:
"your config has uncommitted edits — commit or stash, then re-run"),
and is `--host <other>` allowed for fleet use (recommended: yes)?

### D-NX3 — Is `jetos try` (ephemeral, unrecorded) in or out?

*Plain words: a sanctioned temporary tier, clearly fenced off from the
real system — or nothing but the config?*

**Option A — yes, shell-scoped only (recommended).** Packages exist on
PATH for one shell session via store links; nothing written anywhere;
the exit message tells you how to keep it.
- Strengths: kills the #1 daily-driver complaint about NixOS
  (`nix-shell -p` is this feature, but Nix never integrated it
  honestly); zero drift by construction.
- Weaknesses: one more verb to teach; must visibly never survive a
  reboot or appear in `jetos diff`.

**Option B — no ephemeral tier.** Purity of story.
- Strengths: one fewer concept.
- Weaknesses: users will fake it with containers or — worse — stop
  using jetos for daily driving. The absence *creates* drift pressure.

### D-NX4 — Migration on-ramp for NixOS users: how much do we build?

*Plain words: the people most likely to adopt jetos year one already
run NixOS. Do we meet them halfway?*

**Option A — a one-shot reporter, not a converter (recommended).**
`jetos migrate --from /etc/nixos` reads the evaluated NixOS config
(not the nixlang source) and emits a *report*: which settings map to
jetos options (with the lines to paste), which don't yet, which
packages resolve via D-NX1's bootstrap tap.
- Strengths: honest; no promise of translating arbitrary nixlang;
  output is a checklist a human finishes in an afternoon.
- Weaknesses: not magic; marketing can't say "automatic migration."

**Option B — full automatic conversion of NixOS configs.**
- Strengths: the dream demo.
- Weaknesses: nixlang configs are Turing-complete programs; the long
  tail is unconvertible, and a 90% converter that silently drops the
  other 10% of someone's system is worse than a report.

### D-NX5 — What is the v0 product, concretely?

*Plain words: "replace NixOS" is not shippable. What single artifact is?*

**Option A — one reference desktop image (recommended).** x86_64,
Cinnamon (the Mint-class target already in jetos.md OS4), installer
that generates the §4-layout repo, D-NX1 bootstrap tap on. Server/
headless profile follows; enterprise fleet features (central repo,
many hosts, signed internal cache — all already designed in by
`hosts/*` + D-PM caches) get a guide, not new machinery.
- Strengths: one thing to polish, demo, and benchmark against NixOS;
  beginner and enterprise stories share 100% of the machinery.
- Weaknesses: ARM, alternative desktops, and cloud images wait.

**Option B — a "toolkit" release: jetos as a framework, no image.**
- Strengths: cheaper.
- Weaknesses: replays Nix's adoption failure — infinite power, no
  on-ramp. Rejected by the philosophy this whole project runs on.

---

## 7. Scope guards (so this stays an evolution, not a second Nix)

- **No escape hatches.** No raw-nixlang passthrough, no "execute this
  shell script at activation" option, no per-repo helper libraries.
  The absence of escape hatches is the product (jetos.md guardrail,
  reaffirmed). The D-NX1 bootstrap tap is a *substituter*, not a
  language bridge — Nix code never appears in a jetos config.
- **One way to do things, audited per decision.** Every D-NX option
  above was checked against this; anything that creates a second
  mechanism for an existing job is out by default.
- **Single-file `jet run` is forever untouched** (R9 / PM-I8). The
  language never grows OS-flavored ceremony.
- **All of it is diagnosable.** Every new error (J-M merge, J-P purity,
  activation failures) carries a code, what/why/fix, and a snapshot —
  I4 does not bend for the OS layer. If jetos shipped with Nix-quality
  errors, there would be no reason for it to exist.
