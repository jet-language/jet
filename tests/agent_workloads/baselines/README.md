# Frozen interpreter baseline

`receipt.tsv` records one run of every frozen task under Bash, Python, and Node on the declared machine. `outputs/` stores the raw stdout and stderr bytes from those runs. Each row carries the shared policy digest from `tests/agent_workloads.rs`; the receipt uses the corpus scoring string from `domain_contract.tsv`; it is evidence, not a second runner or scoring model.

Regenerate the receipt with the capture command in `docs/audits/agent-workload-corpus.md`. Set all three `JET_CORPUS_BASELINE_*` values. Do not run capture on a different machine and overwrite this record.
