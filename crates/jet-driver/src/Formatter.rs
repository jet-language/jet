//! Driver-owned formatter facade.
//!
//! The parser formatter has no dependency on the Package model.  This facade
//! masks the optional leading Package carrier before parsing and restores it
//! verbatim after formatting, so parser spans stay byte-exact without creating
//! a second Package field grammar (D-ECO-INLINEPACKAGE1).

pub use jet_parser::Formatter::{
    canonical_program, format_program, format_synthetic_program, FormatOptions,
    retired_interpolation_selector_edits, retired_print_family_edits, retired_type_edits,
    unified_diff,
};

use crate::Diagnostics::Diagnostic;
use crate::Package;

/// Format ordinary Jet source while preserving one leading inline Package
/// declaration exactly as authored.
pub fn format_source(src: &str) -> Result<String, Vec<Diagnostic>> {
    format_source_with_options(src, FormatOptions::default())
}

/// Format ordinary Jet source with explicit formatter controls.  The Package
/// carrier is structural context; its fields remain owned by PackageFacts and
/// are not reformatted by this module.
pub fn format_source_with_options(
    src: &str,
    options: FormatOptions,
) -> Result<String, Vec<Diagnostic>> {
    let (mut ordinary, block) = match Package::mask_inline_package_source(src) {
        Ok(value) => value,
        Err(error) => return Err(vec![error.diagnostic()]),
    };
    let Some(block) = block else {
        return jet_parser::Formatter::format_source_with_options(src, options);
    };

    // The extractor guarantees that everything before the carrier is trivia.
    // Keep that trivia verbatim, but hide it from the ordinary formatter so it
    // cannot move comments across the package carrier.
    let prefix_end = block.span.start;
    let mut ordinary_bytes = ordinary.into_bytes();
    for byte in ordinary_bytes.iter_mut().take(prefix_end) {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
    ordinary = String::from_utf8(ordinary_bytes)
        .expect("Package masking preserves valid UTF-8");
    let formatted = jet_parser::Formatter::format_source_with_options(&ordinary, options)?;
    let mut out = String::with_capacity(src.len() + formatted.len());
    out.push_str(&src[..prefix_end]);
    out.push_str(block.source(src));
    if !formatted.is_empty() {
        if !matches!(out.as_bytes().last(), Some(b'\n' | b'\r')) {
            out.push('\n');
        }
        out.push_str(&formatted);
    }
    Ok(out)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_preserves_inline_package_and_ordinary_source() {
        let source = "package {\n    name: \"inline-demo\"\n}\n\npub(package) fn helper() => String { return \"ok\" }\nfn run() { print(helper()) }\n";
        let formatted = format_source(source).expect("inline Package source should format");
        assert!(formatted.starts_with("package {\n    name: \"inline-demo\"\n}\n"));
        assert!(formatted.contains("pub(package) fn helper"));
        assert!(formatted.contains("fn run"));
    }

    #[test]
    fn format_rejects_duplicate_inline_package_carriers() {
        let source = "package { name: \"one\" }\npackage { name: \"two\" }\nfn run() {}\n";
        let diagnostics = format_source(source).expect_err("duplicate carriers must fail");
        assert_eq!(diagnostics[0].code, "E1361");
    }
}
