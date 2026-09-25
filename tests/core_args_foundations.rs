//! Source-owned `core.args` defaults and destination aliases across all tiers.

mod common;
mod tir_support;

#[test]
fn core_args_defaults_do_not_override_a_named_alias() {
    tir_support::assert_tiers_agree(
        "core_args_foundations",
        r#"
use core.args as api

fn run() {
    definition :: api.define("mode", "execution mode", false)
    aliased :: api.dest(definition, "profile")
    configured :: api.default_value(aliased, "safe")
    defs :: [ArgDef]{configured}

    named :: api.decode_argv([String]{"tool", "--mode", "cli"})
    named_with_defaults :: api.apply_defaults(named, defs)
    print(api.get_text(named_with_defaults, "mode"))
    print(api.get_text(named_with_defaults, "profile"))

    absent :: api.apply_defaults(api.decode_argv([String]{"tool"}), defs)
    print(api.get_text(absent, "profile"))
}
"#,
        "cli\n\nsafe\n",
    );
}
