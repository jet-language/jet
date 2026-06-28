//! D-DET1 (ratified 2026-06-22): `#Pure` ⇒ reproducible.
//!
//! Two ratified pieces beyond the base purity checks (which already ship as
//! E3401/E3403):
//!   1. Injected deterministic `Clock` / `Rng` capabilities — a `#Pure fn` may
//!      read time/randomness THROUGH a `Clock`/`Rng` parameter (seeded by the
//!      caller, hence reproducible), while ambient `time.now()`/`random.int()`
//!      stay E3403.
//!   2. `assume_deterministic { … }` — the expert escape that suspends the
//!      determinism rejections (E3401/E3403) for its body.

// ── Piece 1: injected deterministic Clock / Rng ───────────────────────────────

/// A `#Pure fn` reading time through an injected `Clock` param compiles.
#[test]
fn pure_fn_injected_clock_ok() {
    let src = r#"
#Pure fn at(clock: Clock) -> Int {
    return clock.now()
}
fn main() {
    c @= time.clock(500)
    print("{at(c)}")
}
use core.time as time;
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "injected Clock should compile: {:?}",
        res.err()
    );
}

/// A `#Pure fn` drawing randomness through an injected `~Rng` param compiles.
#[test]
fn pure_fn_injected_rng_ok() {
    let src = r#"
use core.random as random;
#Pure fn draw(rng: ~Rng) -> Int {
    return rng.int(1, 6)
}
fn main() {
    r := random.rng(7)
    print("{draw(~r)}")
}
"#;
    let res = jet::compile(src);
    assert!(res.is_ok(), "injected Rng should compile: {:?}", res.err());
}

/// The deterministic capability constructors (`time.clock`, `random.rng`) are
/// themselves usable inside a `#Pure fn` — they carry no ambient effect.
#[test]
fn pure_fn_constructs_caps_ok() {
    let src = r#"
use core.time as time;
use core.random as random;
#Pure fn seeded() -> Int {
    c @= time.clock(10)
    r := random.rng(1)
    return c.now() + r.int(0, 0)
}
fn main() { print("{seeded()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "cap constructors should be pure: {:?}",
        res.err()
    );
}

/// Ambient `time.now()` inside a `#Pure fn` is STILL E3403 — the injection is
/// not a backdoor around the determinism rule.
#[test]
fn pure_fn_ambient_time_still_e3403() {
    let src = r#"
use core.time as time;
#Pure fn bad() -> Int {
    return time.now()
}
fn main() { print("{bad()}") }
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "ambient time.now() in #Pure fn must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3403"),
        "expected E3403, got: {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

/// Ambient `random.int(…)` inside a `#Pure fn` is STILL E3403.
#[test]
fn pure_fn_ambient_random_still_e3403() {
    let src = r#"
use core.random as random;
#Pure fn bad() -> Int {
    return random.int(1, 6)
}
fn main() { print("{bad()}") }
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "ambient random.int() in #Pure fn must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3403"),
        "expected E3403, got: {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

// ── Piece 2: assume_deterministic { } expert escape ───────────────────────────

/// `assume_deterministic { … }` suspends E3403 for ambient time inside a `#Pure fn`.
#[test]
fn assume_deterministic_suppresses_e3403() {
    let src = r#"
use core.time as time;
#Pure fn risky() -> Int {
    t := 0
    assume_deterministic {
        t = time.now()
    }
    return t
}
fn main() { print("{risky()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "assume_deterministic should suppress E3403: {:?}",
        res.err()
    );
}

/// `assume_deterministic { … }` suspends E3401 for an impure builtin call too.
#[test]
fn assume_deterministic_suppresses_e3401() {
    let src = r#"
#Pure fn risky() -> Int {
    assume_deterministic {
        print("side effect")
    }
    return 42
}
fn main() { print("{risky()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "assume_deterministic should suppress E3401: {:?}",
        res.err()
    );
}

/// The suppression is scoped: an ambient call OUTSIDE the block still fires E3403.
#[test]
fn assume_deterministic_is_scoped() {
    let src = r#"
use core.time as time;
#Pure fn risky() -> Int {
    assume_deterministic {
        a := time.now()
    }
    return time.now()
}
fn main() { print("{risky()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_err(),
        "ambient call outside the block must still fail"
    );
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3403"),
        "expected E3403 for the call outside the block, got: {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

/// `assume_deterministic` is a contextual keyword: a binding named
/// `assume_deterministic` (not followed by `{`) still works.
#[test]
fn assume_deterministic_contextual_keyword() {
    let src = r#"
fn main() {
    assume_deterministic @= 5
    print("{assume_deterministic}")
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "`assume_deterministic` should still be a valid name: {:?}",
        res.err()
    );
}

// ── D-DET-CAPAPI (ratified 2026-06-25): widened Clock / Rng surface ────────────
//
// Additive to the minimal set: `rng.bool()` / `rng.pick(list)` / `rng.shuffle(~list)`,
// the absolute `clock.advance(to_ms)`, the `Duration`-based `clock.wait(d)`, and
// the `Duration` value (`time.ms`/`time.secs`, `duration.millis()`). All stay
// pure-callable through the injected, seeded handles.

/// The widened `Rng` draws (`bool`/`pick`/`shuffle`) compile inside a `#Pure fn`.
#[test]
fn pure_fn_widened_rng_ok() {
    let src = r#"
use core.random as random;
#Pure fn draws(rng: ~Rng) -> Bool {
    flip := rng.bool()
    xs := [1, 2, 3]
    chosen := rng.pick(xs) ?? 0
    deck := [9, 8, 7]
    rng.shuffle(~deck)
    return flip || chosen == 0
}
fn main() {
    r := random.rng(7)
    print("{draws(~r)}")
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "widened Rng draws should compile: {:?}",
        res.err()
    );
}

/// `rng.pick(list)` returns the element's optional type (`[String]` → `String?`).
#[test]
fn rng_pick_returns_element_option() {
    let src = r#"
use core.random as random;
#Pure fn choose(rng: ~Rng) -> String {
    cards := ["A", "K", "Q"]
    return rng.pick(cards) ?? "none"
}
fn main() {
    r := random.rng(1)
    print("{choose(~r)}")
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "rng.pick element typing should compile: {:?}",
        res.err()
    );
}

/// Every `Rng` draw advances the stream, so a non-`~` receiver is rejected (E0202).
#[test]
fn rng_bool_needs_mut_receiver() {
    let src = r#"
use core.random as random;
fn main() {
    r @= random.rng(3)
    b := r.bool()
    print("{b}")
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "rng.bool() on a non-`~` rng must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

/// `rng.shuffle(deck)` without `~` on the list arg is rejected (E0202).
#[test]
fn rng_shuffle_needs_mut_list_arg() {
    let src = r#"
use core.random as random;
fn main() {
    r := random.rng(3)
    deck := [1, 2, 3]
    r.shuffle(deck)
    print("{deck}")
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_err(),
        "rng.shuffle without `~` on the list must fail"
    );
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

/// The widened `Clock` surface (`advance` / `wait`) compiles inside a `#Pure fn`.
#[test]
fn pure_fn_widened_clock_ok() {
    let src = r#"
use core.time as time;
#Pure fn run(clock: ~Clock) -> Int {
    base := clock.advance(5000)
    span := time.secs(1)
    return base + clock.wait(span)
}
fn main() {
    c := time.clock(0)
    print("{run(~c)}")
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "widened Clock surface should compile: {:?}",
        res.err()
    );
}

/// `clock.advance`/`clock.wait` move the clock, so a non-`~` receiver fails (E0202).
#[test]
fn clock_advance_needs_mut_receiver() {
    let src = r#"
use core.time as time;
fn main() {
    c @= time.clock(0)
    n := c.advance(100)
    print("{n}")
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "clock.advance() on a non-`~` clock must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

/// `Duration` constructors (`time.ms`/`time.secs`) and `duration.millis()` are
/// pure — usable inside a `#Pure fn` (they mint a value, carry no ambient effect).
#[test]
fn pure_fn_duration_ok() {
    let src = r#"
use core.time as time;
#Pure fn span_ms() -> Int {
    d := time.secs(3)
    return d.millis()
}
fn main() { print("{span_ms()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "Duration value ops should be pure: {:?}",
        res.err()
    );
}
