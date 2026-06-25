//! Typestate tests (D-STATE1 / D-STATE-DECL / D-STATE-REQ / D-STATE-TRANS):
//! `#State(S)` require-state guards and `#Transition(From -> To)` transitions,
//! declared in a `state TypeName { … }` block, with the wrong-state error E0150
//! and the unknown-state error E0151. State is a compile-time fact threaded by the
//! checker and erased in codegen (I3).

fn codes(src: &str) -> Vec<&'static str> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.iter().map(|d| d.code).collect(),
    }
}

fn lint_codes(src: &str) -> Vec<&'static str> {
    match jet::compile(src) {
        Ok(out) => out.lints.iter().map(|d| d.code).collect(),
        Err(_) => Vec::new(),
    }
}

/// D-STATE-DECL: declaration block form — states are a bounded named set.
const DECL: &str = r#"
state Reservation { Pending, Confirmed, CheckedIn }

struct Reservation {
    guest: String
}

impl Reservation {
    #Transition(_ -> Pending) fn book(guest: String) -> Reservation {
        return Reservation { guest: guest }
    }
    #Transition(Pending -> Confirmed) fn pay(self: ^Reservation) -> Reservation {
        return self
    }
    #Transition(Confirmed -> CheckedIn) fn check_in(self: ^Reservation) -> Reservation {
        return self
    }
    #State(CheckedIn) fn room_key(self) -> String {
        return "key"
    }
}
"#;

/// The correct lifecycle (book -> pay -> check_in -> room_key) compiles clean.
#[test]
fn correct_lifecycle_ok() {
    let src = format!(
        "{DECL}\nfn main() {{\n  r := Reservation.book(\"a\")\n  r = r.pay()\n  r = r.check_in()\n  print(r.room_key())\n}}\n"
    );
    assert!(codes(&src).is_empty(), "correct order must compile: {:?}", codes(&src));
}

/// Calling a `Confirmed`-transition on a still-`Pending` value is E0150.
#[test]
fn transition_in_wrong_state_is_error() {
    let src = format!(
        "{DECL}\nfn main() {{\n  r := Reservation.book(\"a\")\n  r = r.check_in()\n}}\n"
    );
    assert!(codes(&src).contains(&"E0150"), "skipping pay() must be E0150: {:?}", codes(&src));
}

/// Calling a `#State(CheckedIn)` guarded read before checking in is E0150.
#[test]
fn guarded_read_in_wrong_state_is_error() {
    let src = format!(
        "{DECL}\nfn main() {{\n  r := Reservation.book(\"a\")\n  r = r.pay()\n  print(r.room_key())\n}}\n"
    );
    assert!(codes(&src).contains(&"E0150"), "room_key before check_in must be E0150: {:?}", codes(&src));
}

/// Doing every step in order, then the guarded read, is accepted.
#[test]
fn guarded_read_after_full_lifecycle_ok() {
    let src = format!(
        "{DECL}\nfn main() {{\n  r := Reservation.book(\"a\")\n  r = r.pay()\n  r = r.check_in()\n  print(r.room_key())\n}}\n"
    );
    assert!(!codes(&src).contains(&"E0150"), "full lifecycle then read must be clean: {:?}", codes(&src));
}

/// A program with no typestate markers is entirely unaffected (no false E0150).
#[test]
fn no_typestate_is_inert() {
    let src = r#"
struct Box { n: Int }
impl Box {
    fn get(self) -> Int { return self.n }
}
fn main() {
    b @= Box { n: 1 }
    print(b.get())
}
"#;
    assert!(!codes(src).contains(&"E0150"), "no typestate → no E0150: {:?}", codes(src));
}

/// D-STATE-DECL: a `#State(X)` marker referencing an undeclared state is E0151.
#[test]
fn unknown_state_in_marker_is_e0151() {
    let src = r#"
state Crate { Full, Empty }

struct Crate { data: Int }

impl Crate {
    #Transition(_ -> Full) fn fill(data: Int) -> Crate {
        return Crate { data: data }
    }
    #State(Stuffed) fn get(self) -> Int {
        return self.data
    }
}

fn main() {
    b := Crate.fill(1)
    print(b.get())
}
"#;
    assert!(codes(src).contains(&"E0151"), "unknown state must be E0151: {:?}", codes(src));
}

/// D-STATE-DECL: a `#Transition(A -> B)` marker referencing an undeclared to-state is E0151.
#[test]
fn unknown_transition_to_state_is_e0151() {
    let src = r#"
state Crate { Full, Empty }

struct Crate { data: Int }

impl Crate {
    #Transition(_ -> Stuffed) fn fill(data: Int) -> Crate {
        return Crate { data: data }
    }
}

fn main() { }
"#;
    assert!(codes(src).contains(&"E0151"), "undeclared to-state in Transition must be E0151: {:?}", codes(src));
}

/// D-STATE-DECL: a state declared with no outgoing transition is a dead-end (L0151 lint).
/// `CheckedIn` is the terminal state of the Reservation machine — it has no outgoing
/// `#Transition(CheckedIn -> …)` so it is a dead-end. The machine compiles anyway.
#[test]
fn dead_end_state_is_l0151() {
    // DECL declares `state Reservation { Pending, Confirmed, CheckedIn }`.
    // `CheckedIn` has no outgoing transition → L0151.
    let src = format!(
        "{DECL}\nfn main() {{\n  r := Reservation.book(\"a\")\n  r = r.pay()\n  r = r.check_in()\n  print(r.room_key())\n}}\n"
    );
    assert!(lint_codes(&src).contains(&"L0151"), "dead-end CheckedIn must be L0151: {:?}", lint_codes(&src));
}

/// D-STATE-DECL: when every declared state has an outgoing transition, no L0151 fires.
#[test]
fn no_dead_end_no_l0151() {
    let src = r#"
state Gate { Open, Closed }

struct Gate { w: Int }

impl Gate {
    #Transition(_ -> Closed) fn new(w: Int) -> Gate { return Gate { w: w } }
    #Transition(Closed -> Open) fn open(self: ^Gate) -> Gate { return self }
    #Transition(Open -> Closed) fn close(self: ^Gate) -> Gate { return self }
}

fn main() {
    g := Gate.new(1)
    g = g.open()
    g = g.close()
}
"#;
    // Every state has an outgoing transition (Open→Closed, Closed→Open); no dead end.
    assert!(codes(src).is_empty(), "no errors expected: {:?}", codes(src));
    assert!(!lint_codes(src).contains(&"L0151"), "no dead-end → no L0151: {:?}", lint_codes(src));
}

