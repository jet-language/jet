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
fn read(value: &Int) -> Int {
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

const CARD_4392_SOURCE: &str = r###"
fn window(asset: [U8], start: Int, width: Int) -> View<U8> from asset {
    asset[start..<start + width]
}
fn longer(left: [U8], right: [U8], width: Int) -> View<U8> from left | right {
    if left.len() >= right.len() {
        return left[0..<width]
    }
    right[0..<width]
}
fn run() {
    asset :: [U8{1}, U8{2}, U8{3}, U8{4}]
    short :: [U8{9}]
    middle :: window(asset, 1, 2)
    print(middle.len())
    picked :: longer(short, asset, 1)
    print(picked[0])
}
"###;

/// Card #4392: a returned view names the lifetime of the owners sema proved
/// (one owner beside borrowed `Int` inputs, or a union of owners), so rustc
/// never has to guess it and never rejects the signature (I2).
#[test]
fn card_4392_returned_view_names_its_owner_lifetime() {
    let rust = compile("tir_card_4392_view_return", CARD_4392_SOURCE);
    assert!(
        rust.contains("__jet_asset: &'__jet_view Vec<u8>"),
        "the single view owner beside Int inputs lost its lifetime:\n{rust}"
    );
    assert!(
        rust.contains("__jet_left: &'__jet_view Vec<u8>, __jet_right: &'__jet_view Vec<u8>"),
        "a union of view owners lost its shared lifetime:\n{rust}"
    );
    assert!(
        rust.contains("-> &'__jet_view [u8] {"),
        "the returned view does not name its owners' lifetime:\n{rust}"
    );
    assert_tiers_agree("tir_card_4392_view_return_runtime", CARD_4392_SOURCE, "2\n1\n");
}

const CARD_4392_CARRIER_SOURCE: &str = r###"
struct Record {
    kind: View<str>
    body: View<str>
}
fn split(header: String, payload: String) -> Record {
    kind :: header.after(":")
    body :: payload.after(":")
    return Record{kind: kind, body: body}
}
fn run() {
    header :: "kind:message"
    payload :: "body:hello"
    record :: split(header, payload)
    kind :: record.kind
    body :: record.body
    print(~kind)
    print(~body)
}
"###;

/// Card #4392: a struct holding views declares the view lifetime, a function
/// returning it names its proven owners' lifetime on the struct, and a bound
/// field read stays the view rather than an owned copy.
#[test]
fn card_4392_view_carrying_struct_names_its_owner_lifetime() {
    let rust = compile("tir_card_4392_view_carrier", CARD_4392_CARRIER_SOURCE);
    assert!(
        rust.lines().any(|line| line.contains("struct ")
            && line.ends_with("Record<'__jet_view> {"))
            && rust.contains("__jet_kind: &'__jet_view str,"),
        "the view-carrying struct lost its lifetime parameter:\n{rust}"
    );
    assert!(
        rust.lines().any(|line| line.starts_with("fn ")
            && line.contains(
                "(__jet_header: &'__jet_view String, __jet_payload: &'__jet_view String) -> "
            )
            && line.ends_with("Record<'__jet_view> {")),
        "the struct-returning function does not name its owners' lifetime:\n{rust}"
    );
    assert!(
        !rust.lines().any(|line| line.contains("jet_string_view_copy(")
            && (line.contains(".__jet_kind") || line.contains(".__jet_body"))),
        "a bound view field read materialized an owned copy:\n{rust}"
    );
    assert_tiers_agree(
        "tir_card_4392_view_carrier_runtime",
        CARD_4392_CARRIER_SOURCE,
        "message\nhello\n",
    );
}

/// Card #4392: a consumed (`^`) parameter is freed when the function returns,
/// so sema refuses a view into it before any backend sees the signature.
#[test]
fn card_4392_view_into_consumed_parameter_is_rejected_in_sema() {
    let src = r###"
fn owned_head(xs: ^[U8]) -> View<U8> {
    xs[0..<1]
}
fn run() {
    data :: [U8{1}, U8{2}]
    print(owned_head(^data).len())
}
"###;
    let diags = tir_support::compile_source("tir_card_4392_consumed_owner", src)
        .err()
        .expect("a view into a consumed parameter must be rejected");
    assert!(
        diags
            .iter()
            .any(|diag| diag.code == "E2305" && diag.what.contains("which this function consumes")),
        "{diags:?}"
    );
}
