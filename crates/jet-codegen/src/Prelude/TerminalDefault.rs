// Card #1751: the 80x24 terminal default is one fact. `TerminalPolicy::default`
// (Prelude/CoreLib/JetStd/CommonTypes.rs) and `PtyConfig::default`
// (Prelude/CoreLib/ProcessPty.rs, dual-compiled for the resident JIT host)
// both read it here instead of hand-typing 80/24, so the AOT default and the
// Cranelift JIT's PTY spec cannot drift.

pub const JET_TERMINAL_DEFAULT_COLS: i64 = 80;
pub const JET_TERMINAL_DEFAULT_ROWS: i64 = 24;
