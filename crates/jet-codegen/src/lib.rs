#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export lower seams. Sema transitively includes Parser/Lexer/Comptime/Foundation.
pub use jet_sema::{
    CanonicalAST, Collections, Comptime, Diagnostics, Formatter, Generics, Lexer, Parser, Sema,
    Syntax, Traits, AST, SHA256,
};
pub mod Codegen;
/// D-ASYNCRT1=A: M:N scheduler substrate for jet-jit host shims.
pub mod scheduler {
    fn jet_deadline_remaining_ms() -> Option<i64> {
        None
    }
    fn jet_deadline_exceeded(_kind: &str) -> ! {
        std::process::exit(70);
    }
    thread_local! {
        static JET_IN_SCHEDULER_TASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    pub fn jet_scheduler_task_panic_enter() {
        JET_IN_SCHEDULER_TASK.with(|c| c.set(true));
    }
    pub fn jet_scheduler_task_panic_leave() {
        JET_IN_SCHEDULER_TASK.with(|c| c.set(false));
    }
    include!("Prelude/Scheduler.rs");
}
// Prelude/ contains include_str-embedded text files, not Rust modules.
