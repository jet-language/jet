# Frontend v2 — design-director challenge pass

Review of the seven surfaces against the v2 promise (options are UX archetypes
sharing one fixed theme, not palette-family variants). Every issue found was
fixed in place; residual risks and a director's pick (owner still chooses) close
each section. Files are annotated static mockups for the TUI surfaces (rendered
ANSI-as-HTML) and interactive mockups for the two GUI surfaces (Canvas, Studio).

## Verdict summary

| Surface | Core-loop distinctness | State |
|---------|------------------------|-------|
| Canvas | 4 distinct verbs | fixed, pass |
| Studio | 4 distinct IAs | fixed, pass |
| REPL | 3 distinct | pass |
| Prompt | 3 distinct (pull/push/event) | pass |
| Help | 3 distinct (recall/reference/intent) | pass |
| CLI | 3 distinct | pass |
| Dev server | 3 distinct (richness split) | fixed, pass |

---

## Canvas

Core loops, read from the mockups (not the notes):
- **workbench** — browse the palette catalog, drag a node onto the graph, wire pins, tune in the docked inspector.
- **flow** — type at the cursor to summon a type-compatible node; all chrome floats, nothing docked.
- **duallens** — edit either the code lens or the graph lens; the other follows the cursor live.
- **guided** — pick from only the type-compatible offers; the program grows correct-by-construction, one step at a time.

Distinctness: **pass.** Four different verbs (drag-catalog / summon-at-cursor /
edit-either-synced-lens / pick-typed-offers) over four different IAs (fixed docks
/ infinite floating canvas / split twin panes / vertical spine + offer tray). No
two collapse.

Findings → fixed:
- flow.html diagnostic header paraphrased E0204 ("value changed here, can't be used again") → restored verbatim ("`x` is being changed in this call, so it can't be used again here"). I4.
- flow.html E0204 card was positioned but never made visible (`.on` never added) → now shows under the failing call, as the notes claim.
- guided.html refused-offer Fix line dropped the code backticks → restored `` `&x` ``/`` `copy x` `` to match the snapshot byte-for-byte.
- Renamed the `#hud` CSS id (aviation term, non-visible but grep-flaggable) → `#stageinfo`.

Two key tasks visible: beginner-first-node (guided's every-offer-type-checks is
the strongest; workbench's labelled drag is the most familiar) and expert-speed
(⌘K/right-click in flow, tabs+align in workbench, full text editing in duallens).

Residual risk: guided's linear spine suits growing a function, not reshaping a
large graph — it needs a declared hand-off to workbench/flow (noted in its own
risks).

**Director's pick: duallens** — one artifact, text-speed for experts and a
free live graph for beginners, no new gesture to learn; guided is the beginner
standout to keep as the teaching mode.

## Studio

Core loops:
- **settings** — pick a category, flip one option's control, confirm the single source line it writes, apply.
- **changeset** — stage edits from anywhere into one changeset whose primary screen is the diff + impact, then apply the reviewed set as one build.
- **opsboard** — watch the fleet health board; drill into a red thing, fix or roll back from the drawer, watch it go green.
- **projectional** — read config as the document; edit each value through an inline control, no syntax typed.

Distinctness: **pass.** Four different primary screens (hierarchical navigator /
staged-diff review canvas / health dashboard with drill-in / single live source
document). No shared landing screen.

Findings → fixed (the admitted stubs and other dead controls):
- Secret **Re-key** button was a no-op stub → wired to `jet os secret rekey <name>` (re-encrypts the age file to the host's current recipients, plaintext never read). Same treatment for **Add secret** (`jet os secret add`), **Add package** (`jet os pkg add`), **Remove** package (`jet os pkg remove`, stages a source diff), and the **host-pick** control (notes single-host editing; hosts come from the fleet in workspace.jet).
- opsboard multi-host fleet was invented data → grounded: added provenance ("fleet from workspace.jet"), a code comment stating members come from the `hosts:` list in workspace.jet and per-host state from `jet os status <host>`, and a note that a single-machine user sees one row.
- opsboard **Refresh** button was dead → wired to re-render + toast ("Re-polled jet os status").
- opsboard host ribbon printed a hardcoded "gen 42" label on every host → now per-host `gen ${h.gen}`.

Two key tasks visible in every archetype: change-kernel (settings Boot category /
changeset stage boot.kernel / opsboard drawer / projectional inline dropdown) and
rollback (settings Generations / changeset History / opsboard alert drawer /
projectional generation time-machine).

Residual risk: opsboard's drill-in editor is intentionally shallow versus the
other three; a real build would deep-link to settings/projectional for deep edits.
projectional widgetizes option values only (package/service sub-records stay
rendered-static) — a full impl must widgetize all values and reuse the formatter
for the Text round-trip.

**Director's pick: projectional** — single source-of-truth document, I8-clean
(the same set-option mechanism, an expert entrypoint), scales from beginner to
expert; changeset is the safety-tier runner-up for teams that want a review gate.

## REPL

Core loops:
- **line** — type an expression, read its value, keep typing (scrollback is the record).
- **notebook** — build a session of blocks you fold, rerun, edit in place, pin.
- **workspace** — evaluate on the left, watch bindings evolve in a live inspector on the right.

Distinctness: **pass.** Line-at-a-time / addressable blocks / split panes are
three IAs, not one.

E0112 verbatim in all three (color and NO_COLOR variants byte-checked; the
workspace pane soft-wraps to width with every word intact). NO_COLOR honesty:
selection becomes `>`, status words carry state, the box survives — checked.

Ballot flags carried (do not resolve here):
- `?name` symbol-docs is new syntax not in REPL.rs → needs an owner ballot row.
- notebook edit-in-place rerun must recompute or stale downstream bindings → the semantics need a ballot.

**Director's pick: line** for the shipped default (lowest ceremony, clean pipe);
notebook is the differentiated power surface once its rerun ballot lands.

## Prompt

Core loops (a genuine pull / push-always / push-on-event axis):
- **minimal** — the prompt stays silent until you press `^G` for status.
- **segments** — an always-on two-line info strip answers every status question at a glance.
- **adaptive** — the prompt speaks only on an event (a live spinner, or one inserted action line on failure), else silence.

Distinctness: **pass.** The axis is behavioural, not cosmetic.

E0112 verbatim in all three. adaptive's action line is prompt copy that points at
the diagnostic and hands a command ("→ 1 problem … Fix it, then: jet run") without
restating the E-code words — correct per I4. NO_COLOR: segments are words first
(`build ✗ E0112`, `test stale`, `main •1`); minimal swaps chevron color for an
`ok`/`x N` tag; adaptive keeps the Braille glyph and degrades `→` to `->`.

**Director's pick: adaptive** — silence-as-signal is the freshest and lowest-noise;
segments for users who want a constant readout.

## Help (`jet ?`)

Core loops (speed / depth / intent):
- **palette** — summon, type three chars, Enter, back in the shell with the command prefilled (never run).
- **browser** — explore the whole tool like a reference book, category → command → detail, error codes included.
- **tasks** — say what you want to do, get a numbered recipe of real runnable commands.

Distinctness: **pass.**

Two key tasks: find-fast (palette's fuzzy overlay) and explore (browser's
man-depth pages). E0112 words verbatim where shown. Note: the browser error-code
page renders message + Why + Fix (no instance location — correct for a generic
code reference), and the tasks recipe leads with message + Fix as a recap; both
show verbatim words with no paraphrase, but they are excerpts, not the full block.

**Director's pick: palette** — fastest path back to a prefilled shell, matching
jet's speed ethos; browser is the depth companion.

## CLI output

Core loops:
- **ledger** — read top to bottom once; the active line spins, done lines freeze, scrollback is the permanent record.
- **live** — watch one pinned bottom region (counts + bar + elapsed); finished items promote up into clean scrollback.
- **plan** — every mutation prints a full plan, asks once `[y/N]`, then applies terse, each done line tied to a plan line.

Distinctness: **pass.** Append-record / pinned-region / plan-gate are three
different progress experiences.

E0102 verbatim in all three (build-failure stills byte-checked). NO_COLOR honest
in each (spinner inert, region degrades to appended lines, plan gate auto-declines
without `--yes`). Package data uses nixpkgs, consistent with the interim
native-deps decision.

**Director's pick: plan** — matches the owner's nh/terraform benchmark and is the
safest for destructive ops (`gc`, `jetos switch`); ledger is the zero-ceremony
default for everyday adds.

## Dev server (`jet dev --target=web`)

Core loops (a split-of-richness axis):
- **quiet** — live in the browser; the terminal is a three-line daemon; a full error overlay lives in the page.
- **dashboard** — the terminal leads (pinned status header + request log); the browser shows only a thin error banner.
- **paired** — a one-line terminal status and a browser corner strip mirror each other exactly; on error both render the identical frame.

Distinctness: **pass.** quiet and dashboard are deliberate opposites; paired
refuses the split.

Findings → fixed:
- Visible copy "the terminal is **mission control**" (dashboard `.loop`, plus two NOTES rows) → reworded to "the terminal is the primary view." Metaphor/branding, banned by the brief.

E0102 verbatim in all three (paired soft-wraps the Why/Fix to the narrow two-column
layout; words and code intact). NO_COLOR honest: no sticky header on a pipe (plain
status line), dot degrades to a bracketed word, overlay falls back to the terminal
diagnostic.

**Director's pick: paired** — removes the "which surface is authoritative?"
question by construction (both read the same version poll); dashboard for
terminal-centric multi-client debugging.

---

## One-theme audit

All GUI mockups (Canvas, Studio) share the brief's exact GUI tokens (ground
`#0B1119`, panel `#16202E`, accent `#3FC6FF`, ok/warn/err, rail set). All TUI
mockups share the brief's semantic palette inside the simulated terminal: cyan
`#3FC6FF`, green `#58D68D`, yellow `#FFB454`, red `#FF5C5C`, bright-black
`#7E93A8` — identical across all thirteen. **Within every surface, the options
share one palette** — the v1 sin (archetypes that are secretly theme variants) is
absent.

Residual, cross-surface: two doc-scaffold families exist. repl/prompt/help use a
compact terminal-card layout (page background `#0e141b`, magenta `#c88bf0`);
cli/devserver use a landing-page explainer (`#0B1119`, magenta `#C792EA`). Neither
the page-scaffold background nor magenta is a brief-pinned token, and every
simulated terminal renders on brief-accurate hexes, so this is a cosmetic
presentation inconsistency, not the flagged failure. Recommend a follow-up polish
pass to unify the doc-scaffold background to `#0B1119` and pick one magenta;
non-blocking.

## HTML hygiene

- Added `<!doctype html>`, `<html lang="en">`, `<meta charset="utf-8">`, and a viewport meta to the nine repl/prompt/help files that were missing them (they carry box-drawing and Braille glyphs, so charset matters). All files now declare a doctype and title.
- No external http(s) references anywhere (self-contained; localhost strings are simulated product output). Largest file 32 KB, well under the 300 KB cap. Reduced-motion is respected across all mockups.

## Open ballots to carry into owner review

1. REPL `?name` symbol-docs — new user-facing syntax (not in REPL.rs).
2. REPL notebook edit-in-place rerun semantics — how downstream bindings recompute/stale.
