# Compiler defect root causes (FixW1/FixW2 series, 2026-10-03)

Scope: root causes recorded for compiler defects fixed in the FixW series. Each entry names the Tower card it tracked. Root-cause text is from the source notes and was not re-verified against the code.

- Module-qualified nominal identity (cards 4381, 4376, 4402): qualified-identity defects in sema, MIR, emission and imports. Source: FixW2-identity/root-cause.txt. Fixes reported as landed; verify in code.
- Contextual optional return (card 4393): Absent lowered to an Option<Int> placeholder. Source: FixW2-4393/root-cause-and-proof.txt. Check tir.md coverage.
- Lambda capture consumption (card 4384): capture policy ignored checked consuming use; copyable owned String classified as clone. Memory-safety defect. Source: FixW2-4384/root-cause-and-proof.txt. Verify the fix landed.
- Generic binding recursion (card 4390): nested generic bindings installed as recursive type equations overflowed the stack. Fix replaces name equations with capability booleans. Source: FixW1-4390/handoff.txt.
- String view provenance (card 4392): View<str> returns lacked owner provenance; rustc E0106 from elided return lifetimes. Proof was owed. Source: FixW1-4392/proof-owed.txt.
- Generic module bounds (card 4401): specialization left bounds in template spelling; provider.concrete.Native split. Source: FixW2-4401/evidence.json (hand-written evidence).
