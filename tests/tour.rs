//! Every ```jet block in docs/guide/tour.md must compile (M14, invariant I5).
//!
//! Snippets are written to a temp file and checked with `jet::check_with_path`.

use std::fs;
use std::path::PathBuf;

fn extract_jet_blocks(markdown: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut start_line = 0;
    let mut current = String::new();

    for (i, line) in markdown.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "```jet" {
            in_block = true;
            start_line = i + 1;
            current.clear();
            continue;
        }
        if in_block && trimmed == "```" {
            in_block = false;
            blocks.push((start_line, current.trim_end().to_string()));
            current.clear();
            continue;
        }
        if in_block {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    assert!(
        !in_block,
        "docs/guide/tour.md has an unclosed ```jet block starting near line {start_line}"
    );
    blocks
}

#[test]
fn tour_snippets_compile() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tour = root.join("docs/guide/tour.md");
    let src = fs::read_to_string(&tour).expect("docs/guide/tour.md");
    let blocks = extract_jet_blocks(&src);
    assert!(
        blocks.len() >= 12,
        "docs/guide/tour.md should have at least 12 jet blocks, found {}",
        blocks.len()
    );

    let tmp = std::env::temp_dir().join("jet_tour_snippets");
    fs::create_dir_all(&tmp).unwrap();

    for (idx, (line, code)) in blocks.into_iter().enumerate() {
        let path = tmp.join(format!("snippet_{idx:02}.jet"));
        fs::write(&path, &code).unwrap();
        let shown = format!("docs/guide/tour.md (block near line {line})");
        let diags = jet::check_with_path(path.to_str().unwrap());
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == jet::diag::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "tour snippet #{idx} near line {line} failed to compile:\n{}\n--- source ---\n{code}\n---",
            jet::render_diagnostics(&shown, &code, &diags)
        );
    }
}
