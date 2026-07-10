//! Generate docs/reference/errors/E####.md from ui snapshots (M14 workstream 3).
//!
//! Run: `UPDATE_DOCS=1 cargo test --test gen_errors gen_error_pages -- --nocapture`

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct ParsedDiag {
    code: String,
    what: String,
    why: String,
    fix: String,
    #[allow(dead_code)] // parsed for completeness; assertions use code/what/why/fix
    location: String,
}

fn parse_stderr(stderr: &str) -> Vec<ParsedDiag> {
    let mut out = Vec::new();
    let lines: Vec<&str> = stderr.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("Error [") {
            if let Some((code, what)) = rest.split_once("]: ") {
                let mut location = String::new();
                let mut why = String::new();
                let mut fix = String::new();
                i += 1;
                if i < lines.len() && lines[i].starts_with("  --> ") {
                    location = lines[i].trim_start_matches("  --> ").to_string();
                    i += 1;
                }
                while i < lines.len() {
                    if lines[i].starts_with(" Why: ") {
                        why = lines[i].trim_start_matches(" Why: ").to_string();
                    } else if lines[i].starts_with(" Fix: ") {
                        fix = lines[i].trim_start_matches(" Fix: ").to_string();
                    } else if lines[i].starts_with("Error [") {
                        break;
                    }
                    i += 1;
                }
                out.push(ParsedDiag {
                    code: code.to_string(),
                    what: what.to_string(),
                    why,
                    fix,
                    location,
                });
                continue;
            }
        }
        i += 1;
    }
    out
}

fn jet_stem(path: &Path) -> String {
    if path.file_name().map(|n| n == "stderr").unwrap_or(false) {
        path.parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "main".to_string())
    } else {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

fn collect_ui_cases(ui_dir: &Path) -> BTreeMap<String, (PathBuf, Vec<ParsedDiag>)> {
    let mut by_code: BTreeMap<String, (PathBuf, Vec<ParsedDiag>)> = BTreeMap::new();
    let ext = jet::Syntax::FILE_EXT;

    fn walk(
        dir: &Path,
        ui_dir: &Path,
        ext: &str,
        by_code: &mut BTreeMap<String, (PathBuf, Vec<ParsedDiag>)>,
    ) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, ui_dir, ext, by_code);
                continue;
            }
            if path.extension().and_then(|x| x.to_str()) != Some("stderr") {
                continue;
            }
            let stderr = fs::read_to_string(&path).unwrap();
            let diags = parse_stderr(&stderr);
            let stem = jet_stem(&path);
            let jet_path = if path.file_name().map(|n| n == "stderr").unwrap_or(false) {
                path.parent().unwrap().join(format!("main.{ext}"))
            } else {
                path.with_extension(ext)
            };
            let rel_jet = jet_path
                .strip_prefix(ui_dir.parent().unwrap())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            for d in diags {
                by_code
                    .entry(d.code.clone())
                    .or_insert_with(|| (PathBuf::from(&rel_jet), Vec::new()))
                    .1
                    .push(d);
                let _ = stem;
            }
        }
    }

    walk(ui_dir, ui_dir, ext, &mut by_code);
    by_code
}

fn render_page(code: &str, jet_rel: &str, diag: &ParsedDiag, has_fixed: bool) -> String {
    let title = format!("{code}: {}", diag.what);
    let fixed = if has_fixed {
        let fixed_rel = jet_rel.replace(".jet", ".fixed.jet");
        format!("\n## Fixed program\n\nSee [`{fixed_rel}`](../../../{fixed_rel}).\n")
    } else {
        String::new()
    };
    format!(
        "# {title}\n\n\
         **Code:** `{code}`\n\n\
         ## What\n\n\
         {what}\n\n\
         ## Why\n\n\
         {why}\n\n\
         ## Fix\n\n\
         {fix}\n\n\
         ## Example\n\n\
         Failing program: [`{jet_rel}`](../../../{jet_rel})\n\
         {fixed}\n\
         ---\n\n\
         [Back to diagnostics registry](../../spec/diagnostics.md)\n",
        title = title,
        code = code,
        what = diag.what,
        why = diag.why,
        fix = diag.fix,
        jet_rel = jet_rel,
        fixed = fixed,
    )
}

/// Representative error codes always generated (M14 subset).
const REPRESENTATIVE: &[&str] = &[
    "E0101", "E0102", "E0103", "E0104", "E0105", "E0107", "E0108", "E0109", "E0110", "E0111",
    "E0119",
    "E0120",
    "E-WEB-ABI-TYPE",
    "E-WEB-CROSS-PARTITION",
    "E-WEB-TARGET-BROWSER",
];

/// Canonical ui fixture per code (walk order is nondeterministic).
const PREFERRED_UI: &[(&str, &str)] = &[
    ("E0101", "tests/ui/no_main.jet"),
    ("E0102", "tests/ui/unknown_function.jet"),
    ("E0103", "tests/ui/print_needs_one.jet"),
    ("E0104", "tests/ui/wrong_arg_count.jet"),
    ("E0105", "tests/ui/defined_twice.jet"),
    ("E0107", "tests/ui/unknown_name.jet"),
    ("E0108", "tests/ui/binding_type_mismatch.jet"),
    ("E0109", "tests/ui/mixed_numbers.jet"),
    ("E0110", "tests/ui/cond_not_bool.jet"),
    ("E0111", "tests/ui/assign_to_val.jet"),
    ("E0119", "tests/ui/unknown_type.jet"),
    ("E0120", "tests/ui/return_borrowed_param.jet"),
    ("E-WEB-ABI-TYPE", "tests/ui/web_abi_type.jet"),
    ("E-WEB-CROSS-PARTITION", "tests/ui/web_cross_partition.jet"),
    ("E-WEB-TARGET-BROWSER", "tests/ui/web_target_browser.jet"),
];

fn load_preferred_diag(root: &Path, code: &str, jet_rel: &str) -> ParsedDiag {
    let stderr_path = if jet_rel.ends_with("/main.jet") {
        PathBuf::from(jet_rel).parent().unwrap().join("stderr")
    } else {
        PathBuf::from(jet_rel).with_extension("stderr")
    };
    let stderr = fs::read_to_string(root.join(&stderr_path))
        .unwrap_or_else(|_| panic!("missing stderr for {jet_rel} at {}", stderr_path.display()));
    parse_stderr(&stderr)
        .into_iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("no {code} in {}", stderr_path.display()))
}

#[test]
fn gen_error_pages() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ui_dir = root.join("tests/ui");
    let out_dir = root.join("docs/reference/errors");
    let _cases = collect_ui_cases(&ui_dir);

    let mut generated = 0usize;
    for code in REPRESENTATIVE {
        let jet_rel = PREFERRED_UI
            .iter()
            .find(|(c, _)| c == code)
            .map(|(_, p)| *p)
            .unwrap_or_else(|| panic!("missing PREFERRED_UI entry for {code}"));
        let diag = load_preferred_diag(&root, code, jet_rel);
        let jet_path = root.join(jet_rel);
        let fixed_path = jet_path.with_file_name(
            jet_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .replace(".jet", ".fixed.jet"),
        );
        let page = render_page(code, jet_rel, &diag, fixed_path.is_file());
        let out_path = out_dir.join(format!("{code}.md"));
        if std::env::var("UPDATE_DOCS").is_ok() {
            fs::create_dir_all(&out_dir).unwrap();
            fs::write(&out_path, &page).unwrap();
            eprintln!("wrote {}", out_path.display());
        }
        generated += 1;
        if out_path.is_file() {
            let on_disk = fs::read_to_string(&out_path).unwrap();
            assert_eq!(
                on_disk, page,
                "{code}.md is stale — run: nix develop -c env UPDATE_DOCS=1 cargo test --test gen_errors gen_error_pages -- --nocapture"
            );
        }
    }

    assert!(
        generated >= 10,
        "expected at least 10 representative error pages"
    );
}
