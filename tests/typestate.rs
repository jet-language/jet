//! Typestate tests (D-STATE1, option A): `#State(S)` require-state guards and
//! `#Transition(From -> To)` transitions, with the wrong-state error E0150. State
//! is a compile-time fact threaded by the checker and erased in codegen (I3).

fn codes(src: &str) -> Vec<&'static str> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.iter().map(|d| d.code).collect(),
    }
}

const DECL: &str = r#"
tag Pending {}
tag Confirmed {}
tag CheckedIn {}

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
