//! Stable process exit-code table (E2-M3, extends the E2-M2 contract).
//!
//! This is the single source of truth for what `jet` returns to a shell or
//! CI runner. The numbers are part of the public contract and never change
//! meaning; scripts and CI gates depend on them.
//!
//! | Code | Name           | Meaning                                         |
//! |------|----------------|-------------------------------------------------|
//! | 0    | `OK`           | success                                         |
//! | 1    | `USER_ERROR`   | the user's program/manifest has a problem       |
//! | 2    | `USAGE`        | the command line was wrong (bad/missing args)   |
//! | 70   | `RUNTIME_PANIC`| a built program stopped at runtime (`panic`)    |
//! | 101  | `ICE`          | internal compiler error (I2: rustc rejected     |
//! |      |                | generated code, or the compiler itself crashed) |
//!
//! Documented in docs/spec/release-policy.md ("Exit-code table").

/// Everything succeeded.
pub const OK: i32 = 0;

/// The user's program or manifest has a problem we reported with a diagnostic.
pub const USER_ERROR: i32 = 1;

/// The command line itself was wrong: unknown command, missing argument, or a
/// flag that doesn't apply. Distinguished from `USER_ERROR` so scripts can tell
/// "I called jet wrong" apart from "my program has a bug".
pub const USAGE: i32 = 2;

/// A built program stopped at runtime via `panic`/`require`/an index fault.
/// Emitted by the generated runtime (`jet_panic`), surfaced by `jet run`.
pub const RUNTIME_PANIC: i32 = 70;

/// Internal compiler error (invariant I2): rustc rejected the Rust we
/// generated, or the compiler hit a state it should never reach. Never the
/// user's fault.
pub const ICE: i32 = 101;
