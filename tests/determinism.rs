//! D-DET1 (ratified 2026-06-22): `#Pure` ⇒ reproducible.
//!
//! Two ratified pieces beyond the base purity checks (which already ship as
//! E3401/E3403):
//!   1. Injected deterministic `Clock` / `Rng` capabilities — a `#Pure fn` may
//!      read time/randomness THROUGH a `Clock`/`Rng` parameter (seeded by the
//!      caller, hence reproducible), while ambient `time.now()`/`random.int()`
//!      stay E3403.
//!   2. `#Nondeterministic("reason") { … }` — expert escape suspending
//!      determinism rejections (E3401/E3403) for its body.

mod common;

// ── Piece 1: injected deterministic Clock / Rng ───────────────────────────────

/// A `#Pure fn` reading time through an injected `Clock` param compiles.
#[test]
fn pure_fn_injected_clock_ok() {
    let src = r#"
fn at(clock: Clock) =[]=> Int {
    return clock.now()
}
fn run() {
    c :: Clock.new(500)
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

/// A `#Pure fn` drawing randomness through an injected `&Rng` param compiles.
#[test]
fn pure_fn_injected_rng_ok() {
    let src = r#"
use core.random as random;
fn draw(rng: &Rng) =[]=> Int {
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

/// The deterministic capability constructors (`Clock.new`, `random.rng`) are
/// themselves usable inside a `#Pure fn` — they carry no ambient effect.
#[test]
fn pure_fn_constructs_caps_ok() {
    let src = r#"
use core.time as time;
use core.random as random;
fn seeded() =[]=> Int {
    c :: Clock.new(10)
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

#[test]
fn system_clock_is_monotonic_and_effectful() {
    let pure = r#"
fn bad() =[]=> Int {
    clock := Clock.system()
    return clock.now()
}
fn run() { print(bad()) }
"#;
    let diags = jet::compile(pure).expect_err("Clock.system must carry Time");
    assert!(diags.iter().any(|diag| diag.code == "E3403"));

    let launder = r#"
fn read(clock: Clock) =[]=> Int {
    return clock.now()
}
fn make_system_clock() => Clock {
    return Clock.system()
}
fn run() {
    direct := Clock.system()
    print(read(direct))
    hidden := make_system_clock()
    print(read(hidden))
}
"#;
    let diags = jet::compile(launder).expect_err("a system Clock must not enter pure code");
    assert!(
        diags.iter().filter(|diag| diag.code == "E3403").count() >= 2,
        "both direct and return-type laundering must fail: {diags:?}"
    );

    if !common::have_rustc() {
        return;
    }
    let runtime = r#"
use core.time as time

fn run() {
    clock := Clock.system()
    before := clock.now()
    fork := ~clock
    fork_before := fork.now()
    time.sleep(2)
    after := clock.now()
    print(after >= before)
    print(fork.now() >= fork_before)
    reported := clock.tick(-1000000)
    print(clock.now() >= after)
    print(reported == clock.now())
}
"#;
    let (code, stdout, stderr) =
        common::build_and_run("jet_system_clock", "system_clock", runtime);
    assert_eq!(code, 0, "system clock failed: {stderr}");
    assert_eq!(stdout, "true\ntrue\ntrue\ntrue\n");
}

#[test]
fn system_clock_has_total_tir_and_an_honest_resident_jit_boundary() {
    let dir = common::unique_tmp("jet_system_clock_jit_boundary");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    let src = r#"
fn run() {
    clock := Clock.system()
    print(clock.now())
}
"#;
    std::fs::write(&path, src).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert!(
        jet_jit::tir_lowers_bundle(&bundle),
        "{}",
        jet_jit::tir_lower_fail_reason(&bundle)
    );
    assert!(!jet_jit::resident_jit_safe_bundle(&bundle));
    assert!(
        jet_jit::resident_jit_safe_bundle_detail(&bundle).contains("entry not resident-safe")
    );
}

#[test]
fn pure_code_rejects_clock_provenance_laundered_through_a_struct() {
    let read = r#"
struct Holder {
    clock: Clock
}
fn read(holder: Holder) =[]=> Int {
    return holder.clock.now()
}
fn run() {
    holder := Holder.{ clock: Clock.system() }
    print(read(holder))
}
"#;
    let diagnostics =
        jet::compile(read).expect_err("aggregate clock observation must retain Time");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3403"),
        "{diagnostics:#?}"
    );

    let compare = r#"
struct Holder {
    clock: Clock
}
fn same(holder: Holder) =[]=> Bool {
    return holder.clock == holder.clock
}
fn run() {
    holder := Holder.{ clock: Clock.system() }
    print(same(holder))
}
"#;
    let diagnostics =
        jet::compile(compare).expect_err("aggregate clock comparison must retain Time");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3403"),
        "{diagnostics:#?}"
    );

    let copy = r#"
struct Holder {
    clock: Clock
}
fn copy_clock(holder: Holder) =[]=> Clock {
    return ~holder.clock
}
fn run() {
    holder := Holder.{ clock: Clock.system() }
    copied := copy_clock(holder)
    print(copied.now())
}
"#;
    let diagnostics =
        jet::compile(copy).expect_err("aggregate clock copy must retain Time");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3403"),
        "{diagnostics:#?}"
    );

    let copy_aggregate = r#"
struct Holder {
    clock: Clock
}
fn copy_holder(holder: Holder) =[]=> Holder {
    return ~holder
}
fn run() {
    holder := Holder.{ clock: Clock.system() }
    copied := copy_holder(holder)
    print(copied.clock.now())
}
"#;
    let diagnostics =
        jet::compile(copy_aggregate).expect_err("whole aggregate clock copy must retain Time");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3403"),
        "{diagnostics:#?}"
    );

    let enum_aggregate = r#"
enum BoxedClock {
    Held(Clock)
}
fn copy_box(value: BoxedClock) =[]=> BoxedClock {
    return ~value
}
fn show_box(value: BoxedClock) =[]=> String {
    return "{value:Debug}"
}
fn run() {}
"#;
    let diagnostics = jet::compile(enum_aggregate)
        .expect_err("enum-contained clock observation must retain Time");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3403"),
        "{diagnostics:#?}"
    );

    let format = r#"
struct Holder {
    clock: Clock
}
fn show(holder: Holder) =[]=> String {
    return "{holder.clock} {holder.clock:Debug}"
}
fn run() {
    holder := Holder.{ clock: Clock.system() }
    print(show(holder))
}
"#;
    let diagnostics =
        jet::compile(format).expect_err("aggregate clock formatting must retain Time");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3403"),
        "{diagnostics:#?}"
    );

    let format_aggregate = r#"
struct Holder {
    clock: Clock
}
fn show(holder: Holder) =[]=> String {
    return "{holder:Debug}"
}
fn run() {
    holder := Holder.{ clock: Clock.system() }
    print(show(holder))
}
"#;
    let diagnostics = jet::compile(format_aggregate)
        .expect_err("whole aggregate clock formatting must retain Time");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3403"),
        "{diagnostics:#?}"
    );
}

#[test]
fn pure_code_rejects_clock_observation_through_an_imported_nominal_type() {
    let dir = common::unique_tmp("jet_imported_clock_aggregate");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("clock_box.jet"),
        "pub enum BoxedClock {\n    Held(Clock)\n}\n",
    )
    .unwrap();
    let main = dir.join("main.jet");
    std::fs::write(
        &main,
        "use \"clock_box\"\nfn copy_box(value: clock_box.BoxedClock) =[]=> clock_box.BoxedClock {\n    return ~value\n}\nfn show_box(value: clock_box.BoxedClock) =[]=> String {\n    return \"{value:Debug}\"\n}\nfn run() {}\n",
    )
    .unwrap();

    let diagnostics = jet::check_with_path(main.to_str().unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3403"),
        "{diagnostics:#?}"
    );
}

/// Ambient `time.now()` inside a `#Pure fn` is STILL E3403 — the injection is
/// not a backdoor around the determinism rule.
#[test]
fn pure_fn_ambient_time_still_e3403() {
    let src = r#"
use core.time as time;
fn bad() =[]=> Int {
    return time.now()
}
fn run() { print("{bad()}") }
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "ambient time.now() in #Pure fn must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3403"),
        "expected E3403, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

/// Ambient `random.int(…)` inside a `#Pure fn` is STILL E3403.
#[test]
fn pure_fn_ambient_random_still_e3403() {
    let src = r#"
use core.random as random;
fn bad() =[]=> Int {
    return random.int(1, 6)
}
fn run() { print("{bad()}") }
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "ambient random.int() in #Pure fn must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3403"),
        "expected E3403, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

/// #1488: `date.today()` must have the same ambient classification in sema's
/// purity walk and the explicit `$` comptime-fold path. It used to be present
/// only in the TIR fold guard, so the two paths diverged.
#[test]
fn nondeterministic_time_classification_is_shared_by_purity_and_folding() {
    let pure_src = r#"
use core.time.date as date;

fn pure_today() =[]=> String {
    return date.today().to_string()
}

fn run() {
    print(pure_today())
}
"#;
    let purity_diagnostics =
        jet::compile(pure_src).expect_err("ambient date.today must be rejected in pure code");
    assert!(
        purity_diagnostics.iter().any(|diagnostic| diagnostic.code == "E3403"),
        "sema purity must report E3403, got: {purity_diagnostics:#?}"
    );

    let fold_src = r#"
use core.time.date as date;

fn run() {
    $today :: date.today()
}
"#;
    let fold_diagnostics =
        jet::compile(fold_src).expect_err("ambient date.today must be rejected at comptime");
    assert!(
        fold_diagnostics.iter().any(|diagnostic| diagnostic.code == "E3403"),
        "explicit comptime folding must report E3403, got: {fold_diagnostics:#?}"
    );
    assert!(
        fold_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E0956"),
        "ambient date.today must not fall back to generic comptime E0956: {fold_diagnostics:#?}"
    );
}

// ── Piece 2: audited nondeterminism escape ────────────────────────────────────

/// `#Nondeterministic("reason") { … }` suspends E3403 inside `#Pure fn`.
#[test]
fn assume_deterministic_suppresses_e3403() {
    let src = r#"
use core.time as time;
fn risky() =[]=> Int {
    t := 0
    #Nondeterministic("ambient clock is explicit test input") {
        t = time.now()
    }
    return t
}
fn run() { print("{risky()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "#Nondeterministic should suppress E3403: {:?}",
        res.err()
    );
}

/// `#Nondeterministic("reason") { … }` suspends E3401 too.
#[test]
fn assume_deterministic_suppresses_e3401() {
    let src = r#"
fn risky() =[]=> Int {
    #Nondeterministic("ambient print is deliberate") {
        print("side effect")
    }
    return 42
}
fn run() { print("{risky()}") }
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "#Nondeterministic should suppress E3401: {:?}",
        res.err()
    );
}

/// The suppression is scoped: an ambient call OUTSIDE the block still fires E3403.
#[test]
fn assume_deterministic_is_scoped() {
    let src = r#"
use core.time as time;
fn risky() =[]=> Int {
    #Nondeterministic("ambient clock is deliberate") {
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

/// The widened `Rng` draws (`bool`/`pick`/`shuffle`) compile inside a `#Pure fn`.
#[test]
fn pure_fn_widened_rng_ok() {
    let src = r#"
use core.random as random;
fn draws(rng: &Rng) =[]=> Bool {
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
fn choose(rng: &Rng) =[]=> String {
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

/// The widened `Clock` surface (`advance` / `wait`) compiles inside a `#Pure fn`.
#[test]
fn pure_fn_widened_clock_ok() {
    let src = r#"
use core.time as time;
fn drive_clock(clock: &Clock) =[]=> Int {
    base := clock.advance(5000)
    span := Duration.seconds(1) ?? panic("duration")
    return base + clock.wait(span)
}
fn run() {
    c := Clock.new(0)
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
    c :: Clock.new(0)
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
fn span_ms() =[]=> Int {
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
