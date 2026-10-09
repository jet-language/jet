# RNG static arithmetic check (2026-10)

Scope: hand computation of the RNG output for seed 99, compared with a recorded test output.

- First mix: 5774104757893143958.
- Final xor: 583982616703494564.
- Bit 0 is 0, so the predicate evaluates to false.
- The recorded random_audit.out line 9 reads "true".

The discrepancy is unresolved in the source note. Source: GoldenSquad2/rng-static-arithmetic.txt.
