# Case-study evidence contract

No case study is published from this directory until a UL14 capstone supplies
durable evidence. A finished case study must link a retained receipt for:

- migration effort and source scope;
- build and runtime measurements with host/toolchain facts;
- failed attempts, recovery, and rollback;
- clean-machine reproduction;
- the exact Jet and source revisions used.

Claims are bounded by the receipt. A case study cannot turn a fixture, a
single-machine measurement, or an unverified marketing statement into a
general performance, compatibility, support, or security claim.

The machine-readable shape is
[`case-study.schema.json`](../schemas/case-study.schema.json). The absence of
a case-study record is intentional while the capstone evidence is pending; it
is not a placeholder release claim.
