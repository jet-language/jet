# Mine-video report: "Verse: A New Scripting Language? In THIS Economy?"

*Source: Logan Smith, https://www.youtube.com/watch?v=ebqKYLKjL6U — mined 2026-07-24. Report-only; no Tower/repo changes.*

**Verdict:** No code gaps. This video is a pedagogical crash course on Epic's Verse (the language deprecating Unreal Blueprints in UE6). Jet has independently arrived at the *better-regarded* answer to nearly every design question it raises, and already tracks the one open idea (transactional rollback) as a "watch" lesson. The real value is **strategic validation**: the video documents, in real time, Epic abandoning the non-programmer visual audience that Jet's Epoch 6 Canvas is built to keep.

## Source coverage & limits
- **Video:** Logan Smith, 27m, pub 2026-07-23, 27.3k views, 2.7k likes, 468 comments. Full transcript read (109 chunks).
- **Captions: auto-generated only** — no creator subtitles. Terminology is consistent and the speaker is clear, so **medium-high** confidence, but no claim rests on caption text alone.
- **Comments:** all 468 embedded in capture (409 roots / 59 replies). Stratified sample read: top-liked + 118 keyword-technical + targeted full-text pulls. No retrieval warnings.
- **Note:** the skill's `inspect_capture.py` helper does not exist in this repo; captures parsed with inline Python (`nix shell nixpkgs#python3`). Ledger built by hand at `/tmp/verse.claims.json`.

## What the video actually argues
Verse is a functional-logic language with a rich static type system. Core mechanics:
- **Effect system.** Functions carry effect specifiers (`computes` = pure; default = `reads`/`writes`/`allocates` heap effects). Compiler enforces them and exploits purity (CSE, memoization). Full function-type variance over effects.
- **`decides` effect = fallibility.** Failure lives in the *effect system*, not the return type — a fallible fn still returns plain `int`, just "sometimes fails." No throw/return; failure short-circuits. Transcript (03:31) + comments confirm a failing path **rolls back all its state mutations** (transactional memory).
- **Dual call syntax.** Failure-returning fns called with `[]`, infallible with `()`. Array/map indexing reuses `[]` because they're *partial functions* key→value; compiler enforces bounds/absence safety. This "derived square brackets from first principles" is the video's payoff moment.
- **`if` is effect control flow** (runs a fallible expr, branches on success/fail — like try/catch); `logic` is the boolean type; `?` converts a logic value to success/fail; failure↔Optional is isomorphic via an `option` constructor.
- **Framing thesis:** LLMs are obsoleting hand-written code; a text language like Verse may end up mostly AI-authored.

## Strongest comment signals (anonymous)
- **Top technical critique — `decides` carries no error payload.** Repeated independently: *"I can't tell the caller what made it fail"*; a backend dev: *"we need to propagate information about the problem… Rust's Result are my bread and butter."* Verse's guidance (per a commenter checking the docs) is to express failure modes as **preconditions checked before the call**, plus rollback+retry for alternatives.
- **Idiosyncrasy fatigue.** *"why 'logic' instead of 'boolean'!? Why sometimes square brackets and sometimes round ones… when it's already checked by the compiler?"*; complaints about 8-char keywords.
- **Effects should be inferred, not hand-typed** (multiple).
- **Audience mismatch, sharpest form:** *"you invent visual programming for artists and level designers… then deprecate it in favor of yet another scripting language… The new Verse is not for artists and designers, it's for AI."*
- **Parse-don't-validate alternative:** make illegal inputs unrepresentable (a `NonZeroInt`), so the fn is total and never fails — push failure to one parse boundary.
- Context: Verse is led by **Simon Peyton Jones** (Haskell); one commenter who met him relays team skepticism that its denotational semantics are "realistically achievable."

## Jet cross-check (per topic)

| Video topic | Jet status | Evidence |
|---|---|---|
| Effect system + purity | **Already implemented** | D-EFF1 Koka-style **inferred** effect rows; `--[E1,E2]->` arrow (D-SHAPE8=A, card #543); purity = empty row `--[]->`; closed 10-root vocab (Net/FS/IO/DB/Time/Rand/Env/Exec/Log/GPU); `#Caps(…)` block restriction. `docs/spec/spec.md:2534-2643` |
| `decides` (failure-as-effect, no payload) | **Deliberately rejected** | Jet uses errors-as-values `T ? E` with **typed** `E`, postfix `?` propagation, `??` fallback. `docs/spec/spec.md:784-806`. This *is* the Rust-Result path commenters preferred over `decides`. |
| Transactional rollback on failure | **Tracked as "watch" — the one open idea** | `docs/proposals/language-shape-research.md:354`, `docs/archive/language-lessons-and-regrets.md:399-407`: rollback only via *explicit* checked transaction regions, sema-proven rollback-safe; "ordinary `?` propagation never implies rollback." Not built; correctly deferred. |
| Dual `[]`/`()` call syntax by fallibility | **Rejected by design** | Jet keeps one call syntax; fallibility is in return type + `?`. I7 (ratified syntax) + owner anti-idiosyncrasy. Commenter critique validates the choice. |
| `if` reads effect not bool; `logic` naming | **Rejected by design** | Jet `if` reads real bools; no `logic` rename. Naming discipline = I7. |
| Optional↔failure | **Already implemented, kept distinct** | `T?` = `Val`/`None` (no null); `T ? E` = fallible; D-RESULT-OPTION-CANON1 disambiguates the sigil; `?` propagates both. `docs/spec/spec.md:555-557,798-800` |
| Effects should be inferred | **Already the Jet choice** | Rows inferred per-function (D-EFF1), not mandatory hand-annotation. |
| Blueprint → dense-language audience mismatch | **Strategic validation — see below** | Epoch 6 "Canvas" + D-CANVAS-RAD1=A. |
| Parse-don't-validate / refinement types | Minor, not a gap | Jet has newtypes; no `NonZero`-style refinement type surfaced. Not raised as a need. |

## The one thing worth the owner's attention (strategic, not a code gap)

Jet's **Blueprint north-star is exactly the counter-position to what this video documents.** Epic is replacing a friendly *visual* tool for non-programmers with a dense text language — commenters read the real target as "AI, not artists." Jet's **Epoch 6 Canvas** is a source-backed, Blueprint-class **visual editor** (1:1 UE Blueprint graph UX over ordinary Jet source), and **D-CANVAS-RAD1=A** ("one editor, two surfaces," Lazarus-class RAD) explicitly notes Blueprint's "vendor is stepping away… which leaves both halves contested."

Two honest tensions for the owner, not action items:
1. **Timing.** The Blueprint-abandonment moment is a market opening *now*; Jet's answer (Canvas M1–M6, then RAD) is `planned`/`open`, and RAD is `blockedBy` #384 (post-M-arc). The video is evidence the window is opening on schedule — nothing to change, but useful signal for sequencing.
2. **Audience.** Jet's declared v1 audience is programmers (Go/Zig/C/Rust switchers, `philosophy.md:130-139`); the non-programmer/hobbyist base Blueprints served is only addressed post-M-arc via RAD. The video's core complaint ("who is this actually *for*?") is the exact question the RAD ballot answers — worth keeping RAD's "Sam the hobbyist" persona sharp when the M-arc lands.

## Corrections / disputes
- None material against Jet. The video's own caveat (description + 19:02) admits its `safe_divide` example is imperfect — integer division in Verse already has `decides` built in and returns a rational, not an int. Doesn't affect any Jet finding.

## Recommendations
- **Implement:** nothing. Jet's error and effect models already answer this video.
- **Optional doc touch-up:** the video + comment ledger are fresh field evidence corroborating two existing Jet dispositions — the errors-as-values-over-`decides` choice and the Verse rollback "watch." The lessons archive (`language-lessons-and-regrets.md:399-407`) could cite the payload-less-failure critique as ecosystem evidence, but it's already substantively covered.
- **Avoid:** the two things the video's own comments flag as mistakes — payload-free failure and call-syntax overloaded by fallibility. Jet already avoids both; keep it that way.

**Owner gates:** none newly raised. Existing gate this video pressures on timing only: D-CANVAS-RAD1 sequencing (already ratified =A, blocked by #384).
