/// Shared `.gitignore` parsing and matching for filesystem walks.
///
/// A matcher is an immutable root-to-directory rule stack. Each directory adds
/// its own ignore file after inherited rules, so the last matching rule wins
/// while child files naturally have precedence over parent files.
#[derive(Clone, Debug)]
struct JetFsIgnoreRule {
    base: String,
    pattern: String,
    negated: bool,
    directory_only: bool,
    anchored: bool,
    has_separator: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct JetFsIgnoreMatcher {
    root: std::path::PathBuf,
    rules: std::sync::Arc<Vec<JetFsIgnoreRule>>,
}

impl JetFsIgnoreMatcher {
    pub(crate) fn new(root: &std::path::Path) -> Self {
        Self {
            root: root.to_path_buf(),
            rules: std::sync::Arc::new(Vec::new()),
        }
    }

    pub(crate) fn with_directory_rules(
        &self,
        directory: &std::path::Path,
        filename: &str,
    ) -> std::io::Result<Self> {
        let path = directory.join(filename);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(self.clone()),
            Err(error) => return Err(error),
        };
        // Do not follow a symlink supplied as an ignore file. A symlink is not
        // a trusted rule source and following it would violate walk's no-follow
        // policy even though the entry itself is never traversed.
        if !metadata.file_type().is_file() {
            return Ok(self.clone());
        }
        let bytes = std::fs::read(&path)?;
        let text = String::from_utf8_lossy(&bytes);
        let base = directory
            .strip_prefix(&self.root)
            .unwrap_or(directory)
            .to_string_lossy()
            .replace('\\', "/");
        let mut rules = Vec::with_capacity(self.rules.len() + text.lines().count());
        rules.extend(self.rules.iter().cloned());
        for line in text.lines() {
            if let Some(rule) = JetFsIgnoreRule::parse(line, &base) {
                rules.push(rule);
            }
        }
        Ok(Self {
            root: self.root.clone(),
            rules: std::sync::Arc::new(rules),
        })
    }

    pub(crate) fn is_ignored(&self, relative: &str, is_dir: bool) -> bool {
        let relative = relative.replace('\\', "/");
        let path_parts = split_components(&relative);
        if path_parts.is_empty() {
            return false;
        }
        // A negation cannot re-include a path below an excluded parent
        // directory. Evaluate each parent as its own directory candidate so
        // `foo/` blocks `foo/bar`, while a later `!foo/` can reopen the branch.
        for prefix_len in 1..path_parts.len() {
            let prefix = path_parts[..prefix_len].join("/");
            let mut ignored = false;
            for rule in self.rules.iter() {
                if rule.matches(&prefix, true) {
                    ignored = !rule.negated;
                }
            }
            if ignored {
                return true;
            }
        }
        let mut ignored = false;
        for rule in self.rules.iter() {
            if rule.matches(&relative, is_dir) {
                // Gitignore precedence is ordered: a later negation or ignore
                // replaces the earlier answer instead of accumulating state.
                ignored = !rule.negated;
            }
        }
        ignored
    }
}

impl JetFsIgnoreRule {
    fn parse(line: &str, base: &str) -> Option<Self> {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line = trim_unescaped_trailing_spaces(line);
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let mut pattern = line.to_string();
        let negated = pattern.starts_with('!');
        if negated {
            pattern.remove(0);
        }
        if pattern.is_empty() {
            return None;
        }
        let directory_only = ends_with_unescaped(&pattern, '/');
        if directory_only {
            pattern.pop();
        }
        let anchored = pattern.starts_with('/');
        if anchored {
            pattern.remove(0);
        }
        if pattern.is_empty() {
            return None;
        }
        let has_separator = contains_unescaped(&pattern, '/');
        Some(Self {
            base: base.trim_matches('/').to_string(),
            pattern,
            negated,
            directory_only,
            anchored,
            has_separator,
        })
    }

    fn matches(&self, relative: &str, is_dir: bool) -> bool {
        let Some(local) = relative_under_base(relative, &self.base) else {
            return false;
        };
        let path_parts = split_components(local);
        let Some(last) = path_parts.last() else {
            return false;
        };
        if self.has_separator || self.anchored {
            if self.directory_only && !is_dir {
                return false;
            }
            let pattern_parts = split_components(&self.pattern);
            return glob_path_matches(&pattern_parts, &path_parts);
        }
        if self.directory_only && !is_dir {
            return false;
        }
        glob_component_matches(&self.pattern, last)
    }
}

fn relative_under_base<'a>(relative: &'a str, base: &str) -> Option<&'a str> {
    if base.is_empty() {
        return Some(relative);
    }
    if relative == base {
        return Some("");
    }
    relative.strip_prefix(base).and_then(|rest| rest.strip_prefix('/'))
}

fn split_components(path: &str) -> Vec<&str> {
    path.split('/').filter(|part| !part.is_empty()).collect()
}

fn trim_unescaped_trailing_spaces(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut end = chars.len();
    while end > 0 && chars[end - 1] == ' ' {
        let mut backslashes = 0;
        let mut index = end - 1;
        while index > 0 && chars[index - 1] == '\\' {
            backslashes += 1;
            index -= 1;
        }
        if backslashes % 2 == 0 {
            end -= 1;
        } else {
            break;
        }
    }
    chars[..end].iter().collect()
}

fn ends_with_unescaped(value: &str, wanted: char) -> bool {
    let chars: Vec<char> = value.chars().collect();
    let Some(last) = chars.last() else {
        return false;
    };
    if *last != wanted {
        return false;
    }
    let mut backslashes = 0;
    let mut index = chars.len() - 1;
    while index > 0 && chars[index - 1] == '\\' {
        backslashes += 1;
        index -= 1;
    }
    backslashes % 2 == 0
}

fn contains_unescaped(value: &str, wanted: char) -> bool {
    let chars: Vec<char> = value.chars().collect();
    let mut escaped = false;
    for ch in chars {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == wanted {
            return true;
        }
    }
    false
}

fn glob_path_matches(pattern: &[&str], path: &[&str]) -> bool {
    // A trailing `dir/**` names the contents of `dir`, not the directory
    // itself. Other `**` positions still accept zero components. Checking the
    // whole prefix also handles leading globstars such as `**/cache/**`.
    if pattern.len() > 1
        && pattern.last() == Some(&"**")
        && glob_path_matches(&pattern[..pattern.len() - 1], path)
    {
        return false;
    }
    let mut memo = vec![vec![None; path.len() + 1]; pattern.len() + 1];
    fn visit(
        pattern: &[&str],
        path: &[&str],
        pattern_index: usize,
        path_index: usize,
        memo: &mut [Vec<Option<bool>>],
    ) -> bool {
        if let Some(result) = memo[pattern_index][path_index] {
            return result;
        }
        let result = if pattern_index == pattern.len() {
            path_index == path.len()
        } else if pattern[pattern_index] == "**" {
            visit(pattern, path, pattern_index + 1, path_index, memo)
                || (path_index < path.len()
                    && visit(pattern, path, pattern_index, path_index + 1, memo))
        } else {
            path_index < path.len()
                && glob_component_matches(pattern[pattern_index], path[path_index])
                && visit(pattern, path, pattern_index + 1, path_index + 1, memo)
        };
        memo[pattern_index][path_index] = Some(result);
        result
    }
    visit(pattern, path, 0, 0, &mut memo)
}

fn glob_component_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let mut memo = vec![vec![None; text.len() + 1]; pattern.len() + 1];
    fn visit(
        pattern: &[char],
        text: &[char],
        pattern_index: usize,
        text_index: usize,
        memo: &mut [Vec<Option<bool>>],
    ) -> bool {
        if let Some(result) = memo[pattern_index][text_index] {
            return result;
        }
        let result = if pattern_index == pattern.len() {
            text_index == text.len()
        } else {
            match pattern[pattern_index] {
                '*' => {
                    visit(pattern, text, pattern_index + 1, text_index, memo)
                        || (text_index < text.len()
                            && visit(pattern, text, pattern_index, text_index + 1, memo))
                }
                '?' => {
                    text_index < text.len()
                        && visit(pattern, text, pattern_index + 1, text_index + 1, memo)
                }
                '\\' if pattern_index + 1 < pattern.len() => {
                    text_index < text.len()
                        && pattern[pattern_index + 1] == text[text_index]
                        && visit(pattern, text, pattern_index + 2, text_index + 1, memo)
                }
                '\\' => {
                    text_index < text.len()
                        && text[text_index] == '\\'
                        && visit(pattern, text, pattern_index + 1, text_index + 1, memo)
                }
                '[' => {
                    if let Some((matched, next)) = bracket_match(pattern, pattern_index, text.get(text_index)) {
                        matched && visit(pattern, text, next, text_index + 1, memo)
                    } else {
                        text_index < text.len()
                            && text[text_index] == '['
                            && visit(pattern, text, pattern_index + 1, text_index + 1, memo)
                    }
                }
                wanted => {
                    text_index < text.len()
                        && text[text_index] == wanted
                        && visit(pattern, text, pattern_index + 1, text_index + 1, memo)
                }
            }
        };
        memo[pattern_index][text_index] = Some(result);
        result
    }
    visit(&pattern, &text, 0, 0, &mut memo)
}

fn bracket_match(
    pattern: &[char],
    start: usize,
    text: Option<&char>,
) -> Option<(bool, usize)> {
    let mut index = start + 1;
    let negated = pattern.get(index).is_some_and(|ch| *ch == '!' || *ch == '^');
    if negated {
        index += 1;
    }
    let mut matched = false;
    let mut has_item = false;
    while index < pattern.len() && pattern[index] != ']' {
        let first = if pattern[index] == '\\' && index + 1 < pattern.len() {
            index += 1;
            pattern[index]
        } else {
            pattern[index]
        };
        index += 1;
        has_item = true;
        if index + 1 < pattern.len() && pattern[index] == '-' && pattern[index + 1] != ']' {
            index += 1;
            let last = if pattern[index] == '\\' && index + 1 < pattern.len() {
                index += 1;
                pattern[index]
            } else {
                pattern[index]
            };
            index += 1;
            if let Some(text) = text {
                matched |= first <= *text && *text <= last;
            }
        } else if text.is_some_and(|text| first == *text) {
            matched = true;
        }
    }
    if index >= pattern.len() || !has_item {
        return None;
    }
    Some((if negated { !matched } else { matched }, index + 1))
}
