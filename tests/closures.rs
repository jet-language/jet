//! M8 closure compile checks (rustc-as-verifier battery subset).

#[test]
fn lambdas_compile_to_rust() {
    let src = r#"
fn apply(f: fn(Int) -> Int, x: Int) -> Int {
    return f(f(x))
}

fn main() {
    nums :: [1, 2, 3]
    print(nums.map((n: Int) => n * n).len())
    total := 0
    nums.each((n: Int) => { total += n })
    print(total)
    print(apply((x: Int) => x + 1, 5))
    print(nums.filter((n: Int) => n > 1).len())
    print(nums.any((n: Int) => n == 2))
    print(nums.reduce(0, (acc: Int, n: Int) => acc + n))
}
"#;
    let out = jet::compile(src).expect("closures should compile");
    assert!(!out.rust.contains("unsafe"), "invariant I1");
    assert!(
        out.rust.contains("jet_list_map"),
        "map should lower to prelude helper"
    );
    assert!(out.rust.contains("move |"), "lambdas should emit closures");
}

#[test]
fn stored_callback_boxes() {
    let src = r#"
fn twice(f: fn(Int) -> Int, x: Int) -> Int {
    return f(f(x))
}

fn main() {
    bump :: (x: Int) => x + 1
    print(twice(bump, 10))
}
"#;
    let out = jet::compile(src).expect("stored fn value should compile");
    assert!(out.rust.contains("Box::new"), "stored lambdas should box");
}

#[test]
fn take_prefix_moves_non_clone_capture() {
    let src = r#"
struct NoClone { tag: Int }
fn main() {
    item :: NoClone { tag: 7 }
    f :: take(item) (n: Int) => n + item.tag
    print(f(1))
}
"#;
    jet::compile(src).expect("take-prefixed lambda should compile");
}

#[test]
fn fn_field_callback() {
    let src = r#"
struct Worker { step: fn(Int) -> Int }
fn main() {
    w :: Worker { step: (n: Int) => n + 1 }
    print(w.step(4))
}
"#;
    let out = jet::compile(src).expect("fn field callback should compile");
    assert!(out.rust.contains("Box::new"), "fn fields should box");
}

#[test]
fn sort_by_with_lambda() {
    let src = r#"
fn main() {
    nums := [3, 1, 2]
    nums.sort_by((n: Int) => n)
    print(nums[0])
}
"#;
    jet::compile(src).expect("sort_by lambda should compile");
}
