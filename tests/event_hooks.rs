#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc};

#[test]
fn decision_hook_outcomes_transform_and_short_circuit() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run(
        "decision_hook_outcomes",
        r#"
use core.event as event

fn show(outcome: HookOutcome<String, String>) {
    if outcome == {
        .Continue(value) -> print("continue {value}")
        .Cancel -> print("cancel")
        .Fail(error) -> print("fail {error}")
    }
}

fn run() {
    zero :: event.decision_hook<String, String>(HookPolicy.FirstCancelElseTransform)
    show(zero.run("original"))

    scope :: event.scope()
    transformed :: event.decision_hook<String, String>(HookPolicy.FirstCancelElseTransform)
    transformed.on_priority(scope, 10, (value: String) => {
        print("first {value}")
        HookDecision.Transform("{value}-one")
    })
    transformed.on(scope, (value: String) => {
        print("second {value}")
        HookDecision.Continue
    })
    show(transformed.run("start"))

    cancelled :: event.decision_hook<String, String>(HookPolicy.FirstCancelElseTransform)
    cancelled.on_priority(scope, 10, (value: String) => HookDecision.Cancel)
    cancelled.on(scope, (value: String) => {
        print("must not run {value}")
        HookDecision.Continue
    })
    show(cancelled.run("stop"))

    failed :: event.decision_hook<String, String>(HookPolicy.FirstCancelElseTransform)
    failed.on_priority(scope, 10, (value: String) => HookDecision.Fail("denied {value}"))
    failed.on(scope, (value: String) => {
        print("must not run {value}")
        HookDecision.Continue
    })
    show(failed.run("save"))
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "continue original\nfirst start\nsecond start-one\ncontinue start-one\ncancel\nfail denied save\n"
    );
}

#[test]
fn decision_hook_lifetime_order_once_mutation_and_reentrancy() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run(
        "decision_hook_lifetime",
        r#"
use core.event as event

fn run() {
    scope :: event.scope()
    hook :: event.decision_hook<Int, String>(HookPolicy.FirstCancelElseTransform)
    late :: hook.on(scope, (value: Int) => {
        print("late {value}")
        HookDecision.Continue
    })
    hook.on_priority(scope, 10, (value: Int) => {
        print("first {value}")
        late.unsubscribe()
        HookDecision.Transform(value + 1)
    })
    hook.once(scope, (value: Int) => {
        print("once {value}")
        if value == 2 { hook.run(10) }
        HookDecision.Continue
    })
    hook.on(scope, (value: Int) => {
        print("last {value}")
        HookDecision.Continue
    })
    hook.run(1)
    hook.run(2)
    print("active {scope.active_count()}")
    scope.cancel()
    hook.run(3)

    owned :: event.decision_hook<Int, String>(HookPolicy.FirstCancelElseTransform)
    if true {
        owner :: event.scope()
        owned.on(owner, (value: Int) => {
            print("leaked {value}")
            HookDecision.Continue
        })
    }
    owned.run(9)
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "first 1\nonce 2\nfirst 10\nlast 11\nlast 2\nfirst 2\nlast 3\nactive 2\n"
    );
}
