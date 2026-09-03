//! Place-window address provenance must follow the referent's storage.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{assert_tiers_agree, build_and_run, compile, interpreter_run};

const STACK_SOURCE: &str = r###"
use core.mem
fn run() {
    value :: 17
    cell :: value
    #Unsafe("cell is live on this stack frame") {
        addr :: mem.address_of(cell)
        pointer :: mem.Ptr<Int>.from_addr(addr)
        print(pointer.*)
        via_volatile :: mem.volatile_read(pointer)
        print(via_volatile)
    }
}
"###;

const BORROWED_SOURCE: &str = r###"
use core.mem
fn read(value: &Int) Int -> {
    cell :: value
    #Unsafe("the borrowed value is live in the caller") {
        addr :: mem.address_of(cell)
        pointer :: mem.Ptr<Int>.from_addr(addr)
        print(pointer.*)
        via_volatile :: mem.volatile_read(pointer)
        return via_volatile
    }
}
fn run() {
    value := 23
    print(read(&value))
}
"###;

const HEAP_SOURCE: &str = r###"
use core.mem
fn run() {
    values := [31, 47]
    cell :: values[0]
    #Unsafe("the list element remains heap-owned") {
        addr :: mem.address_of(cell)
        pointer :: mem.Ptr<Int>.from_addr(addr)
        print(pointer.*)
        via_volatile :: mem.volatile_read(pointer)
        print(via_volatile)
    }
}
"###;

const CARD_2822_SOURCE: &str = r###"
fn run() {
    rows := [[1, 2]]
    if true {
        view :: &rows[0]
        view[0] = 9
        print(view[0])
    }
    print(rows[0][0])
}
"###;

const CARD_2823_SOURCE: &str = r###"
struct Parcel { score: Int }
fn run() {
    parcels := [Parcel{score: 2}]
    if true {
        edit :: &parcels[0].score
        edit = 12
        print(edit)
    }
    print(parcels[0].score)
}
"###;

const CARD_2826_SOURCE: &str = r###"
fn run() {
    rows := [[2]]
    if true {
        edit :: &rows[0][0]
        edit = 12
        print(edit)
    }
    print(rows[0][0])
}
"###;

#[test]
fn stack_window_uses_expiring_sentry_address() {
    let rust = compile("tir_place_window_stack", STACK_SOURCE);
    assert!(
        rust.contains("let __jet_addr: i64 = jet_mem::jet_sentry_stack_address_of"),
        "stack place window lost stack sentry registration:\n{rust}"
    );
    assert_tiers_agree("tir_place_window_stack_runtime", STACK_SOURCE, "17\n17\n");
}

#[test]
fn borrowed_window_stays_caller_owned() {
    let rust = compile("tir_place_window_borrowed", BORROWED_SOURCE);
    assert!(
        rust.contains("let __jet_addr: i64 = jet_mem::jet_sentry_address_of"),
        "borrowed place window was promoted to stack sentry registration:\n{rust}"
    );
    assert!(!rust.contains("let __jet_addr: i64 = jet_mem::jet_sentry_stack_address_of"));
    let (interpreter_code, interpreter_out, interpreter_err) =
        interpreter_run("tir_place_window_borrowed_runtime", BORROWED_SOURCE);
    assert_eq!(interpreter_code, 0, "interpreter stderr: {interpreter_err}");
    assert_eq!(interpreter_out, "23\n23\n");
    let (aot_code, aot_out) =
        build_and_run("tir_place_window_borrowed_runtime_aot", BORROWED_SOURCE);
    assert_eq!(aot_code, 0);
    assert_eq!(aot_out, "23\n23\n");
}

#[test]
fn heap_element_window_stays_allocator_owned() {
    let rust = compile("tir_place_window_heap", HEAP_SOURCE);
    assert!(
        rust.contains("let __jet_addr: i64 = jet_mem::jet_sentry_address_of"),
        "heap element place window was promoted to stack sentry registration:\n{rust}"
    );
    assert!(!rust.contains("let __jet_addr: i64 = jet_mem::jet_sentry_stack_address_of"));
    assert_tiers_agree("tir_place_window_heap_runtime", HEAP_SOURCE, "31\n31\n");
}

/// Hardening finding lane-1/nested-write-window-jit-tier-gap (card #2822): tier-parity regression fixture.
#[test]
fn card_2822_nested_write_view() {
    assert_tiers_agree("tir_card_2822_nested_write_view", CARD_2822_SOURCE, "9\n9\n");
}

/// Hardening finding lane-1/indexed-field-write-window-detaches (card #2823): tier-parity regression fixture.
#[test]
fn card_2823_scalar_field_write_view() {
    assert_tiers_agree(
        "tir_card_2823_scalar_field_write_view",
        CARD_2823_SOURCE,
        "12\n12\n",
    );
}

/// Hardening finding lane-1/nested-scalar-window-shape-drift (card #2826): tier-parity regression fixture.
#[test]
fn card_2826_nested_scalar_write_view() {
    assert_tiers_agree(
        "tir_card_2826_nested_scalar_write_view",
        CARD_2826_SOURCE,
        "12\n12\n",
    );
}

/// Hardening finding lane-1/nested-place-copy-jet-run-alias (card #2821): tier-parity regression fixture.
#[test]
fn card_2821_nested_copy_binding_tier_parity() {
    let src = r###"
fn run() {
    rows := [[1, 2]]
    alias := rows[0]
    alias[0] = 9
    print(alias[0])
    print(rows[0][0])
}
"###;
    assert_tiers_agree("tir_card_2821_nested_copy_binding", src, "9\n1\n");
}
