# Universal trust grants across Jet and Jetpack

**Card:** Tower #229. **Epoch 4.** **Scope:** planning slice for `D-WD1`.

## Goal

One authority graph covers code effects, package resolution, build actions, env
activation, services, images, fleets, and later jetos activation. Beginners see
one intent summary before a risky action runs. Experts can inspect the exact
grant, source, effect, cache key, provenance, expiry, and revocation path.

This extends the U19 trust store instead of adding a second permission system.

## Current Ratified Law

- `D-WD1`: one grant graph spans code effects, packages, builds, envs,
  services, images, fleets, and jetos activation.
- `D-JPK-DEVCOMPOSE1=D`: `jet env` opens tools only; `jet dev` runs project
  code after env realization and service readiness.
- `D-JPK-SECRET1=A`: secret reads carry the `Secret` effect; decrypted values
  stay out of the hangar and lock.
- `D-JPK-NODAEMON1=A`: no resident daemon, no root except transient sudo for
  jetos activation.
- `D-JPK-OFFLINE1=A`: realize-class verbs do not touch the network when the lock
  is satisfied.
- `D-BUILDPOLICY1`: build Tier 1 is pure and locked; Tier 2 needs an explicit
  impure grant with provenance.
- `D-WD4`: lock records policy and provenance facts that the grant graph must
  explain.

## Vertical Slices

### T1. Authority Fact Inventory

Collect every existing authority source into one internal fact model:

- code effects from sema;
- package provider fetches and registry reads;
- build actions, probes, tools, network, env, and filesystem caps;
- env services, secrets, hooks, source imports, and process launches;
- image and fleet realization inputs;
- cache substitution, signature verification, and lock writes.

Exit: one read-only command path can dump these facts as JSON for a fixture
repo, with no new user-facing syntax.

### T2. Grant Fingerprints

Compute stable grant fingerprints from authority facts and their defining source
hashes. A changed env module, build recipe, service command, source ref, or
secret binding invalidates the matching grant. Pure no-op edits do not.

Exit: tests prove a service command edit re-prompts, a formatting-only edit does
not, and an offline satisfied lock does not request network authority.

### T3. Unified Prompt Summary

Replace separate trust prompts with one summary grouped by intent:

- packages to fetch;
- services to run;
- secrets to expose;
- build effects to allow;
- cache/signature trust roots;
- activation class for image/fleet/jetos work.

The prompt stores the same grant graph the expert view reads. Non-interactive
and JSON paths fail with a structured diagnostic instead of hanging.

Exit: `jet env`, `jet dev`, build-from-source, image, and fleet fixture paths
all use the same grant engine.

### T4. Revocation And Audit

Add internal revocation over repo grants, pattern grants, and one-shot grants.
Audit reads grants without running project code or fetches. Revoked grants force
the next risky action through the prompt again.

Exit: tests cover grant add, list, remove, one-shot bypass, and revoked replay.

### T5. Dossier Integration

Feed the grant graph into the existing explain/dossier direction without making
`jet dossier` a dependency of this card. Each grant fact carries an owner
producer so a later dossier section can show stable JSON and human output.

Exit: every grant fact records `producer`, `source_span` where available,
`locked_identity`, and `revocation_key`.

## Acceptance Tests

- `trust_graph_collects_env_build_package_facts`: one fixture emits code,
  package, build, service, and secret facts.
- `trust_hash_changes_on_authority_edit`: risky source changes invalidate the
  grant.
- `trust_hash_ignores_formatting`: formatting-only edits keep the grant.
- `env_dev_build_share_prompt_engine`: env, dev, and source-build paths call the
  same grant evaluator.
- `trust_noninteractive_is_diagnostic`: CI/JSON path exits with the documented
  E12xx diagnostic.
- `revoked_grant_reprompts`: removing a grant makes the next action ask again.

## Dependencies

- Phase A dispatch, because engine verbs must cross one process contract.
- U19 env/dev split, because it defines first env trust semantics.
- Build-from-source T1, because build effects must enter the graph.
- Explainable lockfiles, because locked identity and rationale are the expert
  audit substrate.
- Signed cache and package signing, because signature trust roots are grant
  facts.

## Ballots Needed

- `D-JPK-GRANTCMD1` — Canonical user command surface for listing, revoking, and
  explaining grants. Existing U19 mentions `jet config trust`; `D-WD1` expands
  scope beyond env trust, so the exact command spelling needs owner approval
  before implementation.
- `D-JPK-GRANTSCHEMA1` — Canonical user-visible grant policy fields, if any are
  added to `pkg.jet` or role modules. Internal lock/trust-store schema needs no
  ballot; user-typeable policy fields do.

