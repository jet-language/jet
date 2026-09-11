# Native JetOS rationale

JetOS is a standalone operating system. It does not build through the NixOS
module system; the earlier `--real` tier was a reskinned NixOS and is not a
product backend.

## Governing decisions

- `D-JOS-NATIVE1=A`: JetOS owns native system assembly.
- `D-JOS-STORE1=A`: Hangar is the on-disk store; Nixpkgs closures may use a
  Hangar-managed compatibility root only as a migration substrate.
- `D-JOS-NIXEVAL1=C`: the product path contains no `nix` binary; Jetpack owns
  its evaluation, fetch, and build boundary.
- `D-JOS-NIXBACKEND2=C`: the Jet-to-NixOS realizer is migration tooling only.
- `D-JOS-PARITYBAR1=A`: the ratified architecture bar is recorded in the
  decision set, not maintained as a second plan here.

The underlying research is captured by the Epoch 7 anatomy, desktop-glue,
no-Nix-pipeline, and repository-inventory audits. Those audits remain the
evidence source for subsystem tradeoffs and reuse decisions.

## Priority source law

Ordinary contributions stay plain values. An expert override uses
`OptionValue.{ value, priority }` with `.Default`, `.Force`, or `.Priority(n)`.
Real option paths ending in `priority` remain ordinary. Explain retains the
wrapper and all contenders; realization receives only the selected value.

## Install trust law

`D-JOS-INSTALLTRUST1=A` is the jetos install-trust contract. It protects the
system without making Jet central review a permission to run software.

- **Freedom:** anyone may install software they choose or build. Owner-built
  local installs and `jet run` run without Jet notarization, catalog approval,
  or a category ban.
- **Capability boundary:** third-party and foreign apps run sandboxed and start
  with deny-by-default access to files, network, devices, and agents. A grant
  is explicit and is recorded through Studio or Jet configuration; capability
  escalation never follows from install or catalog presence.
- **Identity and provenance:** Hangar hashes and optional publisher signatures
  are trust UI signals. They identify the artifact or publisher and can provide
  supply-chain evidence; they do not decide whether owner software may run. An
  unsigned remote catalog artifact may warn, but it is not blocked by that
  warning. Catalog ranking may prefer signed publishers, but ranking is not a
  hard gate.
- **Recovery:** installs are committed as generations so a bad install can be
  rolled back atomically.
- **Policy:** family and enterprise locks are expert opt-in tightenings, not
  the beginner default. The same law applies to every future jetos device
  class unless a later ballot amends it; mobile implementation remains
  deferred by `D-VERDICT-480-1`.

### Migration alternative

The semantic importer remains central. The Jet-to-NixOS realizer is available
only through `jet os migrate compare-nixos <host> --out <dir>` for explicitly
labeled A/B comparison. Product commands cannot reach this backend; native
system assembly remains the product path.

## Ratified owner decisions

- B-E7-DESKTOPNS1=E: `services.desktop.*` with `.Auto` derivation and typed
  invalid-combination assertions.
- B-E7-BASELINE1=D: terminal and graphical baselines materialize every choice
  directly into `config.jet`; terminal is lean-modern, graphical is complete
  without requiring a command line, and expert edits are never silently healed.
- B-E7-IDENTITY1=E: `YY.MM` releases plus alphabetically ordered
  aviation-navigation codenames; first release is `26.10 "Apex"`.

