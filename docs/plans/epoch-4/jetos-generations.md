# jetos nameable generations (#2 / c27)

Far-horizon vet. Card body is one line: "Named OS generations." Card `plan`
field in tower.json is corrupted — it's card #91's (generic modules,
`D-GENMOD1/2`) plan text, copy-pasted in error; both cards were carded from
the same 2026-06-26 Ideas.md sweep and both picked up the same 2026-06-26
epoch-restructure log line, which is likely how the fields got cross-wired.
Not fixed here (tower.json is out of scope for this task) — flag for
whoever next has write access to reconcile.

## Where this actually lives

Ignore the corrupted plan field; the real design lives in
`docs/plans/epoch-4/vision.md` and `README.md`, already sketched
under "jetos":

```jet
module system.laptop {
    imports: find("./modules")
    packages: [default.[firefox, ghostty, ffmpeg]]
    services: { tailscale: Service.{}, pipewire: Service.{} }
    users: { nate: User.{ shell: fish, groups: [wheel] } }
}
```

```
$ jet switch --name "pre-gpu-driver"
$ jet store generations
$ jet store rollback
```

This is Phase D of the epoch-4 sequencing (`README.md` §Sequencing):
"jetos realization: single-host switch/generations/rollback; fleet push
realization; ISO / VM test harness" — milestone `OS2` in
`implementation.md`: "build generator + activation … VM switch -> rollback;
power-cut sim boots prior generation." Named (not just numbered) generations
is a UX refinement on top of OS2's base switch/rollback mechanism, not a
separate mechanism — same one-canonical-path spirit as I8.

`jetos` generations/rollback genuinely is e4 scope: it's inside the "jetos
Phase 2 ISO" exit criterion for epoch e4 in `.tower/tower.json`
(`epochs[3].exitCriteria`). It is not out-of-scope work being smuggled into
e4 — it's just very late in e4's own sequencing.

## Why there's no honest e4 slice right now

Phase D has a standing gate, restated verbatim in `implementation.md`:
"Do not start until prerequisites land: M12 layer 3 / pure eval foundations,
Phase A dispatch, canonical role modules, and enough hangar realization."

Checked against the current "Shipping Baseline" in `README.md`: Phase A
(dispatch seam + `pkg.jet` canon + module-declaration role form + filename
cleanup) is explicitly *not done* — the README says "Do not assume the
shipped implementation already matches the current canon. The first work in
Epoch 4 is a reconciliation pass." Phases B and C (script deps, image
capture, fleet parse, env/dev split, services, secrets, Nix bridge) sit
between Phase A and Phase D and are also not started. There is no code to
write for generations/rollback today — the activation/build-generator
machinery it would sit on doesn't exist yet.

## Syntax note (not a new ballot — just drift to fix when Phase D starts)

`vision.md`'s example (`jet switch --name …`, `jet store generations`, `jet
rollback`) predates `D-JPK-DISPATCH1=B` (ratified 2026-07-02): "Jetpack /
jetos verbs must cross a git-style process boundary (`jetpack`, `jetos`, or
future engine binary) … Do not pile U11-U19 onto the old in-process
`jet::Jetpack::run` path." Under that ratified rule the verbs should be
`jetos switch`, `jetos generations`, `jetos rollback` — not `jet switch` as
currently written in the vision doc. This is a doc-consistency fix, not an
owner decision (the dispatch rule already settled it); flagging so it isn't
mistaken for a live open question. No ballot needed for this.

## Ballot check

**No ballot to raise now.** The one genuine owner-facing surface question —
exactly how naming works (`--name` flag at switch time vs. a separate
`jetos rename <gen> <name>` vs. auto-generated names with opt-in override;
collision handling; how `jetos generations` lists/sorts named vs. numbered
entries) — is real, but it's premature to spend an owner decision on UX
details for a feature that sits behind three unstarted phases of prerequisite
work (A, B, C) that will themselves surface and settle a lot of adjacent
naming/verb conventions first. Raising it now risks the decision going stale
or being re-litigated once Phase A-C conventions exist, and adds to decision
queue clutter for something not actionable for a long time. Revisit when
Phase D's prerequisites are actually landing.

## Phase recommendation

**Frozen — propose to owner.** Nothing genuinely actionable in e4 right now
beyond the phase gate that already exists in the epoch-4 plan. Recommend:
park this card (or fold its scope as a line item under Phase D / OS2 in
`implementation.md` rather than tracking it as a standalone card) until
Phase A-C land. When Phase D starts, open one ballot then for the exact
naming/verb surface — don't ballot it in isolation today.
