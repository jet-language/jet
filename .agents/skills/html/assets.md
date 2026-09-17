# HTML asset catalog

Read this file when choosing a reference page. Each asset is a complete page in the Jet identity. Read the closest file for layout mechanics only; a report's structure comes from its approved shape, never from an asset's outline.

Reports: 03, 11, 12, 13–17. Tools: 01, 02, 04–10, 18–24.

## Exploration

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 01 | Code approaches comparison | `assets/01-exploration-code-approaches.html` | Multi-column layout showing 2-3 code solutions side-by-side. Prompt panel at top. Each column: code block + pros/cons + verdict status. CSS grid columns. |
| 02 | Visual design exploration | `assets/02-exploration-visual-designs.html` | Gallery of design alternatives. Thumbnail cards with hover states. Side-by-side comparison with annotations. |

## Code

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 03 | PR code review | `assets/03-code-review-pr.html` | Header panel with PR meta (author, branch, status chips). Diff-style code blocks with line-by-line comments. Severity chips (note/attention/critical). Summary panel with counts. |
| 04 | Code understanding | `assets/04-code-understanding.html` | Annotated source view. Callout panels explaining key sections. Flow arrows connecting code blocks. Collapsible detail sections. |
| 05 | Design system | `assets/05-design-system.html` | Token tables (colors, spacing, typography). Component swatches. Live examples in categorized sections. |
| 06 | Component variants | `assets/06-component-variants.html` | Grid of component states/variants. Each cell: component render + props table. Visual diff between variants. |

## Prototyping

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 07 | Animation prototype | `assets/07-prototype-animation.html` | Interactive animation demo area. Easing curve selector. Play/replay controls. CSS animation with JS toggles. |
| 08 | Interaction prototype | `assets/08-prototype-interaction.html` | Clickable UI prototype. State transitions. Micro-interaction demos. |

## Communication

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 09 | Slide deck | `assets/09-slide-deck.html` | Full-viewport slides with `scroll-snap`. `.slide` per slide. Placard headings. Progress marks. Arrow-key / scroll navigation. |
| 10 | SVG illustrations | `assets/10-svg-illustrations.html` | Inline SVG artwork and annotated diagrams in the palette. |
| 11 | Status report | `assets/11-status-report.html` | Placard values. Progress bars. Quiet/attention/critical status. Section-per-team pattern. |
| 12 | Incident report | `assets/12-incident-report.html` | Timeline layout (a real sequence, so it is numbered). Severity chips. Impact scope. Action items with owners. |
| 17 | PR writeup | `assets/17-pr-writeup.html` | Structured PR description. Before/after comparison. Testing checklist. Reviewer notes. |

## Diagrams and research

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 13 | Flowchart diagram | `assets/13-flowchart-diagram.html` | CSS-drawn flowchart with positioned panels and SVG arrows. Decision diamonds. Parallel paths. No diagram library. |
| 14 | Feature explainer | `assets/14-research-feature-explainer.html` | Concept breakdown with visual aids. Progressive disclosure from simple to complex. |
| 15 | Concept explainer | `assets/15-research-concept-explainer.html` | Educational layout. Diagram + text pairing. Key-concept callouts. |
| 16 | Implementation plan | `assets/16-implementation-plan.html` | Phased plan (a real sequence). Dependency arrows. Effort estimates. Milestone markers. Collapsible detail per phase. |

## Custom editing UIs

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 18 | Triage board | `assets/18-editor-triage-board.html` | Kanban columns (drag-drop). Cards with priority chips. Filter toolbar. Sticky column headers. |
| 19 | Feature flags | `assets/19-editor-feature-flags.html` | Toggle switches. Flag list with metadata (environment, rollout %). Search/filter. Edit-in-place. |
| 20 | Prompt tuner | `assets/20-editor-prompt-tuner.html` | Split panel: editor + preview. Parameter sliders. System/user message sections. Run button. Output area. |

## Data and visualization

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 21 | Dashboard | `assets/21-dashboard.html` | Placard-value cards with change indicators. CSS bar chart + SVG donut. SVG sparklines. Service health table with status chips. |
| 22 | Interactive table | `assets/22-interactive-table.html` | Search box + filter buttons. Sortable headers with `aria-sort`. Row hover. Vanilla JS search, filter, sort. |
| 23 | System architecture | `assets/23-system-architecture.html` | Layered node diagram (clients → gateway → services → data → external). SVG connectors. Legend bar. |
| 24 | Timeline / Gantt | `assets/24-timeline-gantt.html` | CSS grid gantt by week. Group headers. Task bars with progress. Milestone marks. "Today" highlight. |

## Tower

| # | Use case | File | Pattern summary |
|---|---|---|---|
| 25 | Ballot reading surface | `docs/proposals/prototypes/ballot-surface.html` | One decision, the owner-ruled order: question, lesson, Current and In the wild (side by side only when both fit), every option in one shape with recommended first and full-width highlighted code, recommendation panel, folded long form. Tower Focus Mode renders the same layout. |

`assets/index.html` is the gallery of all 24.
