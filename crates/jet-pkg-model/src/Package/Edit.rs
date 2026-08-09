//! Comment-preserving in-place edits to the `deps: { … }` block of a
//! `package.jet` manifest (mirrors the old jet.toml `add_dependency`/`remove`).

/// Render a compiler-side `DepSpec` back into `package.jet` dep-value syntax.
fn render_dep_spec(name: &str, spec: &crate::Manifest::DepSpec) -> String {
    use crate::Manifest::{DepSpec, GitSelector};
    match spec {
        DepSpec::Registry(v) => format!("{name}#{v}"),
        DepSpec::Path { path } => path.clone(),
        DepSpec::Git { url, selector } => {
            let sel = match selector {
                GitSelector::Tag(t) => format!("tag: \"{t}\""),
                GitSelector::Branch(b) => format!("branch: \"{b}\""),
                GitSelector::Rev(r) => format!("rev: \"{r}\""),
            };
            format!("{{ git: \"{url}\", {sel} }}")
        }
    }
}

/// Insert or update a dependency in the `deps: { … }` block, preserving
/// comments and existing entries. Creates the block if absent. Mirrors the
/// old jet.toml `add_dependency`, but for Jet-syntax `deps:` blocks (U10).
pub fn add_dep(raw: &str, name: &str, spec: &crate::Manifest::DepSpec) -> String {
    let line = format!("    {name}: {},", render_dep_spec(name, spec));
    insert_or_replace_in_block(raw, "deps", name, &line)
}

/// Remove a dependency from `deps: { … }`, preserving comments.
pub fn remove_dep(raw: &str, name: &str) -> String {
    remove_from_block(raw, "deps", name)
}

/// The `[start, end)` line range of `key: { … }`'s body (the lines strictly
/// between the opening and matching closing brace), tracking brace depth so
/// nested structs (e.g. an inline git dep) don't confuse the boundary.
fn block_line_range(lines: &[String], key: &str) -> Option<(usize, usize)> {
    let header = format!("{key}:");
    let mut start: Option<usize> = None;
    let mut depth = 0i32;
    for (i, line) in lines.iter().enumerate() {
        if start.is_none() {
            let trimmed = line.trim_start();
            let after_header = trimmed
                .starts_with(&header)
                .then(|| trimmed[header.len()..].trim_start())
                .map(|rest| rest.strip_prefix('.').unwrap_or(rest).trim_start());
            if after_header.is_some_and(|rest| rest.starts_with('{')) {
                depth = brace_delta(line);
                start = Some(i + 1);
                if depth <= 0 {
                    return Some((i + 1, i + 1));
                }
            }
            continue;
        }
        depth += brace_delta(line);
        if depth <= 0 {
            return Some((start?, i));
        }
    }
    None
}

fn brace_delta(line: &str) -> i32 {
    line.matches('{').count() as i32 - line.matches('}').count() as i32
}

/// Insert or replace a `name: …` entry inside the `key: { … }` block,
/// creating the block (appended at end of file) if it doesn't exist yet.
fn insert_or_replace_in_block(raw: &str, key: &str, name: &str, new_line: &str) -> String {
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let mut out = lines.clone();

    if let Some((start, end)) = block_line_range(&lines, key) {
        let mut existing: Option<usize> = None;
        for i in start..end {
            let trimmed = lines[i].trim_start();
            if let Some((k, _)) = trimmed.split_once(':') {
                if k.trim() == name {
                    existing = Some(i);
                    break;
                }
            }
        }
        if let Some(i) = existing {
            out[i] = new_line.to_string();
        } else {
            out.insert(end, new_line.to_string());
        }
    } else {
        if !raw.is_empty() && !raw.ends_with('\n') {
            out.push(String::new());
        }
        out.push(String::new());
        out.push(format!("{key}: .{{"));
        out.push(new_line.to_string());
        out.push("}".to_string());
    }

    let mut result = out.join("\n");
    if raw.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Remove a `name: …` entry from the `key: { … }` block, preserving comments
/// and every other entry.
fn remove_from_block(raw: &str, key: &str, name: &str) -> String {
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let Some((start, end)) = block_line_range(&lines, key) else {
        return raw.to_string();
    };
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i >= start && i < end {
            let trimmed = line.trim_start();
            if let Some((k, _)) = trimmed.split_once(':') {
                if k.trim() == name {
                    continue;
                }
            }
        }
        out.push(line.clone());
    }
    let mut result = out.join("\n");
    if raw.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::block_line_range;

    #[test]
    fn block_line_range_checks_started_state_for_hostile_nesting() {
        let nested = [
            "name: \"x\"",
            "version: \"1\"",
            "deps: .{",
            "    git_dep: { git: \"https://example.test/repo\", tag: \"v1\" },",
            "    nested: {",
            "        inner: { value: \"kept\" },",
            "    },",
            "}",
            "outputs: .{ x: .Library.{} }",
        ]
        .map(str::to_string);
        assert_eq!(block_line_range(&nested, "deps"), Some((3, 7)));

        let truncated = [
            "deps: .{",
            "    git_dep: { git: \"https://example.test/repo\", tag: \"v1\" },",
            "    nested: {",
            "        inner: { value: \"unterminated\" },",
        ]
        .map(str::to_string);
        assert_eq!(block_line_range(&truncated, "deps"), None);
    }
}
