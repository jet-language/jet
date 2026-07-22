//! D-DET1 (ratified 2026-06-22): `@Pure` ⇒ reproducible.
//!
//! Two ratified pieces beyond the base purity checks (which already ship as
//! E3401/E3403):
//!   1. Injected deterministic `Clock` / `Rng` capabilities — a `@Pure fn` may
//!      read time/randomness THROUGH a `Clock`/`Rng` parameter (seeded by the
//!      caller, hence reproducible), while ambient `time.now()`/`random.int()`
//!      stay E3403.
//!   2. `@Nondeterministic("reason") { … }` — expert escape suspending
//!      determinism rejections (E3401/E3403) for its body.

mod common;

// ── Piece 1: injected deterministic Clock / Rng ───────────────────────────────

/// A `@Pure fn` reading time through an injected `Clock` param compiles.
#[test]
fn pure_fn_injected_clock_ok() {
    let src = r#"
fn at(clock: Clock) --[]-> Int {
    return clock.now()
}
fn run() {
    c :: time.clock(500)
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

/// A `@Pure fn` drawing randomness through an injected `&Rng` param compiles.
#[test]
fn pure_fn_injected_rng_ok() {
    let src = r#"
use core.random as random;
fn draw(rng: &Rng) --[]-> Int {
    return rng.int(1, 6)
}
fn run() {
    r := random.rng(7)
    print("{draw(&r)}")
}
"#;
    let res = jet::compile(src);
    assert!(res.is_ok(), "injected Rng should compile: {:?}", res.err());
}

/// The deterministic capability constructors (`time.clock`, `random.rng`) are
/// themselves usable inside a `@Pure fn` — they carry no ambient effect.
#[test]
fn pure_fn_constructs_caps_ok() {
    let src = r#"
use core.time as time;
use core.random as random;
fn seeded() --[]-> Int {
    c :: time.clock(10)
    r := random.rng(1)
    return c.now() + r.int(0, 0)
}
fn run() { print("{seeded()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "cap constructors should be pure: {:?}",
        res.err()
    );
}

/// D-TTL-CLOCK2=A: production code can explicitly create a monotonic system
/// Clock, while the constructor itself remains an ambient Time effect.
#[test]
fn system_clock_is_monotonic_and_effectful() {
    let pure = r#"
fn bad() --[]-> Int {
    clock := Clock.system()
    return clock.now()
}
fn run() { print(bad()) }
"#;
    let diags = jet::compile(pure).expect_err("Clock.system must carry Time");
    assert!(
        diags.iter().any(|diag| diag.code == "E3403"),
        "expected E3403, got {:?}",
        diags.iter().map(|diag| diag.code.as_str()).collect::<Vec<_>>()
    );

    if !common::have_rustc() {
        return;
    }
    let runtime = r#"
use core.time as time

fn run() {
    clock := Clock.system()
    before := clock.now()
    time.sleep(2)
    after := clock.now()
    print(after >= before)

    manual := time.clock(10)
    copied := ~manual
    copied.tick(5)
    print("{manual.now()}|{copied.now()}")
}
"#;
    let (code, stdout, stderr) =
        common::build_and_run("jet_system_clock", "system_clock", runtime);
    assert_eq!(code, 0, "system clock failed: {stderr}");
    assert_eq!(stdout, "true\n10|15\n");
}

/// Ambient `time.now()` inside a `@Pure fn` is STILL E3403 — the injection is
/// not a backdoor around the determinism rule.
#[test]
fn pure_fn_ambient_time_still_e3403() {
    let src = r#"
use core.time as time;
fn bad() --[]-> Int {
    return time.now()
}
fn run() { print("{bad()}") }
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "ambient time.now() in @Pure fn must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3403"),
        "expected E3403, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

/// Ambient `random.int(…)` inside a `@Pure fn` is STILL E3403.
#[test]
fn pure_fn_ambient_random_still_e3403() {
    let src = r#"
use core.random as random;
fn bad() --[]-> Int {
    return random.int(1, 6)
}
fn run() { print("{bad()}") }
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "ambient random.int() in @Pure fn must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3403"),
        "expected E3403, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

// ── Piece 2: audited nondeterminism escape ────────────────────────────────────

/// `@Nondeterministic("reason") { … }` suspends E3403 inside `@Pure fn`.
#[test]
fn assume_deterministic_suppresses_e3403() {
    let src = r#"
use core.time as time;
fn risky() --[]-> Int {
    t := 0
    @Nondeterministic("ambient clock is explicit test input") {
        t = time.now()
    }
    return t
}
fn run() { print("{risky()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "@Nondeterministic should suppress E3403: {:?}",
        res.err()
    );
}

/// `@Nondeterministic("reason") { … }` suspends E3401 too.
#[test]
fn assume_deterministic_suppresses_e3401() {
    let src = r#"
fn risky() --[]-> Int {
    @Nondeterministic("ambient print is deliberate") {
        print("side effect")
    }
    return 42
}
fn run() { print("{risky()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "@Nondeterministic should suppress E3401: {:?}",
        res.err()
    );
}

/// The suppression is scoped: an ambient call OUTSIDE the block still fires E3403.
#[test]
fn assume_deterministic_is_scoped() {
    let src = r#"
use core.time as time;
fn risky() --[]-> Int {
    @Nondeterministic("ambient clock is deliberate") {
        a := time.now()
    }
    return time.now()
}
fn run() { print("{risky()}") }
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
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

/// Retired word is now an ordinary identifier.
#[test]
fn retired_determinism_word_is_ordinary_identifier() {
    let src = r#"
fn run() {
    old_escape_name :: 5
    print("{old_escape_name}")
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "ordinary identifier should compile: {:?}",
        res.err()
    );
}

// ── D-DET-CAPAPI (ratified 2026-06-25): widened Clock / Rng surface ────────────
//
// Additive to the minimal set: `rng.bool()` / `rng.pick(list)` / `rng.shuffle(&list)`,
// the absolute `clock.advance(to_ms)`, the `Duration`-based `clock.wait(d)`, and
// checked `Duration` construction and whole-unit reading. All stay
// pure-callable through the injected, seeded handles.

/// The widened `Rng` draws (`bool`/`pick`/`shuffle`) compile inside a `@Pure fn`.
#[test]
fn pure_fn_widened_rng_ok() {
    let src = r#"
use core.random as random;
fn draws(rng: &Rng) --[]-> Bool {
    flip := rng.bool()
    xs := [1, 2, 3]
    chosen := rng.pick(xs) ?? 0
    deck := [9, 8, 7]
    rng.shuffle(&deck)
    return flip || chosen == 0
}
fn run() {
    r := random.rng(7)
    print("{draws(&r)}")
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
fn choose(rng: &Rng) --[]-> String {
    cards := ["A", "K", "Q"]
    return rng.pick(cards) ?? "none"
}
fn run() {
    r := random.rng(1)
    print("{choose(&r)}")
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "rng.pick element typing should compile: {:?}",
        res.err()
    );
}

/// Every `Rng` draw advances the stream, so a non-`&` receiver is rejected (E0202).
#[test]
fn rng_bool_needs_mut_receiver() {
    let src = r#"
use core.random as random;
fn run() {
    r :: random.rng(3)
    b := r.bool()
    print("{b}")
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "rng.bool() on a non-`&` rng must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

/// `rng.shuffle(deck)` without `&` on the list arg is rejected (E0202).
#[test]
fn rng_shuffle_needs_mut_list_arg() {
    let src = r#"
use core.random as random;
fn run() {
    r := random.rng(3)
    deck := [1, 2, 3]
    r.shuffle(deck)
    print("{deck}")
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_err(),
        "rng.shuffle without `&` on the list must fail"
    );
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

/// The widened `Clock` surface (`advance` / `wait`) compiles inside a `@Pure fn`.
#[test]
fn pure_fn_widened_clock_ok() {
    let src = r#"
use core.time as time;
fn drive_clock(clock: &Clock) --[]-> Int {
    base := clock.advance(5000)
    span := Duration.seconds(1) ?? panic("duration")
    return base + clock.wait(span)
}
fn run() {
    c := time.clock(0)
    print("{drive_clock(&c)}")
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "widened Clock surface should compile: {:?}",
        res.err()
    );
}

/// `clock.advance`/`clock.wait` move the clock, so a non-`&` receiver fails (E0202).
#[test]
fn clock_advance_needs_mut_receiver() {
    let src = r#"
use core.time as time;
fn run() {
    c :: time.clock(0)
    n := c.advance(100)
    print("{n}")
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "clock.advance() on a non-`&` clock must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

/// Checked `Duration` construction and whole-unit reading are pure.
#[test]
fn pure_fn_duration_ok() {
    let src = r#"
use core.time as time;
fn span_ms() --[]-> Int {
    d := Duration.seconds(3) ?? panic("duration")
    return d.in(.Milliseconds) ?? panic("duration read")
}
fn run() { print("{span_ms()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "Duration value ops should be pure: {:?}",
        res.err()
    );
}
