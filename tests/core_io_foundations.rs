//! Source-owned `core.io` cursor validation across the applicable execution tiers.

mod common;
mod tir_support;

#[test]
fn core_io_truncate_rejects_malformed_utf8_cursor_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_io_truncate_cursor",
        r#"
use core.io as io

fn run() {
    good :: io.truncate(io.string_buf("éx"), 2)
    good ? value -> print(io.getvalue(value)) ! error -> print("good:rejected")

    bad :: io.truncate(io.StringBuf{text: "é", pos: 1}, 2)
    bad == {
        .Ok(_) -> print("bad:accepted")
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> print(context.operation == .Write)
                else -> print(false)
            }
        }
    }
}
"#,
        "é\ntrue\n",
    );
}
