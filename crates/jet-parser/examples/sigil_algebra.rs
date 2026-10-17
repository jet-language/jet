//! D-SIGIL-ALGEBRA1: apply only the parser's precedence-preserving E0398 edits.
//! Usage: cargo run -p jet-parser --example sigil_algebra -- [--apply] <tree>...
use std::{collections::BTreeMap, env, fs, path::{Path, PathBuf}};
use jet_parser::{Lexer, Parser};

fn files(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() { return Ok(()); }
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let path = entry?.path();
            if !matches!(path.file_name().and_then(|s| s.to_str()), Some(".git" | "target" | ".agent-worktrees" | "node_modules")) {
                files(&path, out)?;
            }
        }
    } else if matches!(path.extension().and_then(|s| s.to_str()), Some("jet" | "md")) {
        out.push(path.to_owned());
    }
    Ok(())
}

fn retired_fixture(path: &Path) -> bool {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    stem.contains("retired") || matches!(stem,
        "sigil_arithmetic_call_spans" | "sigil_bitand_operator_misuse" |
        "sigil_power_operator_misuse")
}

fn migrate(source: &mut String, tree: &str, counts: &mut BTreeMap<(String, String), usize>) {
    // Apply the innermost edit, then reparse: nested operands and compound
    // assignments must never be rewritten using stale byte offsets.
    loop {
        let (tokens, _) = Lexer::lex(source);
        let diagnostics = match Parser::parse_for_check_with_source(&tokens, source) {
            Ok((_, diagnostics)) => diagnostics,
            Err(diagnostics) => diagnostics,
        };
        let mut fixes: Vec<_> = diagnostics.into_iter()
            .filter(|d| d.code == "E0398")
            .filter_map(|d| d.edit.map(|edit| (d.span, edit)))
            .collect();
        fixes.sort_by_key(|(_, edit)| (edit.span.end - edit.span.start, edit.span.start));
        let Some((operator, edit)) = fixes.into_iter().next() else { break };
        let spelling = operator.and_then(|span| source.get(span.start..span.end)).unwrap_or("?").to_owned();
        *counts.entry((tree.to_owned(), spelling)).or_default() += 1;
        source.replace_range(edit.span.start..edit.span.end, &edit.new_text);
    }
}

fn markdown(source: &str, tree: &str, counts: &mut BTreeMap<(String, String), usize>) -> String {
    let mut output = String::new();
    let mut block = String::new();
    let mut in_jet = false;
    for line in source.split_inclusive('\n') {
        if !in_jet && line.trim() == "```jet" {
            in_jet = true;
            output.push_str(line);
        } else if in_jet && line.trim() == "```" {
            // Partial statement examples need a function body to reach the
            // expression parser. Full declarations already have their context.
            if block.contains("fn ") {
                migrate(&mut block, tree, counts);
                output.push_str(&block);
            } else {
                let mut wrapped = format!("fn migration_example() {{\n{block}}}\n");
                migrate(&mut wrapped, tree, counts);
                output.push_str(wrapped.strip_prefix("fn migration_example() {\n").unwrap().strip_suffix("}\n").unwrap());
            }
            block.clear();
            in_jet = false;
            output.push_str(line);
        } else if in_jet {
            block.push_str(line);
        } else {
            output.push_str(line);
        }
    }
    output.push_str(&block);
    output
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut apply = false;
    let mut snapshots = false;
    let mut paths = Vec::new();
    for arg in env::args().skip(1) {
        if arg == "--apply" { apply = true; }
        else if arg == "--snapshots" { snapshots = true; }
        else { files(Path::new(&arg), &mut paths)?; }
    }
    paths.sort();
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for path in paths {
        let before = fs::read_to_string(&path)?;
        if !snapshots && (retired_fixture(&path) || before.contains("retired-spelling-fixture:")) { continue; }
        if snapshots {
            let (tokens, _) = Lexer::lex(&before);
            let diagnostics = match Parser::parse_for_check_with_source(&tokens, &before) {
                Ok((_, diagnostics)) => diagnostics,
                Err(diagnostics) => diagnostics,
            };
            fs::write(path.with_extension("stderr"), jet_parser::Diagnostics::render_all(
                &path.to_string_lossy(), &before, &diagnostics,
            ))?;
            continue;
        }
        let tree = path.components().next().unwrap().as_os_str().to_string_lossy().into_owned();
        let source = if path.extension().and_then(|s| s.to_str()) == Some("md") {
            markdown(&before, &tree, &mut counts)
        } else {
            let mut source = before.clone();
            migrate(&mut source, &tree, &mut counts);
            source
        };
        if apply && source != before { fs::write(&path, source)?; }
    }
    println!("tree\toperator\tsites");
    for ((tree, operator), count) in counts { println!("{tree}\t{operator}\t{count}"); }
    Ok(())
}
