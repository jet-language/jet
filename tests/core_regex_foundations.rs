//! Source-owned `core.regex` validation and class-boundary behavior across tiers.

mod common;
mod tir_support;

#[test]
fn core_regex_rejects_hostile_quantifiers_and_keeps_class_boundaries() {
    tir_support::assert_tiers_agree(
        "core_regex_foundations",
        r#"
use core.regex as re

fn rejected(pattern: String) -> Bool {
    compiled :: re.compile(pattern) ?? {
        return true
    }
    false
}

fn run() {
    // The limit is 4096, and validation must reject before an overflowing
    // decimal accumulator can turn hostile input into an executable pattern.
    print(rejected("a{{4097}}"))
    print(rejected("a{{999999999999999999999999999999999999}}"))
    print(rejected("["))

    // A closing bracket is a literal first member of a class. The second
    // bracket closes it, so this valid negated class must match the leading a.
    classed :: re.compile("[^]]+") ?? panic("class pattern rejected")
    print(re.find(classed, "a]bc") ?? "none")

    // Escaped class shorthands retain their meaning inside brackets.
    digits :: re.compile("[\\d]+") ?? panic("digit class rejected")
    print(re.find(digits, "id=42") ?? "none")

    // Named groups and noncapturing group syntax remain valid source policy.
    named :: re.compile("(?<word>(?:[a-z]+))") ?? panic("named group rejected")
    print(re.find(named, "123jet") ?? "none")
}
"#,
        "true\ntrue\ntrue\na\n42\njet\n",
    );
}
