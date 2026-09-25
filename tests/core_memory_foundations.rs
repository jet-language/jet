//! Source-owned memory/ownership contract coverage across execution tiers.
//!
//! The allocator and scope surfaces are compiler-owned typed boundaries; this
//! test keeps their observable lifetime and cleanup behavior in one hostile
//! scenario rather than duplicating backend-specific expectations.

mod common;
mod tir_support;

#[test]
fn allocator_reset_close_and_scope_cleanup_keep_one_lifetime_contract() {
    tir_support::assert_tiers_agree(
        "core_memory_foundations",
        r#"
use core.mem
use core.mem.scope as scope

fn run() {
    arena :: mem.Arena.new(capacity: 32)
    first :: arena.alloc(7)
    print(first)

    // Reset is reusable, but the first view is no longer live past this edge.
    arena.reset()
    second :: arena.alloc(11)
    print(second)

    // Fallible allocation keeps the typed provider report identical on every tier.
    fixed :: mem.Fixed.new(size: 40)
    fixed.try_alloc(7) ? value -> {
        print("success {value}")
    }
    ! error -> {
        print("unexpected failure {error.requested_bytes} {error.allocator}")
    }
    fixed.try_alloc(9) ? value -> {
        print("unexpected success {value}")
    }
    ! error -> {
        print("failure {error.requested_bytes} {error.allocator}")
    }
    close(^fixed)

    // Scope guards run in reverse declaration order, before the function exits.
    _outer :: scope.guard(() -> print("outer cleanup"))
    _inner :: scope.guard(() -> print("inner cleanup"))
    print("live")

    // close is terminal ownership transfer; no view remains live here.
    close(^arena)
}
"#,
        "7\n11\nsuccess 7\nfailure 8 Fixed\nlive\ninner cleanup\nouter cleanup\n",
    );
}
