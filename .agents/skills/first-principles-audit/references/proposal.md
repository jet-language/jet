# First-principles proposal

Write the retained proposal at `docs/proposals/<area>-<slug>.md` in direct `simple` prose. Do not hard-wrap paragraphs. Use tables, code, and diagrams so the owner can judge the surface instead of trusting a description. Give every material claim a code block, before/after pair, tree, or table.

Load and follow the `simple` skill before writing. Use short, direct sentences and common words; avoid dense jargon, stodgy report-speak, and needless repetition.

Keep this order:

1. **Executive summary.** State the finding, one idea, why now, concrete payoffs, ballot asks, and what does not change.
2. **Problem.** Give a short setup and a side-by-side evidence table. Show every divergent form in the declared closure, its `file:line` home, and its defect.
3. **Proposal.** Work element by element with real before/after pairs. For each element, show beginner, intermediate, and expert code. The beginner rung types nothing; every higher rung is explicit opt-in and does not change the lowest rung's behavior.
4. **Final vision.** Close with complete real-syntax programs from default to expert extreme, today's code beside proposed code for the same job, and a tree or layout diagram of the end state. Mark every not-yet-ratified line `proposed`.
5. **What this unlocks.** Cover the declared domain's common case and extremes.
6. **What stays.** Keep only walls, zero-cost behavior, or spellings that win on merit. Never keep something only because it shipped.
7. **Decisions for the owner.** Map direction-level choices to a ballot slate. Each ballot must stand alone so the owner can adopt a subset.
8. **Adoption boundary.** State which ballot authorizes each change and which Tower card or ballot owns implementation work. Do not turn the proposal into an implementation task list.

## Non-negotiable design checks

Unify, simplify, and power up together. Preserve every existing capability, add natural instances, and delete mechanisms rather than introducing a parallel one. Test the idea against trivial and demanding work in the declared area. If it hollows beginner defaults, needs an invariant carve-out, duplicates a mechanism it claims to delete, or makes machine repair less deterministic, kill or narrow that slice.

The surface is the product. Every unification must look better on the page. A syntax-area rethink also accounts for open lexical space: prefix and suffix conventions, reserved namespaces, sigils, casing, and a proposed use or stated reservation.

## Magic/control ladder

For every automatic default, show the three exits beside the worked example:

| Exit | Required evidence |
| --- | --- |
| See | A real command and output show what the default resolved. |
| Replace | Real explicit syntax performs the same job without the default. |
| Refuse | A project-level switch disables the default. |

Check ceremony creep: the common case must not gain a marker. Check magic without an exit: every default has a ledger, an explicit spelling, and a project switch. Default to opt-out when the compiler can decide correctly; reserve opt-in for choices it cannot decide.

Mark every touched decision as ratified, amended, or new. A ratified rule is respected unless a ballot names its amendment. Frozen walls stay closed unless a ballot explicitly reopens them. Breaking changes are acceptable in this greenfield design when the final surface is better.

## Synthesis before prose

Name the one idea in one sentence. Build an evidence table of every shadow mechanism in the closure that does the same job, with its home and defect. Apply the unification test: every existing feature must fall out as an instance of the idea, every known missing feature must use the same mechanism, and any feature that resists becomes evidence against the idea or a documented wall.

Find the small grid of orthogonal axes and name recurring ring shapes such as point/delta or exact/approximate/measured. State the conservation-style law that unifies the area and show ratified rules as instances of it. Run the coding-agent pass over verdict fidelity, latency, actionability, context economy, and repair determinism. Keep the kill-check in the root: kill or narrow a slice that hollows beginner defaults, needs an invariant carve-out, duplicates a deleted mechanism, or makes machine repair harder.
