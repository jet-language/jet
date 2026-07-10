mod common;

#[test]
fn normalize_is_lexical_rooted_and_platform_native() {
    if !common::have_rustc() {
        return;
    }
    let absolute = if cfg!(windows) {
        "C:/../../alpha/../beta"
    } else {
        "/../../alpha/../beta"
    };
    let root = if cfg!(windows) { "C:/" } else { "/" };
    let windows_cases = if cfg!(windows) {
        r#"
    print(path.normalize("C:../../leaf"))
    print(path.normalize("//server/share/../../leaf"))"#
    } else {
        ""
    };
    let src = format!(
        r#"
use core.path as path

fn run() {{
    print(path.normalize("../../alpha/./beta/../gamma"))
    print(path.normalize("{absolute}"))
    print(path.normalize(""))
    print(path.normalize("{root}")){windows_cases}
}}
"#
    );
    let (code, stdout, stderr) = common::build_and_run("jet_path_durability", "normalize", &src);
    assert_eq!(code, 0, "generated program failed: {stderr}");
    let sep = std::path::MAIN_SEPARATOR;
    let expected_relative = format!("..{sep}..{sep}alpha{sep}gamma");
    let expected_absolute = if cfg!(windows) {
        format!("C:{sep}beta")
    } else {
        format!("{sep}beta")
    };
    let expected_root = if cfg!(windows) {
        format!("C:{sep}")
    } else {
        sep.to_string()
    };
    let mut expected = vec![expected_relative, expected_absolute, String::new(), expected_root];
    if cfg!(windows) {
        expected.push(format!("C:..{sep}..{sep}leaf"));
        expected.push(format!(r"\\server{sep}share{sep}leaf"));
    }
    assert_eq!(stdout, format!("{}\n", expected.join("\n")));
}

#[test]
fn generated_normalizer_uses_platform_components() {
    let src = "use core.path as path\n\nfn run() { print(path.normalize(\"a/../b\")) }\n";
    let out = jet::compile(src).expect("path normalize fixture should compile");
    assert!(out.rust.contains("std::path::Component::Prefix"));
    assert!(out.rust.contains("std::path::Component::RootDir"));
    assert!(out.rust.contains("std::path::PathBuf::new()"));
    assert!(!out.rust.contains("s.split('/')"));
}
