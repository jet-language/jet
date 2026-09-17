# HTML implementation and closeout

Read this file before delegation. The orchestrator resolves every product and design choice, then passes one complete brief to one authorized OMP implementation worker.

## Inputs to collect

Before delegation, collect:

- the user's goal;
- the actual content and data;
- for a report, the approved status, proposal, hybrid, or owner-approved fluid shape and the written story, act by act, in ELI5 prose;
- for a tool, the resolved behavior, states, controls, and interactions;
- the exact target paths and allowed file scope;
- existing project conventions;
- the selected reference asset and always `theme.css`;
- all constraints; and
- acceptance and verification requirements.

Resolve the design plan before delegation. Do not ask the worker to resolve a product or design choice. Use exactly one authorized OMP implementation task selected under `AGENTS.md`, through `task` or `hub`. Do not invoke raw Codex CLI. If the host cannot provide the required adapter, report that routing failure instead of claiming a handoff.

## Brief

Start the brief with this executor role so the worker implements directly:

```text
Role: You are the single authorized OMP implementation worker for this HTML handoff. Implement directly. Do not read this skill's SKILL.md, do not delegate, do not ask questions, and do not change scope.
Goal: <the complete user outcome>
Files/scope: <exact write targets, allowed reads, and forbidden paths>
Content/data: <all real copy, values, records, and source material; for a report, the act-by-act story, final prose, ready to set>
Reference asset: <selected asset path(s) and the patterns to reuse; always `theme.css`>
Design plan: <subject, audience, the page's one job, the shape and its acts, the hero thesis and its journey or findings, the signature moment, and every resolved layout choice>
Constraints: <project conventions, technical limits, and resolved decisions>
Acceptance: <observable completion criteria; require a complete, self-contained working HTML page, never a partial snippet>
Verify: <the focused checker and checks the worker may run, plus evidence it must return>
```

The worker is an executor only. It may inspect allowed files, implement the declared page, and run the requested focused checks. It must not invent requirements, make unresolved product or design decisions, ask the user questions, orchestrate other agents, or edit unrelated files.

## Required page rules

- Include `<!doctype html>`, `<meta charset="utf-8">`, the viewport meta, and `<meta name="color-scheme" content="dark">`.
- Put all CSS inline in one `<style>` block that starts with `theme.css`, then `fonts.css`, then page rules that use only the tokens.
- Put all script inline in one `<script>` block. Include `hl.js` for every code block.
- Use no external dependencies: no CDN links, linked webfonts, frameworks, build step, or `<link>`. Embedded font data URIs are allowed through `fonts.css`.
- Use semantic HTML. Use `<button>` for controls and tables for tabular data.
- Generate a complete, working page, never a partial snippet.
- Keep the page responsive to 360px; verify it at 768px and 1280px.
- When no use case matches, combine the two closest asset patterns without changing the identity.

## Closeout

After the worker returns, the orchestrator reviews the diff and verifies the completed surface. Run the focused checker:

```text
node .agents/skills/html/scripts/check.mjs <file>
```

Then open the actual page in a real browser. Take screenshots at 1280 and 768, exercise every control and interactive path, read the console, and read the page once as the owner. For a report, apply the “so what” test to every screen and confirm the approved shape, story, headline hierarchy, density, and fluid-shape gate. For a tool, confirm the resolved state transitions, keyboard paths, empty states, and error actions. Report completion only after this browser evidence exists.
