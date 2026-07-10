#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export lower seams. Sema transitively includes Parser/Lexer/Comptime/Foundation.
pub use jet_sema::{
    CanonicalAST, Collections, Comptime, Diagnostics, Formatter, Generics, Lexer, Parser, Sema,
    Syntax, TargetProfile, Traits, AST, SHA256,
};
pub mod Codegen;
/// D-ASYNCRT1=A: M:N scheduler substrate for jet-jit host shims.
pub mod scheduler {
    #[cfg(test)]
    thread_local! {
        static TEST_DEADLINE_EXCEEDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    #[cfg(test)]
    fn jet_deadline_remaining_ms() -> Option<i64> {
        TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.get().then_some(0))
    }
    #[cfg(not(test))]
    fn jet_deadline_remaining_ms() -> Option<i64> {
        None
    }
    #[cfg(test)]
    fn jet_deadline_exceeded(_kind: &str) -> ! {
        panic!("deadline exceeded");
    }
    #[cfg(not(test))]
    fn jet_deadline_exceeded(_kind: &str) -> ! {
        std::process::exit(70)
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
    fn jet_scheduler_panic_should_unwind() -> bool {
        JET_IN_SCHEDULER_TASK.with(|c| c.get())
    }
    include!("Prelude/Scheduler.rs");
}
// Prelude/ contains include_str-embedded text files, not Rust modules.
