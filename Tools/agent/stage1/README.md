# Stage-one compiler tooling

These scripts drive retained bootstrap host binaries and generated compilers.
Code lives here; candidates, locks, logs and rung output stay in
`$HOME/.cache/jet-dev`. `JET_REPO` overrides the checkout derived from this directory.
Use the repository scripts directly; cache copies are retained only for running agents.

```sh
Tools/agent/stage1/fast-cand4.sh cand29f 12
Tools/agent/stage1/ladder.sh -r L1,L2,L3 "$HOME/.cache/jet-dev/cand29f/jetc0" "$HOME/.cache/jet-dev/stage1/ladder29"
Tools/agent/stage1/selfcheck.sh -c "$HOME/.cache/jet-dev/cand29f/jetc0" -b
Tools/agent/stage1/fixedpoint.sh -c "$HOME/.cache/jet-dev/cand29f/jetc0" --stages 3
```

`fast-cand.sh` uses the retained host3; `fast-cand4.sh` uses host4.
They require the matching `host3.path`/`host4.path` cache file and overlay checkout;
`fast-loop*.sh` accept `JETC0_LOOP_HOST`, `JETC0_LOOP_OVERLAY` and `JETC0_LOOP_DIR`.
`lib.sh` documents the environment protocol, caps, backend pinning and locking.
`goldens.sh` stages fixtures from `fixtures.list`; reports and gates are sibling tools.
Linux systemd user scopes, flock, Node, Python runners and the project devshell are required.

The private assemble HOME intentionally still uses `.cache/jet-luna/compiler-bootstrap`.
It must match the running host4's embedded `Compiler/Bootstrap/Tests.rs` constants.
Change the compiler bootstrap files, both copies of `lib.sh`, `ladder.mjs` and
isolated-check consumers together with the host rebuild, not independently.
See [gates/README.md](gates/README.md) for growth and phase-profile gates.
