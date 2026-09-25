//! Source-owned lexical path containment across all execution tiers.

mod common;
mod tir_support;

#[test]
fn core_path_containment_normalizes_and_rejects_traversal() {
    tir_support::assert_tiers_agree(
        "core_path_foundations",
        r#"
use core.files.path as api

fn run() {
    // Dot segments and both separator spellings are lexical only.
    print(api.is_relative_to("/srv/root/./a/../file", "/srv/root"))
    print(api.is_relative_to("srv\\root\\file", "srv/root"))
    print(api.is_relative_to("srv/root/link/../file", "srv/root"))

    // Component boundaries, not string prefixes, define containment.
    print(api.is_relative_to("/srv/rooted/file", "/srv/root"))
    print(api.is_relative_to("/srv/root/../outside", "/srv/root"))

    // A relative current-directory base accepts children but not an escape.
    print(api.is_relative_to("child/file", "."))
    print(api.is_relative_to("../outside", "."))
    print(api.is_relative_to("..foo/file", "."))

    // Anchors remain distinct, including the platform-neutral drive spelling.
    print(api.is_relative_to("C:\\root\\child", "C:/root"))
    print(api.is_relative_to("/srv/root", "srv/root"))
}
"#,
        "true\ntrue\ntrue\nfalse\nfalse\ntrue\nfalse\ntrue\ntrue\nfalse\n",
    );
}
