---
name: grill-with-docs
description: A relentless interview to sharpen a plan or design, which also creates docs (ADR's and glossary) as we go.
disable-model-invocation: true
---

## Contract

- **Requested outcome:** A completed interview that turns the user's question into facts, resolved decisions, and an agreed domain record.
- **Supplied inputs:** The user's topic, current conversation, existing glossary or ADR material, and—when facts need checking—the bounded research question, source boundary, and caller-owned output boundary.
- **Allowed child result:** `/grilling` supplies the one-question interview cadence; `/batch-grill-me` supplies frontier rounds when the user requests that cadence; `/research` returns sourced facts for the supplied question and source boundary, without requiring a second artifact when this caller owns the result; `/domain-modeling` records only terms or ADRs that the user has agreed. No child starts implementation or an undeclared agenda/workflow.
- **Completion owner:** `grill-with-docs` owns the interview and the resulting record; the user confirms shared understanding.
- **Return point:** After each bounded handoff, return to the open interview decisions, then to the record.
- **Stopping condition:** Stop after the user confirms shared understanding and the agreed record is captured. Do not implement the decision or launch a follow-on audit.

Use the requested interview cadence: `/batch-grill-me` for frontier rounds when
the user asks for it; otherwise use `/grilling` one question at a time. When a
fact needs checking, use `/research` only for the supplied question, source
boundary, and caller-owned output boundary; return its cited findings to this
interview without opening a separate research agenda or required artifact. Use
`/domain-modeling` only to record terms or an ADR that the user has agreed.
Each handoff returns to this skill. If the host cannot perform child
invocation, read the referenced files as supporting instructions in this run
and keep the same return and stopping conditions.

