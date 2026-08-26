//! Typestate tests (D-STATE1 / D-STATE-HOME1 / D-STATE-REQ / D-STATE-TRANS):
//! `#State(S)` require-state guards and `#Transition(From, To)` transitions,
//! declared in a struct-owned `state { … }` section, with the wrong-state error E0150
//! and the unknown-state error E0151. State is a compile-time fact threaded by the
//! checker and erased in codegen (I3).

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::process::Command;

const TIER_SOURCE: &str = r#"
struct Settlement {
    state { Queued, Approved, Settled }
}

impl Settlement {
    #Transition(_, Queued) fn begin() Settlement -[]> { return Settlement{} }
    #Transition(Queued, Approved) fn approve(self: ^Settlement) Settlement -[]> { return self }
    #Transition(Approved, Settled) fn settle(self: ^Settlement) Settlement -[]> { return self }
    #State(Settled) fn report(self) String -[]> { return "settled" }
}

@state_info :: Settlement.reflect()

fn run() {
    item := Settlement.begin()
    item = item.approve()
    item = item.settle()
    print(item.report())
    print(@state_info.states[2].terminal)
    print(@state_info.states[2].reachable ?? false)
}
"#;

const TIER_EXPECTED: &str = "settled\ntrue\ntrue\n";

fn codes(src: &str) -> Vec<String> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.iter().map(|d| d.code.clone()).collect(),
    }
}

fn diagnostics(src: &str) -> Vec<jet::Diagnostics::Diagnostic> {
    jet::compile(src).expect_err("fixture must produce diagnostics")
}

fn lint_codes(src: &str) -> Vec<String> {
    match jet::compile(src) {
        Ok(out) => out.lints.iter().map(|d| d.code.clone()).collect(),
        Err(_) => Vec::new(),
    }
}

/// D-STATE-HOME1=A: the owning struct carries one bounded named state set.
const DECL: &str = r#"
struct Reservation {
    state { Pending, Confirmed, CheckedIn }
    guest: String
}

impl Reservation {
    #Transition(_, Pending) fn book(guest: String) Reservation -[]> {
        return Reservation{ guest: ~guest }
    }
    #Transition(Pending, Confirmed) fn pay(self: ^Reservation) Reservation -[]> {
        return self
    }
    #Transition(Confirmed, CheckedIn) fn check_in(self: ^Reservation) Reservation -[]> {
        return self
    }
    #State(CheckedIn) fn room_key(self) String -[]> {
        return "key"
    }
}
"#;

/// The correct lifecycle (book -> pay -> check_in -> room_key) compiles clean.
#[test]
fn correct_lifecycle_ok() {
    let src = format!(
        "{DECL}\nfn run() {{\n  r := Reservation.book(\"a\")\n  r = r.pay()\n  r = r.check_in()\n  print(r.room_key())\n}}\n"
    );
    assert!(
        codes(&src).is_empty(),
        "correct order must compile: {:?}",
        codes(&src)
    );
}

/// Calling a `Confirmed`-transition on a still-`Pending` value is E0150.
#[test]
fn transition_in_wrong_state_is_error() {
    let src =
        format!("{DECL}\nfn run() {{\n  r := Reservation.book(\"a\")\n  r = r.check_in()\n}}\n");
    assert!(
        codes(&src).iter().any(|c| c == "E0150"),
        "skipping pay() must be E0150: {:?}",
        codes(&src)
    );
}

/// Calling a `#State(CheckedIn)` guarded read before checking in is E0150.
#[test]
fn guarded_read_in_wrong_state_is_error() {
    let src = format!(
        "{DECL}\nfn run() {{\n  r := Reservation.book(\"a\")\n  r = r.pay()\n  print(r.room_key())\n}}\n"
    );
    assert!(
        codes(&src).iter().any(|c| c == "E0150"),
        "room_key before check_in must be E0150: {:?}",
        codes(&src)
    );
}

/// Doing every step in order, then the guarded read, is accepted.
#[test]
fn guarded_read_after_full_lifecycle_ok() {
    let src = format!(
        "{DECL}\nfn run() {{\n  r := Reservation.book(\"a\")\n  r = r.pay()\n  r = r.check_in()\n  print(r.room_key())\n}}\n"
    );
    assert!(
        !codes(&src).iter().any(|c| c == "E0150"),
        "full lifecycle then read must be clean: {:?}",
        codes(&src)
    );
}

/// A program with no typestate markers is entirely unaffected (no false E0150).
#[test]
fn no_typestate_is_inert() {
    let src = r#"
struct Box { n: Int }
impl Box {
    fn get(self) Int -[]> { return self.n }
}
fn run() {
    b :: Box{ n: 1 }
    print(b.get())
}
"#;
    assert!(
        !codes(src).iter().any(|c| c == "E0150"),
        "no typestate → no E0150: {:?}",
        codes(src)
    );
}

/// D-STATE-HOME1=A: a `#State(X)` marker referencing an undeclared state is E0151.
#[test]
fn unknown_state_in_marker_is_e0151() {
    let src = r#"
struct Crate {
    state { Full, Empty }
    data: Int
}

impl Crate {
    #Transition(_, Full) fn fill(data: Int) Crate -[]> {
        return Crate{ data: data }
    }
    #State(Stuffed) fn get(self) Int -[]> {
        return self.data
    }
}

fn run() {
    b := Crate.fill(1)
    print(b.get())
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0151"),
        "unknown state must be E0151: {:?}",
        codes(src)
    );
}

/// D-STATE-HOME1=A: a `#Transition(A, B)` marker referencing an undeclared to-state is E0151.
#[test]
fn unknown_transition_to_state_is_e0151() {
    let src = r#"
struct Crate {
    state { Full, Empty }
    data: Int
}

impl Crate {
    #Transition(_, Stuffed) fn fill(data: Int) Crate -[]> {
        return Crate{ data: data }
    }
}

fn run() { }
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0151"),
        "undeclared to-state in Transition must be E0151: {:?}",
        codes(src)
    );
}

#[test]
fn unknown_transition_from_state_is_e0151() {
    let src = r#"
struct Crate { state { Full, Empty } }
impl Crate {
    #Transition(Gone, Full) fn fill() Crate -[]> { return Crate{} }
}
fn run() {}
"#;
    let diagnostics = codes(src);
    assert!(
        diagnostics.iter().any(|code| code == "E0151"),
        "undeclared from-state in Transition must be E0151: {diagnostics:?}"
    );
}

#[test]
fn retired_top_level_state_companion_teaches_nested_owner() {
    let diagnostics = codes("state Door { Ready }\nstruct Door { opened: Bool }\nfn run() {}\n");
    assert!(
        diagnostics.iter().any(|code| code == "E0157"),
        "{diagnostics:?}"
    );
}

#[test]
fn state_section_shape_diagnostics_are_distinct() {
    let empty = codes("struct Empty { state {} }\nfn run() {}\n");
    assert!(empty.iter().any(|code| code == "E0169"), "{empty:?}");

    let duplicate_section = codes("struct Door { state { Ready } state { Open } }\nfn run() {}\n");
    assert!(
        duplicate_section.iter().any(|code| code == "E0168"),
        "{duplicate_section:?}"
    );

    let duplicate_name = codes("struct Door { state { Ready, Ready } }\nfn run() {}\n");
    assert!(
        duplicate_name.iter().any(|code| code == "E0166"),
        "{duplicate_name:?}"
    );

    let member_collision = codes("struct Door { state { open } open: Bool }\nfn run() {}\n");
    assert!(
        member_collision.iter().any(|code| code == "E0167"),
        "{member_collision:?}"
    );

    let missing_owner = codes(
        "struct Door { opened: Bool }\nimpl Door { #State(Ready) fn open(self) {} }\nfn run() {}\n",
    );
    assert!(
        missing_owner.iter().any(|code| code == "E0159"),
        "{missing_owner:?}"
    );
}

#[test]
fn state_section_sema_diagnostic_spans_cover_the_section() {
    let empty = "struct Empty { state {} }\nfn run() {}\n";
    let empty_start = empty.find("state {").unwrap();
    let empty_end = empty_start + empty[empty_start..].find('}').unwrap() + 1;
    let empty_diagnostic = diagnostics(empty)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "E0169")
        .expect("E0169");
    assert_eq!(
        empty_diagnostic.span,
        Some(jet::Diagnostics::Span::new(empty_start, empty_end))
    );

    let repeated = "struct Door { state { Ready } state { Open } }\nfn run() {}\n";
    let repeated_start = repeated.rfind("state {").unwrap();
    let repeated_end = repeated_start + repeated[repeated_start..].find('}').unwrap() + 1;
    let repeated_diagnostic = diagnostics(repeated)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "E0168")
        .expect("E0168");
    assert_eq!(
        repeated_diagnostic.span,
        Some(jet::Diagnostics::Span::new(repeated_start, repeated_end))
    );
}

#[test]
fn qualified_state_markers_seed_leaf_states() {
    let src = r#"
struct Door {
    state { Closed, Open }
}

impl Door {
    #Transition(_, Door.State.Closed) fn new() Door -[]> { return Door{} }
    #Transition(Door.State.Closed, Door.State.Open) fn open(self: ^Door) Door -[]> {
        return self
    }
    #State(Door.State.Closed) fn inspect(self: ^Door) String -[]> {
        next := self.open()
        return "closed"
    }
}

fn run() {}
"#;
    let diagnostics = codes(src);
    assert!(diagnostics.is_empty(), "qualified markers must resolve to leaves: {diagnostics:?}");
}

#[test]
fn member_collision_in_separate_impl_is_e0167() {
    let diagnostics =
        codes("struct Door { state { open } }\nimpl Door { fn open(self) {} }\nfn run() {}\n");
    assert!(
        diagnostics.iter().any(|code| code == "E0167"),
        "{diagnostics:?}"
    );
}

#[test]
fn generic_struct_state_set_is_shared_by_separate_impl() {
    let source = r#"
struct Box<T> {
    state { Empty, Full }
    value: T
}

impl Box {
    #Transition(_, Empty) fn new(value: ^T) Box<T> -[]> {
        return Box<T>{ value: value }
    }
    #Transition(Empty, Full) fn fill(self: ^Box<T>) Box<T> -[]> {
        return self
    }
    #State(Full) fn read(self) T -[]> {
        return self.value
    }
}

fn run() {
    box := Box<Int>.new(1)
    box = box.fill()
    print(box.read())
}
"#;
    let compiled = jet::compile(source).unwrap_or_else(|diags| panic!("{diags:#?}"));
    assert!(!compiled.rust.contains("Empty"), "state labels must erase");
    assert!(!compiled.rust.contains("Full"), "state labels must erase");
    assert!(
        !compiled.rust.contains("discriminant"),
        "typestate must not add a tag"
    );
}

#[test]
fn typestate_graph_is_folded_before_aot_jit_and_interpreter() {
    assert!(tir_support::have_rustc(), "AOT proof needs rustc");
    tir_support::assert_tiers_agree("typestate_graph_tiers", TIER_SOURCE, TIER_EXPECTED);
    let output = jet::compile(TIER_SOURCE).expect("typestate tier fixture must compile");
    for label in ["Queued", "Approved", "Settled", "Settlement.State"] {
        assert!(
            !output.rust.contains(label),
            "typestate fact `{label}` reached generated runtime Rust"
        );
    }
}

#[test]
fn typestate_graph_runs_on_resident_jit_without_runtime_state() {
    if !jet_jit::cranelift_host_supported() {
        eprintln!("note: resident Cranelift host unavailable; skipping resident-JIT proof");
        return;
    }
    let scratch = common::Scratch::new("typestate-jit");
    let path = scratch.join("main.jet");
    fs::write(&path, TIER_SOURCE).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let mut bundle = jet::Loader::load_entry(&shown).expect("typestate fixture loads");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics.is_empty(),
        "typestate fixture diagnostics: {diagnostics:#?}"
    );
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "typestate fixture must be resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::reset_jit_trace_for_test();
    let outcome = jet::Interpreter::dev_iteration(&shown, false, false);
    let stdout = match outcome {
        jet::Interpreter::RunOutcome::Ran { stdout, .. } => stdout,
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("resident JIT rejected typestate fixture: {diags:?}")
        }
    };
    assert_eq!(stdout, TIER_EXPECTED);
    assert!(
        jet_jit::jit_executed_for_test(),
        "typestate fixture did not execute on JIT"
    );
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "typestate fixture fell back to interpreter"
    );
}

#[test]
fn typestate_graph_reaches_web_without_runtime_state() {
    let scratch = common::Scratch::new("typestate-web");
    let shown = scratch.join("main.jet");
    fs::write(&shown, TIER_SOURCE).unwrap();
    let shown = shown.to_string_lossy().into_owned();
    let output = jet::compile_web_with_path(TIER_SOURCE, &shown).unwrap_or_else(|diags| {
        panic!(
            "web typestate fixture rejected:\n{}",
            jet::render_diagnostics(&shown, TIER_SOURCE, &diags)
        )
    });
    let web = output
        .web
        .expect("web typestate fixture must produce artifacts");
    for label in ["Queued", "Approved", "Settled", "Settlement.State"] {
        assert!(
            !web.wasm_rust.contains(label),
            "typestate fact `{label}` reached web runtime Rust"
        );
    }
    fs::write(scratch.join("app.js"), &web.js_app).unwrap();
    fs::write(scratch.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(scratch.join("package.json"), r#"{"type":"module"}"#).unwrap();
    let node = Command::new("node")
        .current_dir(&scratch.path)
        .arg("app.js")
        .output()
        .expect("spawn node");
    assert!(
        node.status.success(),
        "web typestate fixture failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&node.stdout), TIER_EXPECTED);
}

/// D-STATE-TERMINAL1: a state with no outgoing transition is a valid terminal fact.
#[test]
fn terminal_state_has_no_unreachable_lint() {
    let src = format!(
        "{DECL}\nfn run() {{\n  r := Reservation.book(\"a\")\n  r = r.pay()\n  r = r.check_in()\n  print(r.room_key())\n}}\n"
    );
    assert!(
        codes(&src).is_empty(),
        "a terminal state must compile: {:?}",
        codes(&src)
    );
    assert!(
        !lint_codes(&src).iter().any(|c| c == "L0153"),
        "reachable terminal state must have no reachability lint: {:?}",
        lint_codes(&src)
    );
}

/// D-STATE-TERMINAL1: a self-loop and a reopen transition are outgoing edges.
#[test]
fn self_loop_and_reopen_states_are_nonterminal() {
    let src = r#"
struct Gate {
    state { Closed, Reopened }
    w: Int
}

impl Gate {
    #Transition(_, Closed) fn new(w: Int) Gate -[]> { return Gate{ w: w } }
    #Transition(Closed, Closed) fn hold(self: ^Gate) Gate -[]> { return self }
    #Transition(Closed, Reopened) fn reopen(self: ^Gate) Gate -[]> { return self }
}

fn run() {
    g := Gate.new(1)
    g = g.hold()
    g = g.reopen()
}
"#;
    // Closed has both a self-loop and a reopen edge. Reopened is terminal.
    assert!(
        codes(src).is_empty(),
        "no errors expected: {:?}",
        codes(src)
    );
    let lints = lint_codes(src);
    assert!(
        lints.iter().all(|code| code != "L0153"),
        "self-loop and reopen must not be unreachable: {:?}",
        lints
    );
}

#[test]
fn duplicate_transition_keeps_method_duplicate_diagnostic() {
    let src = r#"
struct Gate { state { Closed, Reopened } }
impl Gate {
    #Transition(_, Closed) fn new() Gate -[]> { return Gate{} }
    #Transition(Closed, Reopened) fn reopen(self: ^Gate) Gate -[]> { return self }
    #Transition(Closed, Reopened) fn reopen(self: ^Gate) Gate -[]> { return self }
}
fn run() {}
"#;
    let diagnostics = codes(src);
    assert!(
        diagnostics.iter().any(|code| code == "E0105"),
        "duplicate transitions must retain E0105: {diagnostics:?}"
    );
}

#[test]
fn entry_graph_reports_only_unreachable_declared_states() {
    let src = r#"
struct Flow {
    state { Start, Done, Cancelled, Orphan }
}
impl Flow {
    #Transition(_, Start) fn start() Flow -[]> { return Flow{} }
    #Transition(Start, Done) fn done(self: ^Flow) Flow -[]> { return self }
    #Transition(Start, Cancelled) fn cancel(self: ^Flow) Flow -[]> { return self }
}
    fn run() {}
"#;
    let lints = lint_codes(src);
    assert_eq!(
        lints.iter().filter(|code| *code == "L0153").count(),
        1,
        "only the unreachable declared state should warn: {lints:?}"
    );
}

#[test]
fn no_entry_graph_does_not_invent_reachability() {
    let src = r#"
struct Flow { state { Start, Done } }
impl Flow {
    #Transition(Start, Done) fn done(self: ^Flow) Flow -[]> { return self }
}
fn run() {}
    "#;
    let lints = lint_codes(src);
    assert!(!lints.iter().any(|code| code == "L0153"), "{lints:?}");
}

/// D-FACT-FLOW1 (card #1621): a branch that confirms an order on one path and
/// cancels it on the other leaves the state unproved. The one join rule reports
/// the divergence (L0152) instead of keeping whatever arm was walked last.
const DIVERGENT: &str = r#"
struct Order {
    state { Draft, Confirmed, Cancelled, Closed }
    id: Int
}

impl Order {
    #Transition(_, Draft) fn start(id: Int) Order -[]> { return Order{ id: id } }
    #Transition(Draft, Confirmed) fn confirm(self: ^Order) Order -[]> { return self }
    #Transition(Draft, Cancelled) fn cancel(self: ^Order) Order -[]> { return self }
    #Transition(Confirmed, Closed) fn close(self: ^Order) Order -[]> { return self }
    #Transition(Cancelled, Closed) fn archive(self: ^Order) Order -[]> { return self }
    #State(Confirmed) fn ship(self) Int -[]> { return self.id }
}
"#;

#[test]
fn divergent_branch_states_are_reported() {
    let src = format!(
        "{DIVERGENT}\nfn decide(paid: Bool) {{\n  order := Order.start(1)\n  if {{\n    paid -> order = order.confirm()\n    else -> order = order.cancel()\n  }}\n  print(order.id)\n}}\nfn run() {{ decide(true) }}\n"
    );
    assert!(
        lint_codes(&src).iter().any(|c| c == "L0152"),
        "a state-divergent branch must be reported: {:?}",
        lint_codes(&src)
    );
}

#[test]
fn divergent_branch_does_not_keep_the_last_walked_arm() {
    let src = format!(
        "{DIVERGENT}\nfn decide(paid: Bool) {{\n  order := Order.start(1)\n  if {{\n    paid -> order = order.confirm()\n    else -> order = order.cancel()\n  }}\n  print(order.ship())\n}}\nfn run() {{ decide(true) }}\n"
    );
    // Keeping the last arm would call `ship` on a `Cancelled` order and report
    // E0150. After the join the state is unproved, so the guard stays silent —
    // and the divergence itself is what gets reported.
    assert!(
        !codes(&src).iter().any(|c| c == "E0150"),
        "the last-walked arm must not decide the state: {:?}",
        codes(&src)
    );
    assert!(
        lint_codes(&src).iter().any(|c| c == "L0152"),
        "the divergence is reported instead: {:?}",
        lint_codes(&src)
    );
}

#[test]
fn agreeing_branches_keep_the_state() {
    let src = format!(
        "{DIVERGENT}\nfn decide(paid: Bool) {{\n  order := Order.start(1)\n  if {{\n    paid -> order = order.confirm()\n    else -> order = order.confirm()\n  }}\n  print(order.ship())\n}}\nfn run() {{ decide(true) }}\n"
    );
    assert!(
        codes(&src).is_empty(),
        "both paths reach `Confirmed`, so `ship` is in state: {:?}",
        codes(&src)
    );
    assert!(
        !lint_codes(&src).iter().any(|c| c == "L0152"),
        "agreeing paths are not a divergence: {:?}",
        lint_codes(&src)
    );
}

/// D-FACT-FLOW1 (card #1621): an else-less pattern table CheckerCore already
/// proved exhaustive has no fall-through path, so agreeing arms must not be
/// reported as diverging against the pre-table state.
#[test]
fn agreeing_switch_arms_over_an_exhaustive_enum_keep_the_state() {
    let src = format!(
        "{DIVERGENT}\nenum Mode {{ Fast Slow }}\nfn decide(m: Mode) {{\n  order := Order.start(1)\n  if m == {{\n    .Fast -> order = order.confirm()\n    .Slow -> order = order.confirm()\n  }}\n  print(order.ship())\n}}\nfn run() {{ decide(Mode.Fast) }}\n"
    );
    assert!(
        codes(&src).is_empty(),
        "both arms of an exhaustive dispatch reach `Confirmed`, so `ship` is in state: {:?}",
        codes(&src)
    );
    assert!(
        !lint_codes(&src).iter().any(|c| c == "L0152"),
        "an exhaustive pattern table has no fall-through path to diverge from: {:?}",
        lint_codes(&src)
    );
}

/// D-FACT-FLOW1 (card #1621): a counted loop's body may run zero times, so
/// the loop merge is the join of the pre-loop state with the post-body state,
/// not the post-body state alone — a mismatch here is a real divergence.
#[test]
fn counted_loop_zero_iterations_reports_the_pre_loop_divergence() {
    let src = format!(
        "{DIVERGENT}\nfn decide(n: Int) {{\n  order := Order.start(1)\n  loop i := 0, i < n {{\n    fresh := Order.start(2)\n    order = fresh.confirm()\n  }}\n  print(order.ship())\n}}\nfn run() {{ decide(1) }}\n"
    );
    assert!(
        lint_codes(&src).iter().any(|c| c == "L0152"),
        "the loop may run zero times, so `Draft` (skipped) and `Confirmed` \
         (ran) must be reported as diverging: {:?}",
        lint_codes(&src)
    );
}
