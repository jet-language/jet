# Native trait capability metadata

Scope: proposal to carry trait capability metadata for native arguments instead of inferring it from declarations.

Problem recorded in the source: the native capability check uses args.iter().all over declarations (MIRRust.rs:8681-8694).

Status in the source: unapplied.

Source: GoldenSquad2/native-capability-design.txt (2026-10-08).
