//! Native Shared-owner and closure-capture ownership regressions.

mod common;

use std::fs;

const SOURCE: &str = r#"
struct Holder { owner: Shared<Int> }

struct SharedPair { first: Shared<Int>, rest: Shared<Int> }
struct Bucket {
    values: [Int],
    by_name: [String:Int],
}

fn extract_owner(holder: ^Holder) -> Shared<Int> {
    return holder.owner
}

fn append_then_fail(values: &[Int], value: Int) -> Int? String! {
    &values.push(value)
    return Err("expected failure")
}

fn make_counter(owner: ^Shared<Int>) -> fn(Int) Int {
    bucket := Bucket{values: [Int]{}, by_name: [String:Int]{}}
    return (value: Int) -> {
        failure :: append_then_fail(&bucket.values, value) ?? -value
        bucket.by_name["last"] = value
        return bucket.values.len() * 100 + bucket.by_name["last"] + owner.strong_count() + failure
    }
}

fn take_once(owner: ^Shared<Int>) -> Shared<Int> {
    callback :: () -> {
        return owner
    }
    return callback()
}

fn replace_then_take(owner: ^Shared<Int>) -> Shared<Int> {
    current := ^owner
    callback :: () -> {
        current = shared 13
        return current
    }
    return callback()
}

fn take_captured_first(pair: ^SharedPair) -> Shared<Int> {
    callback :: () -> {
        return pair.first
    }
    return callback()
}

fn expired_weak() -> Shared.Weak<Int> {
    owner :: shared 5
    return owner.downgrade()
}

fn run() {
    owner := shared 7
    weak :: owner.downgrade()
    holder :: Holder{owner: ^owner}
    escaped :: extract_owner(^holder)
    print(escaped.strong_count())
    {
        copied :: ~escaped
        print(copied.strong_count())
        print(escaped.strong_count())
    }
    print(escaped.strong_count())
    {
        snapshot :: escaped.capture()
        snapshot_copy :: ~snapshot
        print(escaped.strong_count())
        print(snapshot_copy.value)
    }
    projection :: escaped.capture((value: Int) -> value + 1)
    print(projection.value)
    replace_snapshot :: escaped.capture()
    replaced :: escaped.try_replace(^replace_snapshot, 8) ?? false
    print(replaced)
    print(escaped.capture().value)

    live :: weak.upgrade() ?? panic("moved aggregate released its owner")
    {
        counter :: make_counter(^live)
        alias :: ~counter
        print(counter(1))
        print(alias(2))
        print(counter(3))
        print(escaped.strong_count())
    }
    print(escaped.strong_count())

    once_owner :: shared 9
    once_weak :: once_owner.downgrade()
    once_escaped :: take_once(^once_owner)
    print(once_escaped.strong_count())
    if once_weak.upgrade() == .None { panic("FnOnce transfer released its owner") }
    replaced_owner := shared 37
    replaced_weak :: replaced_owner.downgrade()
    replaced_result :: replace_then_take(^replaced_owner)
    print(replaced_result.capture().value)
    print(replaced_result.strong_count())
    print(replaced_weak.upgrade() == .None)
    first_owner := shared 11
    rest_owner := shared 23
    first_weak :: first_owner.downgrade()
    rest_weak :: rest_owner.downgrade()
    pair := SharedPair{first: ^first_owner, rest: ^rest_owner}
    captured_first :: take_captured_first(^pair)
    print(captured_first.capture().value)
    print(captured_first.strong_count())
    print(first_weak.upgrade() != .None)
    print(rest_weak.upgrade() == .None)

    dead :: expired_weak()
    print(dead.upgrade() == .None)
}
"#;

#[test]
fn resident_jit_preserves_shared_and_closure_owner_topology() {
    if !jet_jit::cranelift_host_supported() {
        return;
    }

    let dir = common::unique_tmp("jit_shared_ownership");
    fs::create_dir_all(&dir).expect("create scratch fixture directory");
    let file = dir.join("shared_ownership.jet");
    fs::write(&file, SOURCE).expect("write Shared ownership fixture");

    let shown = file.to_string_lossy().into_owned();
    let mut bundle = jet::Loader::load_entry(&shown).expect("load Shared ownership fixture");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.severity, jet::Diagnostics::Severity::Error)),
        "Shared ownership fixture diagnostics: {diagnostics:#?}"
    );
    assert!(
        common::cranelift_lowers(&bundle),
        "Shared ownership fixture must lower to canonical Cranelift MIR: {}",
        common::cranelift_lower_error(&bundle)
    );
    assert!(
        common::cranelift_resident_safe(&bundle),
        "Shared ownership fixture must be resident-safe: {}",
        common::cranelift_resident_safe_detail(&bundle)
    );

    jet_jit::reset_jit_trace_for_test();
    let outcome = jet::Interpreter::dev_iteration(&shown, false, false);
    let stdout = match outcome {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "resident JIT stderr: {stderr}");
            assert!(stderr.is_empty(), "resident JIT stderr: {stderr}");
            stdout
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("Shared ownership fixture failed: {diagnostics:?}")
        }
    };
    assert_eq!(
        stdout,
        "1\n2\n2\n1\n1\n7\n8\ntrue\n8\n102\n202\n302\n2\n1\n1\n13\n1\ntrue\n11\n1\ntrue\ntrue\ntrue\n"
    );
    assert!(
        jet_jit::jit_executed_for_test(),
        "Shared ownership fixture must execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "Shared ownership fixture must stay resident without deopt or fallback"
    );

    fs::remove_dir_all(&dir).expect("remove scratch fixture directory");
}
