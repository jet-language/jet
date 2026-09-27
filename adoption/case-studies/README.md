# Case-study evidence contract

A case study is a bounded evidence record, not a marketing summary. A record
in this directory must use
[`case-study.schema.json`](../schemas/case-study.schema.json) and retain
receipts for:

- migration effort and source scope;
- build and runtime measurements with host and toolchain facts;
- failed attempts, recovery, and rollback;
- clean-machine reproduction; and
- the exact Jet and source revisions used.

The UL14 capstone evidence sets the boundary for a publishable outcome. Claims
must name the measured scope and follow the retained receipt. A fixture,
single-machine measurement, or unverified statement cannot establish a general
performance, compatibility, support, or security claim.

The directory contains no case-study record by default. That absence avoids an
unsubstantiated outcome; it is not evidence that any migration, support line,
or security property exists. Add a schema-valid record only with the complete
receipts and run the adoption validator against the pack.
