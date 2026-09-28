# Security closure

Use this procedure to close a security-scan request without making the scan a
second work ledger. Tower owns remediation plans, criteria, and closure state.
A retained scan bundle is dated evidence. The governing project rules are in
[`AGENTS.md`](../../../AGENTS.md), and the owning Tower card is the source for
the request's exact scope and acceptance evidence.

## Prepare the request

Start from a clean commit after the relevant remediation has its independent
proof. Record the bound revision, inventory, exclusions, and required outputs
on the owning Tower card. Do not truncate, widen, or silently skip part of the
requested scope to make a scan pass.

Run one fresh Standard prompt-only Codex Security scan against that request,
then complete that same scan. Treat earlier reports as retained evidence, not
as findings to recycle into the new request. If the host cannot honor the
request, stop the scan slice and record the missing prerequisite in Tower.

## Keep the evidence together

Store the request, inventory, canonical outputs, report, and receipt together
under the approved audit location. Record the acceptance evidence on the
owning Tower card. A scan produces evidence; it does not automatically close a
remediation card.

Never hand-edit a canonical scan document or finalize a sealed bundle twice. A
missing report, non-zero finding, changed identity, or incomplete coverage is
not closure. A historical reconciliation can explain prior work, but it does
not prove a fresh external scan.
