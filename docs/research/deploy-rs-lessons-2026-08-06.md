# deploy-rs remote deploy lessons (2026-08-06)

Source: [Deploy NixOS Declaratively With deploy-rs](https://www.youtube.com/watch?v=8gh4YXi_Cgk) (Vimjoyer), cross-checked with [serokell/deploy-rs](https://github.com/serokell/deploy-rs).

Mine artifacts: `/tmp/jet-youtube-8gh4YXi_Cgk.manifest.json`, `/tmp/8gh4YXi_Cgk.claims.json`.
Captions were auto-only; mechanism claims use the README as primary evidence.

## Lessons → Jet tracking

| Lesson | Jet risk | Guard |
|---|---|---|
| Imperative SSH / `nixos-rebuild --target-host` becomes the beginner remote path | Two deploy cultures; fails I8 | `jet deploy` only (`D-ECO-FLEETVERB1=A`); cards #322–#835 |
| Magic rollback: confirm controller can still reach the host after switch | Facade health-file checks miss lockouts | Open ballot **D-JOS-FLEETHEALTH1** on #834; amends `D-JOS-FLEETROLLOUT1` health meaning |
| Split SSH connect user vs activation privilege (wheel + root) | Root SSH as silent default; weak proof | Open ballot **D-JOS-FLEETPRIV1** on #832; amends `D-JOS-FLEETTARGET1` |
| Local build → copy → activate | Fake push scripts close as “done” | #832 + epoch-7 P3 facade reopen; no shell-only closeout |
| Install ≠ deploy (`nixos-anywhere` vs deploy-rs) | One verb does both and confuses beginners | Keep `jet os image` / `vm prove` separate from `jet deploy`; onboarding #1033 family |
| Scale by adding nodes/profiles, same mechanism | Parallel tools (colmena/nixinate shims) | I8; do **not** ballot compatibility shims — learn UX pain into diagnostics/examples |
| Validate deploy graph before push (`deployChecks`) | Backend discovery as user errors | Sema E1242–E1245; plan/proof before switch |

## Do not ballot

- Parallel deploy ecosystems or colmena/nixinate shims (I8).
- Install vs deploy seam (already separate JetOS paths).
- Fleet verb name (`D-ECO-FLEETVERB1=A` = `jet deploy`).
- Staged proof-gated rollout skeleton (`D-JOS-FLEETROLLOUT1=A`).

## Cards that own the lessons

- **#322** — inventory / one mechanism for 1..N hosts; declarative not SSH.
- **#832** — transfer + activation; privilege ballot; real artifact path.
- **#833** — canary/waves on the same rollout object.
- **#834** — stop/rollback; health ballot (reconnect confirm).
- **#835** — durable per-host proof and rollout report.
