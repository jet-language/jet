//! Source-owned font style defaults across execution tiers.

mod common;
mod tir_support;

#[test]
fn system_font_defaults_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_font_system_source",
        r#"
use core.font as font

fn run() {
    body :: font.system(.Body)
    title :: font.system(.Title)
    mono :: font.system(.Monospace)
    print("{body.family}|{body.size}")
    print("{title.family}|{title.size}")
    print("{mono.family}|{mono.size}")
}
"#,
        "system-ui|14.0\nsystem-ui|24.0\nmonospace|14.0\n",
    );
}
