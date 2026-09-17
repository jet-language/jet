---
name: html
description: >-
  Build a self-contained, zero-dependency HTML page only when the user explicitly
  asks for HTML, an interactive page, or a visual/rendered page. Do not trigger
  for an ordinary review, report, plan, or explanation unless HTML is requested.
---

# HTML

Use this skill only for an explicit HTML or interactive-page request. The result is a complete browser page with no dependencies. It keeps one visual identity: jet black, signal red, monospace placards, and a quiet instrument panel.

An ordinary review, report, plan, or explanation stays in its native format. Do not convert it to HTML unless the user asks for the page.

## Route

- **Report:** For a requested status, findings, incident, proposal, PR, or explainer page, read [report.md](report.md). The report branch owns its story, shape, density, and owner gate.
- **Tool:** For a requested editor, prototype, gallery, dashboard, or other interactive surface, read [tool.md](tool.md). The tool branch does not use report acts.
- **Design:** Read [design.md](design.md) for either branch before delegation.
- **Identity:** Read [identity.md](identity.md) for either branch. It is the authority for `theme.css`, `fonts.css`, `hl.js`, palette, typography, motion, accessibility, and page mechanics.
- **Assets:** Read [assets.md](assets.md) when choosing a reference page. Read the closest asset for mechanics, not as a report outline.
- **Implementation:** Read [implementation.md](implementation.md) before any handoff. It contains the complete brief, worker boundary, focused check, and real-browser closeout.

## Permission and completion

- **Requested outcome:** One complete HTML report or tool for the user's explicit visual request.
- **Supplied inputs:** The goal, real content and data, report shape or tool behavior, exact target paths, project conventions, selected references, constraints, and acceptance checks.
- **Allowed child result:** Exactly one authorized OMP implementation worker may implement the declared page. It may not resolve product choices, open another workflow, delegate, or change scope.
- **Completion owner:** The orchestrator owns user communication, product and design decisions, the brief, integration, and final browser verification. The worker owns only the delegated page implementation and focused evidence.
- **Return point:** The worker returns the page and evidence to the orchestrator for diff review and browser verification.
- **Stopping condition:** Stop only after the page is complete, the focused check passes, and the orchestrator has opened the real page in a browser, exercised its controls, read the console, and read the result as the owner.

The orchestrator must not write the HTML or site implementation itself. It resolves the request, delegates one concrete brief to the authorized worker selected under `AGENTS.md` through `task` or `hub`, and reports a routing failure if the host cannot provide that adapter. The worker implements only the declared target and may run only the requested focused checks.

For reports, the orchestrator must also apply the report branch's owner gate and read the rendered page once with its "so what" test. For tools, it must verify the declared interactive paths. Neither branch opens a new agenda after completion.
