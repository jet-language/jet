//! D-JITUNWIND1 (#1995 / #1997): the generated no-unwind boundary.
//!
//! # The rule
//!
//! No Rust panic may be raised while a Cranelift frame is on the stack, and no
//! `extern "C"` frame this crate exposes to generated code may be reached by an
//! unwind. Every host symbol therefore catches inside its own C frame and
//! converts onto the tier's existing status channel
//! (`Concurrency::deliver_caught_unwind`); generated code then leaves through
//! Cranelift control flow at its next `emit_trap_check`, exactly as it already
//! does for a trap.
//!
//! # Why a *generated* boundary and not a reviewed one
//!
//! Two mechanisms were on the table (#1997):
//!
//! 1. Register unwind information for JIT'd code — call
//!    `compiled_code.create_unwind_info(isa)` and register an FDE per compiled
//!    function. Rejected: it makes unwinding a *supported* path through
//!    generated machine code, which is a far larger semantic commitment than it
//!    looks. It is platform-specific frame registration, and it writes Rust
//!    panic propagation into the contract of Jet-generated code.
//! 2. Convert at the boundary — every host seam turns a panic into a status
//!    *before* returning into JIT'd code, so no unwind ever begins with JIT
//!    frames below it. **Chosen.** It matches how the tiers already talk:
//!    `#Shield` delivers a deferred cancel as a status rather than an unwind
//!    (`Prelude/Scheduler.rs::jet_scheduler_shield_leave_status`), and
//!    `jet_deopt_call` already carried exactly this conversion by hand.
//!
//! **The recorded cost of (2) is that the guarantee has to hold across every
//! one of the ~1.7k `host_fns!` entries, so it needs a mechanical check rather
//! than review discipline.** That cost is the centre of the design, not an
//! afterthought, and it is paid three ways:
//!
//! * A host seam is an ordinary Rust `fn`, never an `extern "C" fn`. This is
//!   not cosmetic and it is the part that makes the rest work: rustc gives an
//!   `extern "C"` **body** an abort-on-unwind shim, so a panic raised inside
//!   one dies as `thread caused non-unwinding panic` at that body's own edge —
//!   *before* any wrapper above it could catch. A boundary can only be added by
//!   replacing the C frame, never by wrapping one.
//! * `host_fns!` (`lib.rs`) registers [`guarded`]`($host_fn)`, not
//!   `$host_fn as *const u8`. [`guarded`] returns the address of a generated
//!   `extern "C"` shim with the seam's exact C signature whose body runs the
//!   seam inside [`guard_seam`]. So the one canonical per-symbol declaration
//!   card #1633 established is also the boundary's generator, and a new host
//!   symbol cannot be declared without one. A callback handed to a *foreign*
//!   library rather than to generated code takes the same route by hand —
//!   `Ffi.rs` gives the bridge `guarded(ffi_reporter)`, never an `extern "C"`
//!   body of its own.
//! * `tests/jit_no_unwind_boundary.rs` is the mechanical check, and it is a rule
//!   about frames rather than about names: **no `extern` fn may be defined in
//!   this crate at all**, whatever its ABI or its name, except the shim below.
//!   Every seam named by `host_fns!` must exist, no host address may escape
//!   except through [`guarded_addr`], and the macro itself must still emit the
//!   guard. A guarantee that depends on someone remembering is not a guarantee —
//!   and neither is one that depends on a name prefix: the check's first form
//!   banned `extern "C" fn jet_*`, and `Ffi.rs`'s `jit_ffi_reporter` sat outside
//!   that family until the rule stopped being a name (#1995).
//! * The rule is also unfilable. Every ratcheted section of
//!   `tests/jit_corpus_gate.txt` is shrink-only, so a stem recorded there is one
//!   the example-corpus gate has agreed never to fail on again — and an abort
//!   recorded there would be an abort under permanent protection. So
//!   `corpus_gate_refuse_abort` (`tests/dev_parts/support.rs`) reads the AOT
//!   oracle, the default `jet run` and the forced interpreter, and any abort
//!   marker in stderr fails the stem outright rather than becoming a row.
//!   `streams/generators` is why: it raised a second time from drop glue, died
//!   as `panic in a destructor during cleanup`, and a classifier that only read
//!   the exit code filed it as the benign `AOT exit 1`. The marker list lives
//!   once, in `tests/common`, so the two checks cannot drift.
//! * The one carve-out is a process signal handler (`CoreHost.rs`), which the
//!   kernel enters on a borrowed stack that may have a JIT frame under it.
//!   [`guard_seam`] is the wrong tool there — it reaches
//!   `jet_scheduler_install_panic_hook`, which takes the process panic-hook lock
//!   and allocates on first call, so catching inside a handler would trade an
//!   unreachable panic for a reachable deadlock. Such a handler must be
//!   panic-free instead, and the check pins the statements its body may contain.
//!
//! # The two aborts this replaces, which are not the same abort
//!
//! * `fatal runtime error: failed to initiate panic, error 5` is phase 1
//!   finding *nothing*: `_URC_END_OF_STACK` from libgcc's phase-1 walk running
//!   off the top of the stack without seeing a handler, because
//!   `cranelift-jit` 0.112.3 registers no unwind information for the code it
//!   emits. It is **not** stack exhaustion, so raising
//!   `jet_foundation::CompilerStack::COMPILER_STACK_SIZE` cannot move it. Real
//!   exhaustion prints a textually distinct pair instead: `has overflowed its
//!   stack` followed by `fatal runtime error: stack overflow`.
//! * `thread caused non-unwinding panic` is a *different* abort, and it is the
//!   one `extern "C"` produces by itself: rustc gives an `extern "C"` body an
//!   empty-filter landing pad, so a panic raised in one *is* found in phase 1
//!   and then dies in phase 2. Catching inside the C frame avoids both.
//!
//! Neither is ever a user-facing outcome. A program-side stop renders through
//! the shared report boundary and exits `ExitCodes::RUNTIME_PANIC` (70); a Jet
//! defect takes the branded ICE rail and exits `ExitCodes::ICE` (101) with no
//! Rust payload in the report (I2).

use jet_codegen::scheduler::jet_scheduler_catch_foreign_boundary;

/// A value a guarded seam can return after it converted an unwind.
///
/// Generated code never consumes this value for meaning: the conversion has
/// already put the real outcome on the runtime's status channel, and the next
/// `emit_trap_check` reads that channel and leaves the function. The value only
/// has to be a well-formed word of the right ABI type so the return itself is
/// not UB.
pub(crate) trait SeamAbi: Copy {
    const FAULTED: Self;
}

macro_rules! seam_abi {
    ($( $ty:ty = $zero:expr ),* $(,)?) => {
        $(
            impl SeamAbi for $ty {
                const FAULTED: Self = $zero;
            }
        )*
    };
}

seam_abi! {
    () = (),
    i8 = 0,
    i16 = 0,
    i32 = 0,
    i64 = 0,
    isize = 0,
    u8 = 0,
    u16 = 0,
    u32 = 0,
    u64 = 0,
    usize = 0,
    f32 = 0.0,
    f64 = 0.0,
    bool = false,
}

/// Run one host seam body with the no-unwind boundary in place.
///
/// Always returns normally. A caught control transfer is delivered to the
/// tier's existing status channel by `Concurrency::deliver_caught_unwind` — the
/// same channel `#Shield` and the deopt boundary already use — and never to a
/// second error channel of this module's own (I8).
pub(crate) fn guard_seam<R: SeamAbi>(call: impl FnOnce() -> R) -> R {
    match jet_scheduler_catch_foreign_boundary(call) {
        Ok(value) => value,
        Err(payload) => {
            crate::Concurrency::deliver_caught_unwind(payload);
            R::FAULTED
        }
    }
}

/// Recover a zero-sized `fn` item from its type alone.
///
/// A plain `fn` item type is zero-sized and names exactly one function, so the
/// type parameter carries the whole callee. That is what lets the shim below be
/// an ordinary generic function instead of 1.7k hand-written wrappers.
fn zero_sized_callee<F: Copy + 'static>() -> F {
    debug_assert_eq!(
        std::mem::size_of::<F>(),
        0,
        "a guarded host seam must be a zero-sized `fn` item",
    );
    // SAFETY: `F` is zero-sized. `HostSeam::guarded_ptr` asserts that before it
    // ever hands out the shim's address, and the shim's address is the only way
    // to reach this function, so the precondition holds at every call. A
    // zero-sized value occupies no bytes, so producing it reads nothing.
    unsafe { std::mem::MaybeUninit::<F>::uninit().assume_init() }
}

/// A host `fn` item that can be exposed to generated code behind the boundary.
///
/// `Marker` carries the return type and the parameter types so one blanket impl
/// per arity stays coherent; it is always inferred from the `fn` item and never
/// written at a call site.
pub(crate) trait HostSeam<Marker>: Copy + 'static {
    /// The address of a generated `extern "C"` shim whose C signature is
    /// `self`'s and whose body runs `self` inside [`guard_seam`]. That shim,
    /// never `self`, is what generated code calls: a boundary can only be added
    /// by *replacing* the C frame, because rustc aborts an unwind at an
    /// `extern "C"` body's own edge before any wrapper above it could catch.
    fn guarded_ptr(self) -> *const u8;
}

macro_rules! host_seam_arity {
    ($( $ty:ident $val:ident ),* $(,)?) => {
        impl<Ret, $($ty,)* F> HostSeam<(Ret, $($ty,)*)> for F
        where
            F: Fn($($ty),*) -> Ret + Copy + 'static,
            Ret: SeamAbi,
        {
            fn guarded_ptr(self) -> *const u8 {
                assert_eq!(
                    std::mem::size_of::<F>(),
                    0,
                    "a JIT host seam must be a plain `fn` item, \
                     not a closure carrying captures",
                );
                #[allow(improper_ctypes_definitions)]
                extern "C" fn shim<Ret, $($ty,)* F>($($val: $ty),*) -> Ret
                where
                    F: Fn($($ty),*) -> Ret + Copy + 'static,
                    Ret: SeamAbi,
                {
                    guard_seam(move || zero_sized_callee::<F>()($($val),*))
                }
                shim::<Ret, $($ty,)* F> as *const u8
            }
        }
    };
}

// Arities 0..=10 cover every host symbol this crate declares; `jet_deopt_call`
// (10 words) is the widest. Adding an 11th-word seam is a compile error here
// rather than a silently unguarded boundary.
host_seam_arity!();
host_seam_arity!(A a);
host_seam_arity!(A a, B b);
host_seam_arity!(A a, B b, C c);
host_seam_arity!(A a, B b, C c, D d);
host_seam_arity!(A a, B b, C c, D d, E e);
host_seam_arity!(A a, B b, C c, D d, E e, G g);
host_seam_arity!(A a, B b, C c, D d, E e, G g, H h);
host_seam_arity!(A a, B b, C c, D d, E e, G g, H h, I i);
host_seam_arity!(A a, B b, C c, D d, E e, G g, H h, I i, J j);
host_seam_arity!(A a, B b, C c, D d, E e, G g, H h, I i, J j, K k);

/// The pointer `host_fns!` registers with `JITBuilder::symbol`.
pub(crate) fn guarded<Marker, F: HostSeam<Marker>>(host_fn: F) -> *const u8 {
    host_fn.guarded_ptr()
}

/// The same boundary for a seam handed to generated code as a plain callable
/// address rather than as a named import (`core.compute`'s transform adapters).
/// A raw `host_fn as usize` there would be an unguarded boundary, which is what
/// `tests/jit_no_unwind_boundary.rs` scans for.
pub(crate) fn guarded_addr<Marker, F: HostSeam<Marker>>(host_fn: F) -> usize {
    host_fn.guarded_ptr() as usize
}
