# jetos Studio — mockup notes (v2)

4 archetypes, one shared visual system (DESIGN-BRIEF.md GUI tokens: ground
`#0B1119`, panels `#16202E`, accent `#3FC6FF`, ok/warn/err, system font +
ui-monospace, 6px radius). Same capability set in all 4 — modules w/ typed
options, packages, supervised services, age secrets (names only), generations
(atomic switch/rollback), fleet/hosts. Archetype decides the *experience*.

Core truth wired into every file: a GUI edit = a `set-option` transaction that
rewrites ONE `system.jet` line and returns a source diff (per
`crates/jetpack/src/CLI/studio_transactions.rs`). Run verbs check/plan/build/
proof/generations map to `jet os <verb> <host>`. Data mirrors the real fixture
`tests/fixtures/jetpack-config/config.jet` (host halcyon, KDE Plasma, cachyos
kernel, stylix-ish theme options, `mine` core packages, wifi age secret).

Distinct-core-loop test (must differ; they do):

| file | core loop (one sentence) |
|------|--------------------------|
| settings | Pick a category, flip one option's control, confirm the single source line it writes, apply. |
| changeset | Stage edits from anywhere into one changeset whose primary screen is the diff review + impact, then apply the whole reviewed set as one build. |
| opsboard | Watch the fleet health board; when a tile goes red, drill into it, fix or roll back from the drawer, watch it go green. |
| projectional | Read the config AS the document; edit each value through an inline control so you change system.jet without typing syntax. |

Four different information architectures: hierarchical navigator / staged-diff-
review canvas / health-dashboard-with-drill-in / single live source document.
No two share a primary screen.

Every archetype answers the two acceptance questions in its own idiom:
- **change my kernel** — settings: Boot & kernel category, kernel dropdown w/
  "rebuild + reboot" tag. changeset: stage `boot.kernel` → review reboot impact.
  opsboard: drill into host → Kernel & boot editor. projectional: inline
  `boot.kernel [.CachyOS ▾]` dropdown w/ rebuild warning in the popover.
- **roll back when it breaks** — settings: Generations category, per-generation
  "Roll back to N". changeset: History tab, rollback stages the inverse as a
  reviewable changeset. opsboard: red alert + drawer "Roll back to 41 & reboot",
  failed generation shown struck-through. projectional: generation picker turns
  the doc into a time machine — view gen 40's values, "Restore these values".

---

## settings.html — grouped settings app

**Core loop:** find the one setting, change it confidently, apply.

**Rationale:** beginner tier. Lowest ceremony for "change one thing." Sidebar
groups map 1:1 to config.jet option namespaces (boot/perf/network/desktop/apps
+ services/packages/secrets/generations). Signature = the **pending source
tray**: a docked bottom bar that accumulates the exact `+`/`-` source lines +
impact chips as you toggle, making the "GUI edit = one system.jet line" truth
visible before apply. Nothing hits the machine until Apply.

**Transplants:**
- Apple System Settings — left category rail + right detail pane; search box
  filters the tree; per-row label/description/control-on-right.
- iOS/macOS toggle switch shape.
- GitHub/VS Code source-control tray — the collapsible pending bar w/ count +
  Discard/Apply.
- Terraform-style `+`/`-` diff lines in the tray preview.

**UX risks:** many namespaces → deep tree; search mitigates. Per-change apply
tempts churn (each apply = a generation); tray batches multiple edits into one
build to counter. Grouping is our editorial call, not the source order — risk
of a setting being "somewhere else than expected."

## changeset.html — review-first (git/PR model)

**Core loop:** stage edits, read the full diff + impact, apply as one reviewed
change — never surprised by an apply.

**Rationale:** the safety tier. The diff review IS the landing screen; the edit
surface (left rail form + staged list) is subordinate. Impact ledger (services
restarted, packages rebuilt, reboot, generation N+1) sits *beside* the raw diff
so consequence is legible without reading source. Per-hunk include/exclude lets
you split a changeset. Build & switch = merge; Build only = build without switch.

**Transplants:**
- GitHub PR "Files changed" — unified diff, hunk `@@` headers, per-hunk context,
  additions/deletions coloring, `+N −N` filestat.
- GitHub PR per-file/line viewed-checkbox → our per-hunk include toggle.
- Terraform `plan` — `~`/`+`/`−` change symbols + a human "N to change" summary
  before apply.
- Vercel/Netlify deployment summary card — the plain-language "What this changes"
  list.
- Datadog/PR "impact" panel — the ledger cards.

**UX risks:** heavier than settings for a one-toggle change (must open review) —
acceptable; this tier trades speed for certainty. Excluded-hunk state can
confuse ("why didn't it apply?"); we keep excluded hunks in the changeset + note
it. Authoring edits via a key/value form is less discoverable than a settings
tree — this archetype assumes the user knows option keys (datalist autocompletes).

## opsboard.html — dashboard-first (ops console)

**Core loop:** operate many machines from a live health board; drill in only to
fix a misbehaving thing.

**Rationale:** the fleet tier — editing is rare, operating is constant. Landing
= KPI strip + alerts + host list (per-host generation ribbon) + service grid.
Config editing is a drawer you open *from* a red thing. Kernel-panic story is
the centerpiece: gen 42 failed to boot, auto-fell-back to 41, board shows the
alert w/ one-click clean rollback; the drawer diagnoses (what/why/fix, per
diagnostics.md voice) and shows the offending option diff. Fixing heals the
board live (tile → green, alert clears).

**Transplants:**
- Proxmox VE / Portainer — datacenter/host tree w/ status glyphs, service state
  grid, resource summary tiles.
- Tailscale admin — machine list rows w/ status dot + last-seen + drill row.
- Grafana/Datadog — KPI stat tiles across the top, "needs attention" alert feed
  w/ inline actions.
- GitHub/CI generation ribbon — colored run history squares (green/red, current
  outlined).
- PagerDuty — alert row w/ primary remediation button (Roll back / Restart).

**UX risks:** editing config from a drawer is shallow vs the other three — by
design; deep edits could deep-link to a settings/projectional view. A fleet of
3 is small; real fleets need grouping/filtering (out of scope for the mock).
Auto-heal animation must respect reduced-motion (it does; pulse gated).

## projectional.html — source-first projectional editor

**Core loop:** the file is the truth; edit values inline through controls that
remove the syntax tax — GUI and text are the same artifact.

**Rationale:** the expert tier. The document is `config.jet`, syntax-highlighted,
line-numbered — but every option *value* is a live widget in place (toggle,
dropdown, number stepper, chip list, sealed-secret). Editing a widget rewrites
that line; a git-style gutter marks changed lines. A **Text** toggle reveals the
exact source the projection produces — proving one artifact, two surfaces. The
generation `select` turns the doc into a time machine: view gen 40's values
read-only, "Restore these values" stages them as edits vs live = rollback.

**Transplants:**
- JetBrains MPS — projectional editing: you manipulate structured cells, not
  free text; the value is a control, the rest is rendered syntax.
- Dark (darklang) — structure editor where the program is edited through typed
  affordances, not a text buffer.
- Hazel — typed holes / values-as-editable-nodes in a rendered program.
- GitHub/editor change gutter — per-line changed markers.
- npm/dependabot version dropdowns — the kernel-profile "version picker" widget.
- Retool/observable inline-widget-in-code feel for the embedded controls.

**UX risks:** highest-fidelity, highest-complexity — dense for a beginner (this
is the expert surface, I8-compatible: same set-option mechanism, different
entrypoint). Rendering fidelity: the projection must round-trip to real Jet
source (Text view demonstrates; real impl must reuse the formatter, per the
formatter-round-trip rule). Inline contenteditable strings need careful
paste/enter handling. Not every construct is a widget (packages/services headers
stay rendered-but-static in the mock) — a full impl must widgetize all values.

---

## Shared implementation notes

- Self-contained: inline CSS/JS, no CDN/fonts/assets, works `file://` offline.
  Each < 30KB (budget 300KB).
- Keyboard: visible focus everywhere; drawer/pop close on Escape; number widgets
  arrow-key steppable; `prefers-reduced-motion` kills transitions/pulse.
- Copy: plain functional words, active voice, sentence case; no invented jargon,
  no metaphors, no branding beyond product names (per brief + v1 rejection).
  Errors/diagnostics in what/why/fix voice.
- Mock data is a faithful subset of the halcyon fixture so diffs, option keys,
  and enum variants are real (`.CachyOS`, `.ScxLavd`, `.Limine`, Plasma enable,
  wifi.age). Real wiring = POST `/studio/transaction` + `/studio/run`; the mocks
  simulate locally so they demo without the jet binary.
- Not yet real: multi-host data is illustrative; secret re-key is a stub button;
  projectional widgetizes option values only (not package/service sub-records).
  Flagged here, not hidden.
