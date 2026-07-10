# Epoch 3 capability ledger

`capability-ledger.json` is the checked inventory of every Epoch 3 Tower card.
It prevents a card phase, public claim, test fixture, or implementation path
from changing without re-auditing the evidence attached to that claim.

## Evidence classes

- **Reserved:** the public shape is recognized but does not execute.
- **Facade:** the shape runs through a mock, transcript, placeholder, silent
  omission, or non-production backend.
- **Partial:** a named subset runs and unsupported behavior fails loudly with a
  Jet diagnostic.
- **Implemented:** complete documented behavior runs on one supported path.
- **Proven:** implementation passes every applicable live, hostile, tier,
  platform, scale, recovery, and dogfood lane.

Only `proven` may close a capability claim. Assurance cards may close from an
executable ratchet and its recorded command. Resolution cards may close a
decline, merge, deferral, or other recorded disposition without pretending the
underlying capability shipped.

## Checked inputs

Each row pins:

- Tower identity, phase, body, plan, blockers, and decision outcomes;
- source or executable example evidence for capability claims;
- focused tests or executable golden examples;
- documentation evidence;
- exact verification commands recorded by the card;
- the Tower log entry supporting proof or disposition.

Proof files use SHA-256. Deleting or changing one fails the gate. Structural
Tower drift fails independently, while routine new progress logs do not
invalidate older proof entries.

## Commands

From the repository root:

```sh
node scripts/agent/check-capability-ledger.mjs --check
node scripts/agent/check-capability-ledger.mjs --self-test
node scripts/agent/check-capability-ledger.mjs --generate
```

`--generate` is an audit aid, not an approval mechanism. Review every emitted
`reopen` row against source and original card acceptance, correct Tower through
its CLI or API, then regenerate. Never edit `.tower/tower.json` by hand.

The `truthfulness` integration test runs both the current-ledger check and a
destructive fixture test proving deleted or tampered evidence is rejected.
