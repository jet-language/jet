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
        DepSpec::Foreign {
            language,
            reference,
        } => format!("{}@{reference:?}", language.root()),
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

/// Add one effect root to `authority.holds.allow`, preserving the manifest's
/// comments and unrelated formatting. This is the editor used by an
/// interactive project approval; callers must still reparse the returned
/// source before writing it.
pub fn add_authority_hold(raw: &str, effect: &str) -> String {
    let effect = effect.trim();
    if effect.is_empty() {
        return raw.to_string();
    }
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let Some(authority_line) = lines
        .iter()
        .position(|line| line.trim_start().starts_with("authority:"))
    else {
        return append_authority_hold(raw, effect);
    };
    let Some((authority_start, authority_end)) = block_line_range(&lines, "authority") else {
        return raw.to_string();
    };
    let authority_inline = inline_block_bounds(&lines[authority_line], "authority");
    let holds_line = if authority_inline.is_some() {
        Some(authority_line)
    } else {
        (authority_start..authority_end)
            .find(|index| lines[*index].trim_start().starts_with("holds:"))
    };

    let Some(holds_line) = holds_line else {
        let mut out = lines;
        if let Some((open, close)) = authority_inline {
            out[authority_line] = insert_inline_field(
                &out[authority_line],
                open,
                close,
                "holds: { allow: [",
                effect,
            );
        } else {
            out.insert(
                authority_end,
                format!(
                    "{}holds: {{ allow: [{}] }},",
                    body_indent(&out, authority_start, authority_end, &out[authority_line]),
                    effect
                ),
            );
        }
        return restore_newline(raw, out);
    };

    let mut out = lines;
    if let Some(updated) = add_to_inline_list(&out[holds_line], "allow", effect) {
        out[holds_line] = updated;
        return restore_newline(raw, out);
    }
    if let Some((open, close)) = inline_block_bounds(&out[holds_line], "holds") {
        out[holds_line] = insert_inline_field(
            &out[holds_line],
            open,
            close,
            "allow: [",
            effect,
        );
        return restore_newline(raw, out);
    }
    let Some((holds_start, holds_end)) = block_line_range_between(
        &out,
        "holds",
        authority_start,
        authority_end,
    ) else {
        return raw.to_string();
    };
    out.insert(
        holds_end,
        format!(
            "{}allow: [{}],",
            body_indent(&out, holds_start, holds_end, &out[holds_line]),
            effect
        ),
    );
    restore_newline(raw, out)
}

fn append_authority_hold(raw: &str, effect: &str) -> String {
    let mut out: Vec<String> = raw.lines().map(str::to_string).collect();
    if !raw.is_empty() && !raw.ends_with('\n') {
        out.push(String::new());
    }
    out.extend([
        "authority: .{".to_string(),
        format!("    holds: {{ allow: [{}] }},", effect),
        "}".to_string(),
    ]);
    restore_newline(raw, out)
}

fn restore_newline(raw: &str, out: Vec<String>) -> String {
    let mut result = out.join("\n");
    if raw.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn body_indent(lines: &[String], start: usize, end: usize, header: &str) -> String {
    lines
        .get(start..end)
        .and_then(|body| {
            body.iter()
                .find(|line| !line.trim().is_empty())
                .map(|line| leading_whitespace(line).to_string())
        })
        .unwrap_or_else(|| format!("{}    ", leading_whitespace(header)))
}

fn inline_block_bounds(line: &str, key: &str) -> Option<(usize, usize)> {
    let marker = format!("{key}:");
    let field = line.find(&marker)?;
    let open = line[field + marker.len()..]
        .find('{')?
        .saturating_add(field + marker.len());
    let close = line[open + 1..].rfind('}')?.saturating_add(open + 1);
    (close > open).then_some((open, close))
}

fn add_to_inline_list(line: &str, field: &str, effect: &str) -> Option<String> {
    let marker = format!("{field}:");
    let field_start = line.find(&marker)?;
    let open = line[field_start + marker.len()..]
        .find('[')?
        .saturating_add(field_start + marker.len());
    let close = line[open + 1..].find(']')?.saturating_add(open + 1);
    let body = &line[open + 1..close];
    if body.split(',').any(|entry| normalized_effect(entry) == effect) {
        return Some(line.to_string());
    }
    let addition = if body.trim().is_empty() {
        effect.to_string()
    } else {
        format!(", {effect}")
    };
    Some(format!("{}{}{}", &line[..close], addition, &line[close..]))
}

fn insert_inline_field(
    line: &str,
    open: usize,
    close: usize,
    field_prefix: &str,
    effect: &str,
) -> String {
    let body = &line[open + 1..close];
    let separator = if body.trim().is_empty() || body.trim_end().ends_with(',') {
        ""
    } else {
        ", "
    };
    format!(
        "{}{}{}{}]{}",
        &line[..close],
        separator,
        field_prefix,
        effect,
        &line[close..close],
        &line[close..]
    )
}

fn normalized_effect(value: &str) -> &str {
    value.trim().trim_matches('"').trim_start_matches('.')
}

/// The `[start, end)` line range of `key: { … }`'s body (the lines strictly
/// between the opening and matching closing brace), tracking brace depth so
/// nested structs (e.g. an inline git dep) don't confuse the boundary.
fn block_line_range(lines: &[String], key: &str) -> Option<(usize, usize)> {
    block_line_range_between(lines, key, 0, lines.len())
}

fn block_line_range_between(
    lines: &[String],
    key: &str,
    range_start: usize,
    range_end: usize,
) -> Option<(usize, usize)> {
    let header = format!("{key}:");
    let mut start: Option<usize> = None;
    let mut depth = 0i32;
    for i in range_start..range_end {
        let line = &lines[i];
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

    if let Some(i) = lines
        .iter()
        .position(|line| is_empty_inline_block(line, key))
    {
        out[i] = format!("{}{}: .{{", leading_whitespace(&lines[i]), key);
        out.insert(i + 1, new_line.to_string());
        out.insert(i + 2, "}".to_string());
    } else if let Some((start, end)) = block_line_range(&lines, key) {
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

fn is_empty_inline_block(line: &str, key: &str) -> bool {
    let trimmed = line.trim_start();
    let header = format!("{key}:");
    let Some(rest) = trimmed.strip_prefix(&header) else {
        return false;
    };
    let rest = rest
        .trim_start()
        .strip_prefix('.')
        .unwrap_or(rest.trim_start())
        .trim();
    rest == "{}"
}

fn leading_whitespace(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
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
