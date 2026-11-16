# HTML report branch

Read this file only when the user explicitly requests a report page. A report gets the owner up to speed on the project and, when it proposes, shows a vision of the future. It is a story with an arc, never a rendering of a source document.

## Shape and owner gate

Choose one shape from the content and name it in the design plan:

| Shape | Use when | Acts, in order |
|---|---|---|
| **Status** | Reporting findings, audits, progress, or incidents | Where we were → Where we are → What comes next |
| **Proposal** | Proposing a direction, design, or plan | Where we are → Where we could be → What has to be decided |
| **Hybrid** | Reporting findings and proposing in one page | Where we were → Where we are (findings) → Where we could be → What has to be decided |

Any other shape is a **fluid shape**. Before delegation, ask the owner in one message which shape to use, why it communicates better for this content, and which required acts it keeps. Build only after the owner approves it.

## Story and hero

The first screen has the thesis and one picture of the arc. The thesis is one plain sentence that states what the reader must take away. A proposal uses a past → present → future journey with three to six real events. A status page uses two to four findings that changed the picture. Do not use counts as tiles or add legend clutter beyond source and date.

Keep the report to one linear scroll of three to five screens. Do not collapse content behind an expander. Put necessary precision in a short inline “More precisely” aside.

Before delegation, write the page as an outline of blocks, not paragraphs:

- eyebrow
- headline
- block type
- items

The outline is the brief. Each screen must pass the “so what” test: the owner can state what they now know or must decide. The eyebrows and headlines alone must tell the story.

## Density and hierarchy

A report is scanned, not read. Apply these rules:

- Put the takeaway in every `h2`; never use the act name as the headline. Put the act name in the eyebrow.
- Use a structure for two or more items: grid, cards, two-column list, Before/After pair, numbered sequence, row per area, `ol`, `ul`, `table`, `pre`, or `figure`.
- Use prose as connective tissue: at most two `<p>` in a row, each under 60 words, and no `<p>` over 90 words. Every section has at least one structured element.
- Give each card one finding, item, or step. Use a bold lead of five to ten words, then at most two lines.
- Put a finding number in a `.value` placard with a label, not inside a sentence.
- Alternate surfaces down the page. Do not use the same layout for two consecutive sections.
- Keep a “More precisely” aside to one or two sentences under 40 words. Make it a section or cut it when it needs more.

## Keep out

- Card numbers, decision IDs, ballot lists, and file paths. Tower owns them. Name each pending decision as a question, its stakes, and what changes with each answer.
- Counts presented as meaning. Keep a number only when the number is the finding.
- Sections that mirror the source outline. Merge or drop them until each act is one story beat.
- Progress bars, tables, and meters in proposals. They belong to status pages when a measured value is the finding.
- Interactive filters and sorting unless a status page has a list the owner must search.
- Decorative diagrams. Add a diagram only when structure or sequence is the finding.
- Prose walls.

## Voice

Use ELI5 by default: lead with the point, add one new idea at a time, define each term on first use, and give one concrete example per act. Use short declarative sentences. Add a short inline “More precisely” aside only when exact detail changes a decision. Keep identifiers, commands, and diagnostic text verbatim in examples.
