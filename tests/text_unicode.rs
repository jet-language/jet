//! Card #298: pinned Unicode regeneration and end-to-end text behavior.

use std::fs;
use std::path::Path;
use std::process::Command;

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

const TEXT_PARSER_HELPERS_SOURCE: &str = r#"
use core.text.parse as parse
use combinators

fn run() {
    // Failure fallbacks differ from the expected payload, so None cannot pass.
    binary :: (parse.parse_int_base("101101", 2) ?? -1) == 45
    hexadecimal :: (parse.parse_int_base("FF", 16) ?? -1) == 255
    base36 :: (parse.parse_int_base("z", 36) ?? -1) == 35
    print("integer valid: base2={binary} base16={hexadecimal} base36={base36}")

    base_low :: parse.parse_int_base("1", 1) == .None
    base_high :: parse.parse_int_base("1", 37) == .None
    empty :: parse.parse_int_base("", 10) == .None
    plus_only :: parse.parse_int_base("+", 10) == .None
    minus_only :: parse.parse_int_base("-", 10) == .None
    illegal_digit :: parse.parse_int_base("12z", 10) == .None
    print("integer invalid: base1={base_low} base37={base_high} empty={empty} plus={plus_only} minus={minus_only} digit={illegal_digit}")

    true_text :: (parse.parse_bool(" \tTrUe\n") ?? false) == true
    false_text :: (parse.parse_bool("\tOFF ") ?? true) == false
    invalid_bool :: parse.parse_bool("truthy") == .None
    print("bool: true={true_text} false={false_text} invalid={invalid_bool}")

    signed :: combinators.int_dec(combinators.input(" \t-42tail"))
    if signed == .Val(decimal) {
        rest :: combinators.remaining(decimal.next)
        print("decimal: value={decimal.n} pos={decimal.next.pos} rest={rest}")
    } else {
        print("decimal: missing")
    }
    no_digits :: combinators.int_dec(combinators.input(" \t+tail")) == .None
    print("no digits={no_digits}")

    split_utf8 :: combinators.take_n(combinators.input("é"), 1) == .None
    print("take split utf8={split_utf8}")
    if combinators.take_n(combinators.input("jet"), 2) == .Val(taken) {
        rest :: combinators.remaining(taken.next)
        print("take valid: chunk={taken.chunk} pos={taken.next.pos} rest={rest}")
    } else {
        print("take valid: rejected")
    }
}
"#;

const TEXT_PARSER_HELPERS_GOLDEN: &str = "integer valid: base2=true base16=true base36=true\n\
integer invalid: base1=true base37=true empty=true plus=true minus=true digit=true\n\
bool: true=true false=true invalid=true\n\
decimal: value=-42 pos=5 rest=tail\n\
no digits=true\n\
take split utf8=true\n\
take valid: chunk=je pos=2 rest=t\n";

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
            "Tools/agent/gen-unicode-tables.mjs",
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
        hello
            .rust
            .matches("pub const UNICODE_STRING_VERSION")
            .count(),
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
        text.rust
            .matches("pub const UNICODE_STRING_VERSION")
            .count(),
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
    let text = fs::read_to_string(root.join("crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs"))
        .unwrap();
    let start = text
        .find("// ── core.text helpers")
        .expect("Unicode AOT block start");
    let end = text
        .find("fn jet_std_fs_read")
        .expect("Unicode AOT block end");
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
            "÷" => if !current.is_empty() { &segments.push(std::mem::take(&mut current)); },
            "×" => {},
            hex => {
                let ch = char::from_u32(u32::from_str_radix(hex, 16).ok()?)?;
                full.push(ch);
                current.push(ch);
                any = true;
            }
        }
    }
    if !current.is_empty() { &segments.push(current); }
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
        if !fields[12].is_empty() { &upper.insert(cp, cps(fields[12])); }
        if !fields[13].is_empty() { &lower.insert(cp, cps(fields[13])); }
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
    assert!(
        compiled.status.success(),
        "AOT Unicode harness rejected:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ran = Command::new(&binary).output().unwrap();
    assert!(
        ran.status.success(),
        "AOT Unicode corpus failed:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        "19965 1557 1112064 1093 1826 512\n"
    );
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

FOLDED :: prep { text.casefold("Straßeİς") }
LOWERED :: prep { text.lower("ΟΣ") }
UPPERED :: prep { text.upper("ßև") }
METHOD_LOWERED :: prep { "__PINNED_UPPER__".to_lower() }
METHOD_UPPERED :: prep { "__PINNED_LOWER__".to_upper() }
METHOD_TRIMMED :: prep { "__PINNED_SPACE__jet__PINNED_SPACE__".trim() }
KEYCAP :: prep { text.display_width("1️⃣") }
EMOJI :: prep { text.display_width("©️") }
IGNORABLE :: prep { text.display_width("́‍") }
CLASSES :: prep { text.is_alphabetic("Ж") && text.is_numeric("٣") && text.is_whitespace(" ") }
REGEX_SPACE :: prep { re.is_match(.{"\s"}, " ") }

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
    runtime_regex_space :: re.is_match(.{"\s"}, " ")
    regex_alpha :: re.is_match(.{"\p{{Alphabetic}}+"}, "Ж")
    regex_number :: re.is_match(.{"\p{{Number}}+"}, "٣")
    regex_whitespace :: re.is_match(.{"\p{{White_Space}}+"}, " ")
    insensitive :: re.compile_with("k", re.flags(true, false, false)) ?? panic("regex")
    print("{FOLDED}|{runtime_folded}")
    print("{LOWERED}|{runtime_lowered}")
    print("{UPPERED}|{runtime_uppered}")
    print(METHOD_LOWERED == "__PINNED_LOWER__" && runtime_method_lowered == "__PINNED_LOWER__")
    print(METHOD_UPPERED == "__PINNED_UPPER__" && runtime_method_uppered == "__PINNED_UPPER__")
    print("{METHOD_TRIMMED}|{runtime_method_trimmed}")
    print("{KEYCAP}|{runtime_keycap}")
    print("{EMOJI}|{runtime_emoji}")
    print("{IGNORABLE}|{runtime_ignorable}")
    print("{CLASSES}|{runtime_classes}")
    print("{REGEX_SPACE}|{runtime_regex_space}")
    print(regex_alpha && regex_number && regex_whitespace && insensitive.is_match("K"))
}
"#
    .replace(
        "__PINNED_UPPER__",
        &char::from_u32(0xA7CE).unwrap().to_string(),
    )
    .replace(
        "__PINNED_LOWER__",
        &char::from_u32(0xA7CF).unwrap().to_string(),
    )
    .replace(
        "__PINNED_SPACE__",
        &char::from_u32(0x2003).unwrap().to_string(),
    );
    let (code, stdout, stderr) = common::build_and_run("jet_text_unicode", "parity", &source);
    assert_eq!(code, 0, "Unicode parity fixture failed: {stderr}");
    assert_eq!(
        stdout,
        "strassei̇σ|strassei̇σ\nος|ος\nSSԵՒ|SSԵՒ\ntrue\ntrue\njet|jet\n2|2\n2|2\n0|0\ntrue|true\ntrue|true\ntrue\n"
    );
}

#[test]
fn scalar_inspector_names_invisible_codepoints() {
    if !common::have_rustc() {
        eprintln!("note: rustc not found; skipping scalar inspector proof");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join("crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs"))
        .unwrap();
    let start = text
        .find("// ── core.text helpers")
        .expect("Unicode AOT block start");
    let end = text
        .find("fn jet_std_fs_read")
        .expect("Unicode AOT block end");
    let unicode_block = &text[start..end];
    let root_text = root.to_string_lossy().replace('\\', "\\\\");
    let suffix = r#"
fn main() {
    let mut checked = 0usize;
    for raw in include_str!("__ROOT__/tests/data/unicode/ucd/UnicodeData.txt").lines() {
        let fields: Vec<_> = raw.split(';').collect();
        if fields.len() < 14 { continue; }
        let cp = u32::from_str_radix(fields[0], 16).unwrap();
        let name = fields[1];
        if name.ends_with(", First>") || name.ends_with(", Last>") { continue; }
        let name = if name == "<control>" { fields[10] } else { name };
        if name.is_empty() || name.starts_with('<') { continue; }
        let Some(ch) = char::from_u32(cp) else { continue; };
        let expected = format!("U+{:04X} {}", cp, name);
        let got = jet_text_inspect(&ch.to_string()).into_iter().next().unwrap();
        assert_eq!(got, expected, "name mismatch for U+{cp:04X}");
        checked += 1;
    }
    assert!(checked > 30000, "pinned UnicodeData name rows unexpectedly low: {checked}");
    let family = "👨‍👩‍👦".to_string();
    assert_eq!(jet_text_inspect(&family)[1], "U+200D ZERO WIDTH JOINER");
    println!("{checked}");
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
    let dir = common::unique_tmp("jet_scalar_inspector");
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("scalar_inspector.rs");
    let binary = dir.join("scalar_inspector");
    fs::write(&source, harness).unwrap();
    let compiled = Command::new("rustc")
        .args(["--edition=2021", "-O"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "scalar inspector harness rejected:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ran = Command::new(&binary).output().unwrap();
    assert!(
        ran.status.success(),
        "scalar inspector proof failed:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "40074\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unicode_grapheme_zwj_requires_pictographic_context_on_all_run_tiers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = r#"
use core.text as text

fn run() {
    unrelated := text.graphemes("a‍👨")
    print(unrelated.len())
    loop part in unrelated -> print(part)

    pictographic := text.graphemes("👨‍👩")
    print(pictographic.len())
    loop part in pictographic -> print(part)

    extended := text.graphemes("👨́‍👩")
    print(extended.len())
    loop part in extended -> print(part)
}
"#;
    let scratch = common::Scratch::new("unicode_grapheme_zwj_context");
    let program = scratch.join("grapheme_zwj_context.jet");
    fs::write(&program, source).expect("write ZWJ context fixture");
    fs::write(
        scratch.join("package.jet"),
        "name: \"grapheme_zwj_context\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
    )
    .expect("write ZWJ context authority");

    for (tier, release, interpret) in [
        ("release", true, false),
        ("default", false, false),
        ("interpret", false, true),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
        command.arg("run");
        if release {
            command.arg("--release");
        }
        if interpret {
            command.arg("--interpret");
        }
        let output = command
            .arg(&program)
            .current_dir(&scratch.path)
            .env("JET_RUN_CACHE_DIR", scratch.join(&format!("{tier}-run")))
            .env("JET_STORE_DIR", scratch.join(&format!("{tier}-build")))
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("spawn ZWJ context fixture in {tier} tier: {error}"));
        assert!(
            output.status.success(),
            "ZWJ context fixture failed in {tier} tier:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout,
            "2\na‍\n👨\n1\n👨‍👩\n1\n👨́‍👩\n".as_bytes().to_vec(),
            "ZWJ context segmentation differed in {tier} tier"
        );
    }
}

#[test]
fn unicode_text_audit_matches_golden_on_all_run_tiers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("Examples/features/text/unicode_text_audit.jet");
    let expected = fs::read(root.join("Examples/features/expected/text/unicode_text_audit.out"))
        .expect("read Unicode text audit golden");
    let scratch = common::Scratch::new("unicode_text_audit_tiers");

    for (tier, release, interpret) in [
        ("release", true, false),
        ("default", false, false),
        ("interpret", false, true),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
        command.arg("run");
        if release {
            command.arg("--release");
        }
        if interpret {
            command.arg("--interpret");
        }
        let output = command
            .arg(&example)
            .current_dir(root)
            .env("JET_RUN_CACHE_DIR", scratch.join(&format!("{tier}-run")))
            .env("JET_STORE_DIR", scratch.join(&format!("{tier}-build")))
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("spawn Unicode audit {tier} tier: {error}"));
        assert!(
            output.status.success(),
            "Unicode audit {tier} tier failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout, expected,
            "Unicode audit {tier} tier did not match golden"
        );
    }
}

#[test]
fn wrap_semantics_audit_matches_golden_on_all_run_tiers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("Examples/features/text/wrap_semantics_audit.jet");
    let expected = fs::read(
        root.join("Examples/features/expected/text/wrap_semantics_audit.out"),
    )
    .expect("read textwrap semantics audit golden");
    let scratch = common::Scratch::new("wrap_semantics_audit_tiers");

    for (tier, release, interpret) in [
        ("release", true, false),
        ("default", false, false),
        ("interpret", false, true),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
        command.arg("run");
        if release {
            command.arg("--release");
        }
        if interpret {
            command.arg("--interpret");
        }
        let output = command
            .arg(&example)
            .current_dir(root)
            .env("JET_RUN_CACHE_DIR", scratch.join(&format!("{tier}-run")))
            .env("JET_STORE_DIR", scratch.join(&format!("{tier}-build")))
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("spawn textwrap semantics audit {tier} tier: {error}"));
        assert!(
            output.status.success(),
            "textwrap semantics audit {tier} tier failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout, expected,
            "textwrap semantics audit {tier} tier did not match golden"
        );
    }
}

#[test]
fn string_from_bytes_matches_golden_on_all_run_tiers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("Examples/features/strings/from_bytes.jet");
    let expected = fs::read(root.join("Examples/features/expected/strings/from_bytes.out"))
        .expect("read String.from_bytes golden");
    let scratch = common::Scratch::new("string_from_bytes_tiers");

    for (tier, release, interpret) in [
        ("release", true, false),
        ("default", false, false),
        ("interpret", false, true),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
        command.arg("run");
        if release {
            command.arg("--release");
        }
        if interpret {
            command.arg("--interpret");
        }
        let output = command
            .arg(&example)
            .current_dir(root)
            .env("JET_RUN_CACHE_DIR", scratch.join(&format!("{tier}-run")))
            .env("JET_STORE_DIR", scratch.join(&format!("{tier}-build")))
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("spawn String.from_bytes {tier} tier: {error}"));
        assert!(
            output.status.success(),
            "String.from_bytes {tier} tier failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout, expected,
            "String.from_bytes {tier} tier did not match golden"
        );
    }
}

#[test]
fn text_parser_helpers_preserve_optional_failure_across_tiers() {
    let files = [
        ("main.jet", TEXT_PARSER_HELPERS_SOURCE),
        ("combinators.jet", include_str!("../Core/text/combinators.jet")),
    ];
    for (tier, result) in [
        (
            "AOT",
            tir_support::build_release_and_run_multi(
                "text_parser_helpers_aot", "main.jet", &files,
            ),
        ),
        (
            "default",
            tir_support::run_default_multi(
                "text_parser_helpers_default", "main.jet", &files,
            ),
        ),
        (
            "interpreter",
            tir_support::run_interpret_multi(
                "text_parser_helpers_interpreter", "main.jet", &files,
            ),
        ),
    ] {
        assert_eq!(result.0, 0, "{tier} helper fixture failed:\n{}", result.2);
        assert_eq!(result.1, TEXT_PARSER_HELPERS_GOLDEN, "{tier} helper output");
    }
    let scratch = common::Scratch::new("text_parser_helpers_web");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    for (name, source) in files {
        fs::write(scratch.path.join(name), source).unwrap();
    }
    let entry = scratch.path.join("main.jet");
    let output = jet::compile_web(&entry.to_string_lossy()).unwrap_or_else(|diagnostics| {
        panic!("web rejected text parser helper fixture: {diagnostics:#?}")
    });
    let web = output.web.expect("web compile must produce web artifacts");
    let web_root = scratch.path.join("web");
    let build_dir = web_root.join("build");
    fs::create_dir_all(&build_dir).unwrap();
    fs::write(build_dir.join("web.manifest.json"), &web.manifest_json).unwrap();
    fs::write(build_dir.join("app.js"), &web.js_app).unwrap();
    fs::write(build_dir.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(build_dir.join("app_wasm.rs"), &web.wasm_rust).unwrap();
    let wasm = Command::new("rustc")
        .current_dir(&web_root)
        .args([
            "--edition", "2021", "--target", "wasm32-unknown-unknown",
            "--crate-type", "cdylib", "-O", "build/app_wasm.rs",
            "-o", "build/app.wasm",
        ])
        .output()
        .expect("rustc is required for the web text parser helper regression");
    assert!(
        wasm.status.success(),
        "web text parser helper wasm failed:\n{}",
        String::from_utf8_lossy(&wasm.stderr),
    );
    let node = Command::new("node")
        .current_dir(&build_dir)
        .args([
            "--input-type=module", "-e",
            "const { jet_main } = await import('./app.js');\n\
             const result = await jet_main();\n\
             if (result !== undefined) throw new Error('web entry success result must be undefined');",
        ])
        .output()
        .expect("Node is required for the web text parser helper regression");
    assert!(
        node.status.success(),
        "web text parser helpers failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&node.stdout), TEXT_PARSER_HELPERS_GOLDEN);
    assert!(node.stderr.is_empty(), "web helper stderr: {}", String::from_utf8_lossy(&node.stderr));
}
