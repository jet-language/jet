---
name: html
description: >
  Generate self-contained, zero-dependency HTML pages for rich agent output.
  Use when the agent needs to present information that benefits from visual layout,
  interactivity, or structured presentation beyond plain text/markdown.
  Triggers on: code review, PR review, code understanding, design system docs,
  component variants, status report, incident report, slide deck, presentation,
  flowchart, diagram, implementation plan, feature/concept explainer, PR writeup,
  triage board, kanban, feature flags, prompt tuner, interactive editor,
  animation prototype, interaction prototype, visual design exploration,
  code approach comparison, SVG illustrations, dashboard, data visualization,
  interactive table, sortable table, system architecture diagram, service topology,
  gantt chart, timeline, project timeline, milestone tracker. Also triggers on explicit
  "/html" invocation or when user asks to "show as HTML", "generate HTML",
  "visualize as page", "render as a page", or "make a page for".
---

# HTML

Generate self-contained HTML pages for rich agent output. Zero dependencies, fully inline CSS, viewable in any browser. One visual identity: jet black and signal red, monospace placards, an instrument panel that stays quiet until something needs attention.

A page here has one of two jobs. A **report** gets the owner up to speed on the project and, when it proposes, shows a vision of the future; it must follow a report shape below. A **tool** (an editor, a prototype, a gallery) has its own shape and skips that section.

## Report shapes (mandatory for reports)

The owner reads a report to learn where the project was, where it is, and where it is going. A report is a story with that arc, never a rendering of a document. Choose the shape from the content and name it in the design plan.

| Shape | Use when | Acts, in order |
|---|---|---|
| **Status** | reporting findings, audits, progress, incidents | Where we were → Where we are → What comes next |
| **Proposal** | proposing a direction, design, or plan | Where we are → Where we could be → What has to be decided |
| **Hybrid** | the same page reports findings and proposes | Where we were → Where we are (findings) → Where we could be → What has to be decided |

Any other shape is a **fluid shape** and needs the owner's approval before delegation: ask, in one message, which shape you want to use, why it communicates better for this content, and which required acts it keeps. Build only after approval.

### Hero

The first screen is the thesis plus one picture of the arc. The thesis is one plain sentence that says what the reader must take away. The picture is a journey for proposals (a past → present → future timeline with three to six real events) and the findings for status (the two to four facts that changed the picture). No counts as tiles, no legend clutter beyond source and date.

### Length

One scroll, nothing collapsed: three to five screens, linear, every section earning its place. If content does not fit, cut it; do not hide it behind an expander. Precision lives inline as a quiet "More precisely" aside, not in a fold.

### Density and hierarchy (owner ruling, 2026-09-03)

A report is scanned, not read. The first report built under the shapes above was rejected as "extremely verbose, monotonous, very little visual separation": long prose acts with the act name as the heading. The rules that prevent it, enforced by `scripts/check.mjs`:

- **Headline first.** Every `h2` states the takeaway in one sentence ("Eight areas build; none is complete"), never the act name. The act name ("Where we are") is the eyebrow above it. A reader who reads only the eyebrows and headlines gets the whole story.
- **Structure over prose.** Anything with two or more items is a structure: a grid of cards, a two-column list, a Before/After pair, a numbered sequence, a row per area with a status chip. Prose is connective tissue between structures: at most two `<p>` in a row, each under 60 words, and no `<p>` anywhere over 90 words. Every section holds at least one structured element (`.grid`, `.card`, `.pair`, `ol`, `ul`, `table`, `pre`, `figure`).
- **One idea per block.** A card carries one finding, one item, one step: a bold lead of five to ten words, then at most two lines. Lists of names become chips or a compact grid, never a comma run inside a sentence.
- **Numbers as placards.** When the number is the finding (a measured time, a count of failures), set it as a `.value` placard with a label, not inside a sentence.
- **Visual rhythm.** Alternate surfaces down the page: a card grid, then a pair, then a sequence, then a full-width figure. Two consecutive sections must not share the same layout.
- **Asides stay short.** A "More precisely" aside is one or two sentences under 40 words; more than that is a section of its own or gets cut.

Before delegation, write the page as an outline of blocks (eyebrow, headline, block type, items) rather than as paragraphs; the outline is the brief. The checker rejects a `<p>` over 90 words, three prose paragraphs in a row, and a section without a structured element.

### What stays out

- Card numbers, decision ids, ballot lists, and file paths. Tower owns them and already carries each recommendation; the report gives the context needed to decide, in plain words. The final act names each pending decision as a question, the stakes, and what changes with each answer.
- Counts presented as meaning (219 cards, 45 ballots). A number belongs only when the number is the finding.
- Sections that mirror a source document's outline. Merge or drop them until each act is one story beat.
- Metrics readouts (progress bars, tables, meters) in proposals. They belong to status pages, where a measured value is the finding.
- Interactive filters and sorting, unless a status page has a list the owner must search.
- Decorative diagrams. A diagram appears only when structure or sequence is the finding.
- Prose walls: an act set as running paragraphs (see Density and hierarchy).

### Voice

ELI5 by default: lead with the point, one new idea at a time, every term defined on first use, one concrete example per act. Short declarative sentences; cut every clause that does not change what the reader knows. Add a short "More precisely" aside inline only where exact detail changes a decision. Keep identifiers, commands, and diagnostic text verbatim when they appear in an example.

### The "so what" test

Before delegation, read the plan as the owner: after each screen, can you say in one sentence what you now know or must decide? Then read only the eyebrows and headlines: do they tell the story alone? Cut anything that fails either test.

## Execution contract (mandatory)

The calling agent is the orchestrator. The orchestrator must never write the HTML or site implementation itself. It owns all user communication, required product and design decisions, the delegation brief, diff review, and final verification.

Before delegation, the orchestrator must collect:

- The user's goal
- The actual content and data
- For a report: the shape (status, proposal, hybrid, or owner-approved fluid) and the written story, act by act, in ELI5 prose; the orchestrator writes this story itself, because it is the thinking
- The exact target paths and allowed file scope
- Existing project conventions
- The relevant reference asset or assets from this skill
- All constraints
- Acceptance and verification requirements

The orchestrator must resolve each required decision before delegation, including the design plan below. It must not ask Sol to resolve an unclear product or design choice. It must pass one complete, concrete brief and all relevant file paths, file content, and project context to exactly one GPT-5.6 Sol implementation subagent through the Codex CLI. Use this exact command form and send the brief on standard input:

```sh
codex exec -m gpt-5.6-sol -c model_reasoning_effort=high --sandbox workspace-write --skip-git-repo-check -
```

Start the brief with the executor role so Sol implements directly instead of re-orchestrating:

```text
Role: You are the single GPT-5.6 Sol implementation executor for this html handoff. Implement directly. Do not read this skill's SKILL.md, do not run codex, do not delegate, do not ask questions.
Goal: <the complete user outcome>
Files/scope: <exact write targets, allowed reads, and forbidden paths>
Content/data: <all real copy, values, records, and source material; for a report, the act-by-act story, final prose, ready to set>
Reference asset: <selected asset path(s) and the patterns to reuse; always `theme.css`>
Design plan: <subject, audience, the page's one job, the shape and its acts, the hero thesis and its journey or findings, the signature moment, and every resolved layout choice>
Constraints: <project conventions, technical limits, and resolved decisions>
Acceptance: <observable completion criteria; require a complete, self-contained working HTML page, never a partial snippet>
Verify: <`node .agents/skills/html/scripts/check.mjs <file>` plus focused checks Sol may run and evidence it must return>
```

Sol is an executor and implementor only. Sol may inspect allowed files, implement the delegated site or HTML work within the specified scope, and run the requested focused checks. Sol must not invent requirements, make unresolved product or design decisions, ask the user questions, orchestrate other agents, or edit unrelated files.

After Sol returns, the orchestrator must review Sol's diff and verify the completed surface (open it in a browser: screenshot, console, the interactive paths) before it reports to the user. For a report, read the rendered page once as the owner and apply the "so what" test to every screen. The orchestrator, not Sol, owns user communication and final verification.

## When to generate HTML

Prefer HTML over markdown when the output needs:
- Multi-column layouts or side-by-side comparison
- Interactive elements (filters, toggles, drag-drop)
- Color-coded status indicators or severity badges
- Charts, diagrams, or flowcharts
- Slide-style presentations
- Code review with inline annotations
- Any content exceeding ~500 words that benefits from visual hierarchy

## Design direction

The orchestrator writes a short design plan before delegating. It is part of the brief, not left to Sol.

### Ground it in the subject

Name the subject, its audience, and the page's single job in one sentence each. The reader here is a technical owner reading in a dark terminal; the page's job is to make findings legible and decidable fast. Distinctive choices come from the subject's own world: the identifiers, receipts, diagnostics, traces, ledgers, and instruments the content already contains. Build with the real content throughout; never sample data when the real data exists.

### The hero is a thesis

For reports, the hero is fixed by the shape above: thesis plus journey or findings. For tools, open with the most characteristic thing in the subject's world: the one screen, diff, trace, or interactive moment the reader must see first. The header is: a legend line (source and date), the placard title, the ignition line, and the thesis.

### Palette: the Tower board's own

Named in `theme.css`; use the tokens, never raw hex in page rules. The values are shared with `plugins/tower/app/ui/tower.css` so reports and the board read as one product.

| Token | Value | Role |
|---|---|---|
| `--jet` | `#060608` | page background |
| `--soot` / `--ash` | `#0E0E12` / `#16161C` | panels / raised cards, code, inputs |
| `--seam` / `--seam-hi` | `#232330` / `#343444` | borders and rules; hover borders; no drop shadows |
| `--bone` / `--bone-dim` | `#F4F3F5` / `#C9C7D1` | primary / secondary text |
| `--smoke` | `#9D9CA8` | captions, legends, muted values |
| `--ember` | `#FF2E4D` | signal red: emphasis, live, critical, links, the ignition line, keywords |
| `--oxblood` | `#B3122D` | deep red surfaces: selected rows, attention fills |
| `--ok` / `--amber` | `#43C78C` / `#DFA14F` | green for healthy, added, strings; amber for caution, numbers |
| `--cyan` / `--blue` / `--frost` | `#45B8CA` / `#6A8EF2` / `#8D92C9` | types and in-the-wild chips; calls; low-emphasis series |

Accents mean something or they are not used: green is healthy or added, amber is caution, cyan is a type or an outside reference, blue is a call. Status chips: `.status` quiet outline, `attention` (oxblood fill), `critical` (ember fill), `ok` (green outline), `wild` (cyan outline). Diffs: removed is oxblood tint, added is a green tint. Chart series in order: ember, bone, cyan, amber, frost; grid lines are seam.

### Code: full width, colored, never scrolled sideways

Every code block is highlighted with `hl.js` (paste it inside the single `<script>`; it is the same tokenizer Tower uses, classes `hl-k` keyword, `hl-s` string, `hl-n` number, `hl-c` comment, `hl-f` call, `hl-t` type). `pre` takes the full width of its container and wraps (`white-space: pre-wrap`); never `overflow-x: auto`. Two code blocks sit side by side only when neither has a line over 72 characters and the container is at least 900px wide; otherwise stack them. The checker enforces highlighting and wrapping.

### Typography carries the personality

- Display (`--display`): Exo 2, italic, `800`, embedded from `fonts.css`. Titles lean forward like a jet: h1 `clamp(38px, 5.2vw, 58px)`, h2 `26px`. Use it for h1, h2, and nothing else.
- Body (`--body`): Atkinson Hyperlegible (system fallbacks) at `16px/1.6`; h3 is body at `17px/600`; secondary text in `--bone-dim`.
- Labels and code (`--mono`): JetBrains Mono, embedded. Legends, eyebrows, table heads, and status chips at `12px` uppercase with `0.06–0.08em` tracking; code at `13.5px/1.6`. Nothing on the page is smaller than 12px; the reader must not squint.
- Big values use `.value` (mono placard); `.value.attention` turns ember.

Fonts are embedded as data URIs, never linked: paste `fonts.css` after `theme.css` inside the single `<style>`. It adds about 100 KB per page and keeps the page zero-network.

### Structure encodes information

Eyebrows, ticks, and rules must say something true. Number steps only when the content is a real sequence (a process, a timeline, an ordered plan). Group by what the reader decides between, not by how the system is built. Every section opens with `.section-head` (its tick lights when the section arrives). Cards hold one decision, one finding, or one record each.

### Signature and motion

One orchestrated moment: on load the legend, title, and thesis rise in sequence and the ignition line draws left to right under the title (`@keyframes ignite`); sections then reveal on scroll and their ticks light (`IntersectionObserver` adds `is-visible`). Hover is a single micro-interaction: a card's left edge turns ember. Nothing else moves. `prefers-reduced-motion` removes every animation and shows everything at once.

### Copy

Words are design material. Name things by what the reader controls and recognizes, in the interface's voice: "Filter by wave", not "Apply wave predicate". Buttons say what happens and keep the same name through the flow. Errors and empty states say what happened and what to do next; no apologies, no mood. Sentence case, plain verbs, no filler. Keep the source's identifiers, commands, and diagnostic text verbatim.

### Restraint

Spend boldness on the ignition line and the placard title; keep everything else disciplined. Before handing off, review the plan against the generic version of the same page and change whatever reads as a template. After the build, look at a screenshot and remove one accessory.

### Quality floor

Responsive to 360px; `:focus-visible` outline in ember; `prefers-reduced-motion` honored; text contrast at least 4.5:1 (bone, bone-dim, and smoke on jet all pass); keyboard-operable controls (`<button>` with `aria-pressed`, sortable headers with `aria-sort`); tables scroll inside `.table-wrap`; no console errors.

## Design system code

`theme.css` in this directory is the canonical block: paste it inline at the top of the page's single `<style>`, then `fonts.css`, then page rules that use only the tokens. Page structure:

```html
<div class="page">
  <header>
    <p class="legend rise"><span class="live">Live</span><span>Source · path/or/repo</span><span>2026-09-02</span></p>
    <h1 class="rise">Placard title</h1>
    <div class="ignition"></div>
    <p class="thesis rise">The one sentence the reader must take away.</p>
  </header>
  <section class="reveal">
    <div class="section-head"><h2>Section title</h2></div>
    <!-- panels, cards, tables -->
  </section>
  <footer>Canonical source: path/to/source.md; this page is a rendering.</footer>
</div>
<script>
(() => {
  const els = document.querySelectorAll('.reveal, section');
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  if (reduce || !('IntersectionObserver' in window)) { els.forEach((e) => e.classList.add('is-visible')); return; }
  document.documentElement.classList.add('js'); /* reveal is opt-in: without scripting everything stays visible */
  const io = new IntersectionObserver((entries) => {
    for (const en of entries) if (en.isIntersecting) { en.target.classList.add('is-visible'); io.unobserve(en.target); }
  }, { rootMargin: '0px 0px -8% 0px' });
  els.forEach((e) => io.observe(e));
})();
</script>
```

## Use case index

Every asset is a complete page in this identity. Read the closest one for layout mechanics only; a report's structure comes from its shape, never from the asset's outline. Reports: 03, 11, 12, 13–17. Tools: 01, 02, 04–10, 18–24.

### Exploration

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 01 | Code approaches comparison | `assets/01-exploration-code-approaches.html` | Multi-column layout showing 2-3 code solutions side-by-side. Prompt panel at top. Each column: code block + pros/cons + verdict status. CSS grid columns. |
| 02 | Visual design exploration | `assets/02-exploration-visual-designs.html` | Gallery of design alternatives. Thumbnail cards with hover states. Side-by-side comparison with annotations. |

### Code

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 03 | PR code review | `assets/03-code-review-pr.html` | Header panel with PR meta (author, branch, status chips). Diff-style code blocks with line-by-line comments. Severity chips (note/attention/critical). Summary panel with counts. |
| 04 | Code understanding | `assets/04-code-understanding.html` | Annotated source view. Callout panels explaining key sections. Flow arrows connecting code blocks. Collapsible detail sections. |
| 05 | Design system | `assets/05-design-system.html` | Token tables (colors, spacing, typography). Component swatches. Live examples in categorized sections. |
| 06 | Component variants | `assets/06-component-variants.html` | Grid of component states/variants. Each cell: component render + props table. Visual diff between variants. |

### Prototyping

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 07 | Animation prototype | `assets/07-prototype-animation.html` | Interactive animation demo area. Easing curve selector. Play/replay controls. CSS animation with JS toggles. |
| 08 | Interaction prototype | `assets/08-prototype-interaction.html` | Clickable UI prototype. State transitions. Micro-interaction demos. |

### Communication

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 09 | Slide deck | `assets/09-slide-deck.html` | Full-viewport slides with `scroll-snap`. `.slide` per slide. Placard headings. Progress marks. Arrow-key / scroll navigation. |
| 10 | SVG illustrations | `assets/10-svg-illustrations.html` | Inline SVG artwork and annotated diagrams in the palette. |
| 11 | Status report | `assets/11-status-report.html` | Placard values. Progress bars. Quiet/attention/critical status. Section-per-team pattern. |
| 12 | Incident report | `assets/12-incident-report.html` | Timeline layout (a real sequence, so it is numbered). Severity chips. Impact scope. Action items with owners. |
| 17 | PR writeup | `assets/17-pr-writeup.html` | Structured PR description. Before/after comparison. Testing checklist. Reviewer notes. |

### Diagrams and research

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 13 | Flowchart diagram | `assets/13-flowchart-diagram.html` | CSS-drawn flowchart with positioned panels and SVG arrows. Decision diamonds. Parallel paths. No diagram library. |
| 14 | Feature explainer | `assets/14-research-feature-explainer.html` | Concept breakdown with visual aids. Progressive disclosure from simple to complex. |
| 15 | Concept explainer | `assets/15-research-concept-explainer.html` | Educational layout. Diagram + text pairing. Key-concept callouts. |
| 16 | Implementation plan | `assets/16-implementation-plan.html` | Phased plan (a real sequence). Dependency arrows. Effort estimates. Milestone markers. Collapsible detail per phase. |

### Custom editing UIs

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 18 | Triage board | `assets/18-editor-triage-board.html` | Kanban columns (drag-drop). Cards with priority chips. Filter toolbar. Sticky column headers. |
| 19 | Feature flags | `assets/19-editor-feature-flags.html` | Toggle switches. Flag list with metadata (environment, rollout %). Search/filter. Edit-in-place. |
| 20 | Prompt tuner | `assets/20-editor-prompt-tuner.html` | Split panel: editor + preview. Parameter sliders. System/user message sections. Run button. Output area. |

### Data and visualization

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 21 | Dashboard | `assets/21-dashboard.html` | Placard-value cards with change indicators. CSS bar chart + SVG donut. SVG sparklines. Service health table with status chips. |
| 22 | Interactive table | `assets/22-interactive-table.html` | Search box + filter buttons. Sortable headers with `aria-sort`. Row hover. Vanilla JS search, filter, sort. |
| 23 | System architecture | `assets/23-system-architecture.html` | Layered node diagram (clients → gateway → services → data → external). SVG connectors. Legend bar. |
| 24 | Timeline / Gantt | `assets/24-timeline-gantt.html` | CSS grid gantt by week. Group headers. Task bars with progress. Milestone marks. "Today" highlight. |

### Tower

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 25 | Ballot reading surface | `docs/proposals/prototypes/ballot-surface.html` | One decision, the owner-ruled order: question, lesson, Current and In the wild (side by side only when both fit), every option in one shape with recommended first and full-width highlighted code, recommendation panel, folded long form. Tower Focus Mode renders the same layout. |

`assets/index.html` is the gallery of all 24.

## Generation workflow

1. **Identify the scenario** — report or tool; match the request to a use case above.
2. **Write the story** — for a report, choose the shape, then write every act in ELI5 prose with one concrete example each; strip ids, counts, and outline-mirroring sections; run the "so what" test. Ask the owner first if you want a fluid shape.
3. **Write the design plan** — subject, audience, one job, hero thesis with its journey or findings, signature moment, layout; check it against the generic version and revise.
4. **Read the reference file** — the matching `assets/XX-*.html` plus `theme.css`.
5. **Delegate to Sol** with the brief template; Sol sets the story into the pattern, adding nothing.
6. **Verify** — `node .agents/skills/html/scripts/check.mjs <file>`, then open the page: screenshot at 1280 and 768, exercise every control, read the console, and read it once as the owner.

### Key rules

- Always include `<!doctype html>`, `<meta charset="utf-8">`, the viewport meta, and `<meta name="color-scheme" content="dark">`.
- All CSS inline in one `<style>` block that starts with `theme.css` then `fonts.css`; all script inline in one `<script>` block.
- No external dependencies: no CDN links, no linked webfonts (embedded data URIs only), no frameworks, no build step, no `<link>`.
- Semantic HTML; buttons are `<button>`; tables are tables.
- Responsive to 360px; verify at 768px and 1280px.
- Generate complete, working pages; never partial snippets.
- When no use case matches, combine the two closest patterns; the identity does not change.
