//! Card #298: pinned Unicode regeneration and end-to-end text behavior.

use std::fs;
use std::path::Path;
use std::process::Command;

mod common;

#[test]
fn pinned_unicode_tables_regenerate_byte_identically() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let checksums = Command::new("sha256sum")
        .args(["--check", "SHA256SUMS"])
        .current_dir(root.join("tests/data/unicode"))
        .output()
        .expect("verify vendored Unicode inputs");
    assert!(
        checksums.status.success(),
        "vendored Unicode checksum mismatch:\n{}{}",
        String::from_utf8_lossy(&checksums.stdout),
        String::from_utf8_lossy(&checksums.stderr),
    );
    let output = Command::new("node")
        .args([
            "scripts/agent/gen-unicode-tables.mjs",
            "--check",
            "tests/data/unicode/ucd",
        ])
        .current_dir(root)
        .output()
        .expect("run pinned Unicode generator");
    assert!(
        output.status.success(),
        "pinned Unicode tables do not regenerate byte-identically:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn generated_rust_emits_only_the_unicode_tier_the_program_needs() {
    let hello = jet::compile(r#"fn run() { print("hello") }"#).expect("hello compiles");
    assert_eq!(
        hello.rust.matches("pub const UNICODE_STRING_VERSION").count(),
        1,
        "the always-on Unicode string prelude must be emitted exactly once",
    );
    assert_eq!(
        hello.rust.matches("pub fn jet_unicode_trim_view").count(),
        1,
        "the always-on Unicode string helpers must be emitted exactly once",
    );
    assert!(
        !hello.rust.contains("UNICODE_DECOMP_POOL"),
        "a trivial program must not carry the core.text Unicode tables",
    );
    assert!(
        hello.rust.len() < 350_000,
        "trivial generated Rust grew to {} bytes",
        hello.rust.len(),
    );

    let text = jet::compile(
        r#"use core.text as text
fn run() { print(text.nfc("é")) }"#,
    )
    .expect("core.text program compiles");
    assert_eq!(
        text.rust.matches("pub const UNICODE_STRING_VERSION").count(),
        1,
        "core.text must not duplicate the always-on Unicode string prelude",
    );
    assert_eq!(
        text.rust.matches("pub fn jet_unicode_trim_view").count(),
        1,
        "core.text must not duplicate the always-on Unicode string helpers",
    );
    assert_eq!(
        text.rust.matches("pub const UNICODE_VERSION").count(),
        1,
        "core.text Unicode tables must be emitted exactly once",
    );
    assert_eq!(
        text.rust.matches("pub static UNICODE_DECOMP_POOL").count(),
        1,
        "core.text normalization tables must be emitted exactly once",
    );
}

#[test]
fn aot_prelude_passes_full_unicode_corpora() {
    if !common::have_rustc() {
        eprintln!("note: rustc not found; skipping AOT Unicode corpus proof");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join("crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs")).unwrap();
    let start = text.find("// ── core.text.unicode helpers").expect("Unicode AOT block start");
    let end = text.find("fn jet_std_fs_read").expect("Unicode AOT block end");
    let unicode_block = &text[start..end];
    let root_text = root.to_string_lossy().replace('\\', "\\\\");
    let suffix = r#"
fn cps(field: &str) -> String {
    field.split_whitespace()
        .filter_map(|hex| u32::from_str_radix(hex, 16).ok())
        .filter_map(char::from_u32)
        .collect()
}

fn parse_break(line: &str) -> Option<(String, Vec<String>)> {
    let mut full = String::new();
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut any = false;
    for token in line.split_whitespace() {
        match token {
            "÷" => if !current.is_empty() { segments.push(std::mem::take(&mut current)); },
            "×" => {},
            hex => {
                let ch = char::from_u32(u32::from_str_radix(hex, 16).ok()?)?;
                full.push(ch);
                current.push(ch);
                any = true;
            }
        }
    }
    if !current.is_empty() { segments.push(current); }
    any.then_some((full, segments))
}

fn check_break(corpus: &str, segment: impl Fn(&String) -> Vec<String>) -> usize {
    let mut checked = 0;
    for raw in corpus.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let Some((full, expected)) = parse_break(line) else { continue };
        assert_eq!(segment(&full), expected, "break mismatch: {raw}");
        checked += 1;
    }
    checked
}

fn main() {
    let normalization = include_str!("__ROOT__/tests/data/unicode/NormalizationTest.txt");
    let mut normalization_lines = 0;
    for raw in normalization.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('@') { continue; }
        let fields: Vec<_> = line.split(';').collect();
        if fields.len() < 5 { continue; }
        let c1 = cps(fields[0]); let c2 = cps(fields[1]); let c3 = cps(fields[2]);
        let c4 = cps(fields[3]); let c5 = cps(fields[4]);
        for input in [&c1, &c2, &c3] {
            assert_eq!(jet_text_nfc(input), c2); assert_eq!(jet_text_nfd(input), c3);
        }
        for input in [&c4, &c5] {
            assert_eq!(jet_text_nfc(input), c4); assert_eq!(jet_text_nfd(input), c5);
        }
        for input in [&c1, &c2, &c3, &c4, &c5] {
            assert_eq!(jet_text_nfkc(input), c4); assert_eq!(jet_text_nfkd(input), c5);
        }
        normalization_lines += 1;
    }
    let folding = include_str!("__ROOT__/tests/data/unicode/ucd/CaseFolding.txt");
    let mut fold_lines = 0;
    for raw in folding.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() { continue; }
        let fields: Vec<_> = line.split(';').map(str::trim).collect();
        if fields.len() < 3 || !matches!(fields[1], "C" | "F") { continue; }
        assert_eq!(jet_text_casefold(&cps(fields[0])), cps(fields[2]), "fold mismatch: {raw}");
        fold_lines += 1;
    }
    let mut lower = std::collections::BTreeMap::<u32, String>::new();
    let mut upper = std::collections::BTreeMap::<u32, String>::new();
    for raw in include_str!("__ROOT__/tests/data/unicode/ucd/UnicodeData.txt").lines() {
        let fields: Vec<_> = raw.split(';').collect();
        if fields.len() < 14 { continue; }
        let cp = u32::from_str_radix(fields[0], 16).unwrap();
        if !fields[12].is_empty() { upper.insert(cp, cps(fields[12])); }
        if !fields[13].is_empty() { lower.insert(cp, cps(fields[13])); }
    }
    for raw in include_str!("__ROOT__/tests/data/unicode/ucd/SpecialCasing.txt").lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() { continue; }
        let fields: Vec<_> = line.split(';').map(str::trim).collect();
        if fields.len() < 5 || !fields[4].is_empty() { continue; }
        let cp = u32::from_str_radix(fields[0], 16).unwrap();
        lower.insert(cp, cps(fields[1])); upper.insert(cp, cps(fields[3]));
    }
    let mut casing_scalars = 0;
    for cp in 0..=0x10ffff {
        let Some(ch) = char::from_u32(cp) else { continue };
        let input = ch.to_string();
        assert_eq!(jet_text_lower(&input), lower.get(&cp).cloned().unwrap_or_else(|| input.clone()));
        assert_eq!(jet_text_upper(&input), upper.get(&cp).cloned().unwrap_or_else(|| input.clone()));
        casing_scalars += 1;
    }
    assert_eq!(jet_text_lower(&"ΟΣ".to_string()), "ος");
    let grapheme = check_break(
        include_str!("__ROOT__/tests/data/unicode/GraphemeBreakTest.txt"),
        jet_text_graphemes,
    );
    let word = check_break(
        include_str!("__ROOT__/tests/data/unicode/WordBreakTest.txt"),
        jet_text_word_segments,
    );
    let sentence = check_break(
        include_str!("__ROOT__/tests/data/unicode/SentenceBreakTest.txt"),
        jet_text_sentence_segments,
    );
    assert!(normalization_lines > 15000 && fold_lines > 1500);
    assert!(grapheme > 500 && word > 500 && sentence > 100);
    println!("{normalization_lines} {fold_lines} {casing_scalars} {grapheme} {word} {sentence}");
}
"#
    .replace("__ROOT__", &root_text);
    let mut harness = format!(
        "include!(r#\"{0}/crates/jet-codegen/src/Prelude/Core/UnicodeString.rs\"#);\n\
         include!(r#\"{0}/crates/jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs\"#);\n",
        root_text,
    );
    harness.push_str(
        "mod jet_std {\n#[derive(Clone,Copy)] pub enum TextWidthAmbiguous { Narrow, Wide }\n\
         #[derive(Clone,Copy)] pub enum TextWidthControls { Zero, Reject }\n\
         pub struct TextWidth { pub ambiguous: TextWidthAmbiguous, pub controls: TextWidthControls }\n\
         pub struct TextError { pub message: String }\n}\n",
    );
    harness.push_str(unicode_block);
    harness.push_str(&suffix);
    let dir = common::unique_tmp("jet_unicode_aot_corpora");
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("unicode_aot.rs");
    let binary = dir.join("unicode_aot");
    fs::write(&source, harness).unwrap();
    let compiled = Command::new("rustc")
        .args(["--edition=2021", "-O"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(compiled.status.success(), "AOT Unicode harness rejected:\n{}", String::from_utf8_lossy(&compiled.stderr));
    let ran = Command::new(&binary).output().unwrap();
    assert!(ran.status.success(), "AOT Unicode corpus failed:\n{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "19965 1557 1112064 1093 1826 512\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unicode_text_behavior_matches_comptime_and_aot() {
    if !common::have_rustc() {
        eprintln!("note: rustc not found; skipping Unicode text AOT/comptime proof");
        return;
    }
    let source = r#"
use core.text as text
use core.regex as re

comptime folded = text.casefold("Straßeİς")
comptime lowered = text.lower("ΟΣ")
comptime uppered = text.upper("ßև")
comptime method_lowered = "__PINNED_UPPER__".to_lower()
comptime method_uppered = "__PINNED_LOWER__".to_upper()
comptime method_trimmed = "__PINNED_SPACE__jet__PINNED_SPACE__".trim()
comptime keycap = text.display_width("1️⃣")
comptime emoji = text.display_width("©️")
comptime ignorable = text.display_width("́‍")
comptime classes = text.is_alphabetic("Ж") && text.is_numeric("٣") && text.is_whitespace(" ")
comptime regex_space = re.is_match("\\s", " ") ?? false

fn run() {
    runtime_folded :: text.casefold("Straßeİς")
    runtime_lowered :: text.lower("ΟΣ")
    runtime_uppered :: text.upper("ßև")
    runtime_method_lowered :: "__PINNED_UPPER__".to_lower()
    runtime_method_uppered :: "__PINNED_LOWER__".to_upper()
    runtime_method_trimmed :: "__PINNED_SPACE__jet__PINNED_SPACE__".trim()
    runtime_keycap :: text.display_width("1️⃣")
    runtime_emoji :: text.display_width("©️")
    runtime_ignorable :: text.display_width("́‍")
    runtime_classes :: text.is_alphabetic("Ж") && text.is_numeric("٣") && text.is_whitespace(" ")
    runtime_regex_space :: re.is_match("\\s", " ") ?? false
    regex_alpha :: re.is_match("\\p{{Alphabetic}}+", "Ж") ?? false
    regex_number :: re.is_match("\\p{{Number}}+", "٣") ?? false
    regex_whitespace :: re.is_match("\\p{{White_Space}}+", " ") ?? false
    insensitive :: re.compile_with("k", re.flags(true, false, false)) ?? panic("regex")
    print("{folded}|{runtime_folded}")
    print("{lowered}|{runtime_lowered}")
    print("{uppered}|{runtime_uppered}")
    print(method_lowered == "__PINNED_UPPER__" && runtime_method_lowered == "__PINNED_UPPER__")
    print(method_uppered == "__PINNED_LOWER__" && runtime_method_uppered == "__PINNED_LOWER__")
    print("{method_trimmed}|{runtime_method_trimmed}")
    print("{keycap}|{runtime_keycap}")
    print("{emoji}|{runtime_emoji}")
    print("{ignorable}|{runtime_ignorable}")
    print("{classes}|{runtime_classes}")
    print("{regex_space}|{runtime_regex_space}")
    print(regex_alpha && regex_number && regex_whitespace && insensitive.is_match("K"))
}
"#
    .replace("__PINNED_UPPER__", &char::from_u32(0xA7CE).unwrap().to_string())
    .replace("__PINNED_LOWER__", &char::from_u32(0xA7CF).unwrap().to_string())
    .replace("__PINNED_SPACE__", &char::from_u32(0x2003).unwrap().to_string());
    let (code, stdout, stderr) = common::build_and_run("jet_text_unicode", "parity", &source);
    assert_eq!(code, 0, "Unicode parity fixture failed: {stderr}");
    assert_eq!(
        stdout,
        "strassei̇σ|strassei̇σ\nος|ος\nSSԵՒ|SSԵՒ\ntrue\ntrue\njet|jet\n2|2\n2|2\n0|0\ntrue|true\ntrue|true\ntrue\n"
    );
}
