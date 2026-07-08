# jetos Studio — mockup notes

Three families, one app. Same domain data (host `halcyon`, gen 47, cachyos
kernel, KDE Plasma + stylix, real package/service/secret/fleet content), same
8 screens, same source-backed edit model. Differ in tone + density only — no
theming, no metaphor, copy purely functional.

Shared model (from `Jetpack/CLI.rs` `serve_studio`): every option value is a
projection of `system.jet`. A Studio edit is a `set-option` transaction that
rewrites one source line and returns a diff. Run actions map to
`jet os {check,plan,build,proof,generations}`. That truth drives the whole UI:
**toggle = line of source**, shown live in every family.

## Screens (all functional, no stubs)

overview · modules+options (the heart) · pending review + build/switch ·
generations timeline + diff + rollback · packages (install-state) · services
(status/logs/restart) · secrets (names+age, never values) · hosts/fleet.

## Family A — Carbon (dark, dense, engineered)

- Rationale: pro who lives in tool all day. Linear/Zed density. Every value
  monospaced + tabular; strict alignment; scan not read.
- Signature: **status band** — sticky one-line strip of labeled state lights
  (BUILD OK · GEN 47 · PENDING 3 · PROOF SIGNED). Whole-system state one glance.
- Source-link: per-module `system.jet:NN` bezel button flips right-hand source
  pane to that block + live pending diff. Option rows carry SET (magenta) vs
  DEF (grey) badges = HA config-vs-source duality made explicit.
- Transplants:
  - Apple System Settings → two-pane grouped option rows, inline controls.
  - Home Assistant → config-vs-source duality; SET vs DEFAULT provenance tag.
  - Tailscale → dense fleet rows (addr, role, gen, kernel, state, last-seen).
  - Proxmox → sticky source pane + task-step list on build/switch.
  - Synology Package Center → per-row Install / Installed / Update buttons.
  - Portainer → colored service state badges (running/stopped/scheduled).
  - Vercel → generations timeline + one-click rollback + closure delta.
- Line-select keys (L1..L4) down option rows = fast keyboard target labels,
  not decoration; number keys 1-8 jump sections.
- UX risks: density can fatigue — leaned on generous row padding + grouping.
  Magenta SET vs grey DEF must stay colorblind-safe → also carries text badge.

## Family B — Paper (light, calm, editorial)

- Rationale: counter-position to every dark dev tool. Stripe-docs calm.
  Whitespace + type hierarchy do the work; the only dark surface is code/logs.
- Signature: **fading hairline** — thin rule solid→transparent, leads label to
  value (stat cards) and step to step. Same device on the status band dividers.
- Source-link: each module header is a clickable `mod · system.jet:NN` link
  that loads the source aside; aside is persistent (always shows current block
  + pending diff) so the source is never hidden behind a click.
- Transplants:
  - Stripe docs → editorial hierarchy, eyebrow labels, roomy line length caps.
  - Vercel → deploy-timeline generations (dot rail, current pill, promote/roll).
  - Home Assistant → set vs default tag on every option.
  - Tailscale → fleet table density kept even in the airy layout.
  - Apple System Settings → grouped option cards with one control per row.
- UX risks: light theme + long tables can feel sparse → hairline dividers and
  soft header wash add structure without boxing everything. Contrast checked:
  ink #1B2733 on paper #F7F9FC ≈ 13:1; sub #5A6B7B ≈ 5.2:1 (AA).

## Family C — Pulse (dark, bold, one hot highlight)

- Rationale: energetic, confident, but disciplined. Heavy display type; big
  numbers. Charm-school polish without the noise.
- Signature: **single gradient glow** — exactly one hot→hot2 element per
  screen; its absence is also signal. Overview: the live generation tile.
  Modules: the selected option row. Pending: the running build step + bar.
  Generations: the current-gen node. Everything else is cold violet + `cool`
  (violet) secondary — nav active marker is `cool`, never hot, so the one hot
  thing never competes.
- Source-link: click any option row → it becomes the glow AND loads its
  `system.jet` line in the aside. Selection and source are the same gesture.
- Transplants:
  - Vercel → the promote/rollback timeline + "current" as the loud element.
  - Tailscale → fleet row density; online/offline state dots.
  - Synology → install-state package buttons.
  - Portainer → service state badges + restart/run-now/start actions.
  - Home Assistant → set/default provenance tags.
- UX risks: gradient glow can tip into gaming-UI kitsch → hard rule of ONE per
  screen, thin 1.5px gradient borders not fills, glow reserved for the subject
  the user acts on. Warn/error stay amber/red (not hot) so status ≠ accent.

## Self-critique (killed generic tells)

- No donut/gauge charts, no generic KPI-donut dashboard. Stats are plain big
  numbers with a provenance foot line, because the value (gen number, kernel,
  closure size) is the point, not a ring.
- Sidebar+cards is unavoidable for an admin app, so each family earns
  distinctiveness elsewhere: Carbon's status band + line-select keys, Paper's
  fading-hairline lead lines + persistent source aside, Pulse's one-glow rule.
- Source-backed editing is the through-line the brief demanded: in all three
  the toggle/row visibly ties to a `system.jet` line + the exact diff to write.
  This is the anti-"UI hides the truth" stance, borrowed from HA's YAML duality
  but made always-visible instead of a hidden mode.
- Keyboard: visible focus ring in every family; number keys 1-8 jump sections;
  option rows are real focus targets.

## Constraints met

Self-contained (inline CSS/JS, system fonts, no CDN, works file://). Light JS
only for nav, source-pane swap, service log pick, and the build/switch
simulation. Responsive to laptop + narrow. Each file well under 300 KB.
