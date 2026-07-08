# Prompt frontend archetypes — notes

Three prompts for `jet env` dev shells. Shared TUI palette + copy rules from
`../DESIGN-BRIEF.md`. All ≤2 lines. Diagnostic verbatim from
`tests/ui/arg_type_mismatch.stderr` (E0112). Env name comes from `env.jet`.

Core-loop test (one sentence each; none match):
- **minimal** — prompt stays out of the way until you ask for status.
- **segments** — a glance answers every status question, always on.
- **adaptive** — the prompt speaks only when something changed.

The three are a pull / push-always / push-on-event axis, not three paint jobs.

---

## 1. minimal.html — single line, near-silent

**Core loop:** the prompt stays out of the way until you ask it for status.

**Rationale.** One line: env marker + chevron. The only persistent state is the
chevron's exit-code color (green ok / red + code). One keystroke expands a
single dim line with last build/test, then collapses. Pull, never push.

**Transplants.** starship transient prompt (last-exit glyph carried forward).
fish minimalism. atuin-style on-demand recall (status is fetched, not narrated).

**Risks.** Exit-code-as-color is the whole persistent signal → NO_COLOR must
swap to an `ok`/`x N` tag (done). Glance keystroke (`^G`) must not clash with
readline. Users who want always-on status will prefer segments — that's the
point of the axis.

```
web-api ❯ jet test
12 passed
web-api ❯ jet run           (chevron green: last exit 0)
…server exited 1
web-api ❯ 1                 (chevron red + code: last exit 1)
web-api ❯                   (press ^G to glance ↓)
        build ok · 0.4s     test 12/12 · 8ms ago
NO_COLOR:  web-api ok ❯     web-api x 1 ❯
```

---

## 2. segments.html — rich two-line

**Core loop:** a glance answers every status question before you type.

**Rationale.** Info line of always-on segments (env · version · build · test ·
git) over the input line. Each segment is one truth, color-coded. Finished
commands collapse to a one-line receipt so history stays skimmable. Maximal
awareness for the cost of one extra line.

**Transplants.** starship/powerline segment prompt (words, not powerline
glyphs — ANSI-only) + transient collapse of finished prompts.

**Risks.** Two lines every prompt is a lot for a narrow terminal — segments
must drop right-to-left on width. Segment/diagnostic codes must agree (both
E0112). NO_COLOR relies on segments already being words (`build ✗ E0112`,
`test stale`, `main •1`) — holds.

```
web-api ❯ jet test                          ✓ 12/12 · 0.3s   (collapsed)
web-api · jet 0.1.0 · build ok · test 12/12 · main ✓
❯ _

after a failed build:
web-api ❯ jet build                         ✗ E0112 · 0.2s
web-api · jet 0.1.0 · build ✗ E0112 · test stale · main •1
❯ _
```

---

## 3. adaptive.html — event-driven

**Core loop:** the prompt speaks only when something changed.

**Rationale.** Clean shell = bare chevron. A running job shows one live Braille
spinner line, then it collapses. On failure it inserts exactly one line —
phrased as the next action with a runnable command — that vanishes on your next
input. Success adds nothing; silence is the signal.

**Transplants.** fish/starship transient (collapse to nothing). tealdeer/navi
"here's the command to run" phrasing. Braille progress spinner.

**Risks.** The action line is prompt copy, not a diagnostic — must never restate
the verbatim E-code text (I4); it points and hands a command only. Deciding
"something changed" needs a build/test event feed. Reduced motion freezes the
spinner to a static glyph (done). Users who want constant readouts won't like
the silence — that's segments' job.

```
❯                            (clean: bare chevron)
❯ jet test
⠹ running tests · 6/12 · 0.2s          (live, then collapses)
❯ jet test
12 passed                    (success: prompt adds nothing)
❯

on failure — one inserted action line:
❯ jet test
11 passed · 1 failed
→ 1 test failed. Rerun just it: jet test url_parse
❯
NO_COLOR: spinner ⠹ stays; arrow → degrades to ->
```
