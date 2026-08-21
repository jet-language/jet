//! D-BOUND-LAW1=A / I9: shared encoding value types have one Prelude home.

use std::fs;
use std::path::{Path, PathBuf};

const HOME: &str = "crates/jet-codegen/src/Prelude/CoreLib/JetStd/EncodingTypes.rs";
const TYPES: &[&str] = &[
    "EncodingLimits",
    "EncodingFormat",
    "EncodingErrorKind",
    "EncodingCause",
    "EncodingError",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn declaration_sites(root: &Path, name: &str) -> Vec<String> {
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);
    files.sort();

    let mut sites = Vec::new();
    for file in files {
        let relative = file.strip_prefix(root).unwrap_or(&file);
        let text = fs::read_to_string(&file).unwrap_or_default();
        for (line_number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            let words: Vec<_> = code
                .split(|character: char| {
                    character.is_whitespace() || "{}()[],:;".contains(character)
                })
                .filter(|word| !word.is_empty())
                .collect();
            let is_declaration = words
                .windows(2)
                .any(|window| matches!(window[0], "struct" | "enum") && window[1] == name);
            if !is_declaration {
                continue;
            }
            // DataError carries a private unit marker for an absent encoding
            // cause. It is not the shared EncodingError value type.
            if name == "EncodingError"
                && (code.contains("struct EncodingError;")
                    || code.contains("struct EncodingError ;"))
            {
                continue;
            }
            sites.push(format!("{}:{}", relative.display(), line_number + 1));
        }
    }
    sites
}

#[test]
fn encoding_types_have_one_definition_home_and_jit_include() {
    let root = root();
    let home = root.join(HOME);
    let home_source = fs::read_to_string(&home).expect("EncodingTypes.rs is readable");
    let jit_source = fs::read_to_string(root.join("crates/jet-jit/src/enc_stream/mod.rs"))
        .expect("enc_stream/mod.rs is readable");

    assert!(
        jit_source.contains(
            "include!(\"../../../jet-codegen/src/Prelude/CoreLib/JetStd/EncodingTypes.rs\");"
        ),
        "JIT must include the Prelude encoding type definitions"
    );

    for name in TYPES {
        assert!(
            home_source.contains(&format!("struct {name}"))
                || home_source.contains(&format!("enum {name}")),
            "Prelude encoding type home is missing {name}"
        );
        let sites = declaration_sites(&root, name);
        assert_eq!(
            sites,
            vec![HOME.to_string()],
            "{name} must have exactly one definition site, {HOME}"
        );
    }
}
