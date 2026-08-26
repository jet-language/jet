//! Shared enforcement for the ratified UI case law.

#[derive(Clone, Copy)]
pub(crate) enum CaseLawKind {
    Sentence,
    Title,
    Excluded,
}

pub(crate) struct CaseLawSource {
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) kind: CaseLawKind,
}

/// D-CASE-CHROME1=C / D-CASE-PROSE1=A: collect the strings from the live
/// producers. Renderers are not alternate homes: the CLI registry feeds both
/// help surfaces, diagnostic rows feed explain and reports, and the REPL
/// render helpers feed status, prompt, and interactive chrome.
pub(crate) fn assert_user_facing_case_law() {
    let sources = user_facing_case_sources();
    let violations = case_law_violations(&sources);
    assert!(
        violations.is_empty(),
        "ratified UI case-law violations (D-CASE-CHROME1/D-CASE-PROSE1):\n{}",
        violations.join("\n")
    );
}

pub(crate) fn case_law_violations(sources: &[CaseLawSource]) -> Vec<String> {
    sources
        .iter()
        .filter_map(|source| {
            let violation = match source.kind {
                CaseLawKind::Sentence => sentence_case_violation(&source.value),
                CaseLawKind::Title => title_case_violation(&source.value),
                CaseLawKind::Excluded => None,
            }?;
            Some(format!("{}: {violation}", source.name))
        })
        .collect()
}

pub(crate) fn user_facing_case_sources() -> Vec<CaseLawSource> {
    let mut sources = Vec::new();
    // Every user-facing surface reduces to text, so take `&str` directly.
    // `&dyn Display` cannot accept a `&str`: `str` is unsized, so the
    // unsizing coercion to a trait object does not apply.
    let mut add = |name: String, value: &str, kind: CaseLawKind| {
        sources.push(CaseLawSource {
            name,
            value: value.to_string(),
            kind,
        });
    };

    // Diagnostics.jet is the one diagnostic home. The typed registry is the
    // generated projection consumed by all report/help renderers.
    for (line_index, raw) in jet_foundation::Registry::DIAGNOSTIC_SOURCE
        .lines()
        .enumerate()
    {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.first().copied() != Some("diagnostic") {
            continue;
        }
        for (field, index) in [("meaning", 6), ("What", 7), ("Why", 8), ("Fix", 9)] {
            if let Some(value) = fields.get(index) {
                add(
                    format!("Diagnostics.jet:{} {} {}", line_index + 1, fields[1], field),
                    *value,
                    CaseLawKind::Sentence,
                );
            }
        }
    }

    // CLI registry is the source for `jet help`, `jet ?`, man, and
    // completions. Check each projection's user-readable fields, not its
    // generated formatting.
    for command in jet::CLI::COMMANDS {
        add(
            format!("CLI command {} summary", command.name),
            command.summary,
            CaseLawKind::Sentence,
        );
        for action in command.actions {
            add(
                format!("CLI {} {} summary", command.name, action.name),
                jet::CLI::action_help_summary(command.name, action),
                CaseLawKind::Sentence,
            );
        }
    }
    for group in jet::CLI::command_groups() {
        add(
            format!("CLI {} group label", group.name),
            &jet::CLI::command_group_label(group.name),
            CaseLawKind::Title,
        );
    }
    for flag in jet::CLI::FLAGS.iter() {
        add(
            format!("CLI {} help", flag.long),
            flag.help,
            CaseLawKind::Sentence,
        );
    }
    for line in jet::CLI::usage_page("1.0.0").lines() {
        let line = line.trim();
        if line == "Usage:" || line == "Flags:" {
            add(
                format!("CLI usage header {line}"),
                line.trim_end_matches(':'),
                CaseLawKind::Title,
            );
        } else if let Some(label) = line.strip_suffix(" Commands:") {
            add(
                format!("CLI usage group header {label}"),
                label,
                CaseLawKind::Title,
            );
        }
    }

    // Hybrid help owns a second presentation surface over the same registry.
    // Its index is the source read by both the static and interactive views.
    for category in jet::Help::CATEGORIES {
        add(
            format!("help category {category}"),
            *category,
            CaseLawKind::Title,
        );
    }
    for entry in jet::Help::build_index() {
        add(
            format!("help {} summary", entry.symbol.name),
            &entry.symbol.summary,
            CaseLawKind::Sentence,
        );
        for (flag, help) in entry.flags {
            add(
                format!("help {flag} description"),
                help,
                CaseLawKind::Sentence,
            );
        }
    }

    // REPL help/status/prompt/interactive text comes from its public render
    // seam. Split dynamic identifiers and key names before checking prose.
    let repl_help = jet::REPL::run_transcript(&[":help"], None);
    for (line_index, line) in repl_help.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line_index == 0 || line == "Interactive terminal only" {
            add(
                format!("REPL help heading {line}"),
                line,
                CaseLawKind::Title,
            );
        } else if let Some(tip) = line.strip_prefix("Tip: ") {
            add(
                format!("REPL help tip {tip}"),
                tip,
                CaseLawKind::Sentence,
            );
        } else if let Some((_, description)) = line.split_once("  ") {
            add(
                format!("REPL help line {line_index}"),
                description.trim(),
                CaseLawKind::Sentence,
            );
        }
    }

    let banner = jet::REPL::Render::render_banner("1.0.0", false);
    if let Some(text) = banner.strip_prefix("Jet 1.0.0 — ") {
        add(
            "REPL banner description".to_string(),
            text.split_once("  (").map_or(text, |(text, _)| text),
            CaseLawKind::Sentence,
        );
    }
    for (mode, hint) in [
        ("raw", jet::REPL::Render::render_discovery_hint(true, false)),
        ("cooked", jet::REPL::Render::render_discovery_hint(false, false)),
    ] {
        for part in hint.strip_prefix("Try: ").unwrap_or(&hint).split(" · ") {
            let text = part
                .split_once(' ')
                .map_or(part, |(_, description)| description);
            if !text.starts_with("interactive keys") {
                add(
                    format!("REPL {mode} discovery hint {text}"),
                    text,
                    // D-CASE-CHROME1=C: a one- or two-word hint in the key bar
                    // is a label, not a description, so it takes Title Case.
                    CaseLawKind::Title,
                );
            }
        }
    }
    let prompt = jet::REPL::Render::render_prompt(1, false);
    add(
        "REPL prompt".to_string(),
        prompt.trim(),
        CaseLawKind::Excluded,
    );
    // The fold marker and pin rail are composed lines: `⋯ 42 rows folded ·
    // [Row] · unfold ⏎`. What these extract are mid-line fragments and the
    // literal command names a user types (`unfold`, `unpin`), not standalone
    // chrome. The case law governs whole lines and labels, so capitalising a
    // fragment would read as `⋯ 42 Rows folded`, and capitalising a command
    // name would misreport what the user must type.
    let fold = jet::REPL::Render::render_fold_marker(42, "Row", false);
    add(
        "REPL fold status".to_string(),
        fold.strip_prefix("⋯ 42 ")
            .and_then(|text| text.split(" · ").next())
            .unwrap_or_default(),
        CaseLawKind::Excluded,
    );
    add(
        "REPL fold action".to_string(),
        fold.split(" · ")
            .nth(2)
            .unwrap_or_default()
            .split(' ')
            .next()
            .unwrap_or_default(),
        CaseLawKind::Excluded,
    );
    let pin = jet::REPL::Render::render_pin_rail("total: Int :: 15", 3, 62, false);
    if let Some(hint) = pin.lines().next().and_then(|line| line.split("turn 3").nth(1)) {
        add(
            "REPL pin status".to_string(),
            "turn",
            CaseLawKind::Excluded,
        );
        add(
            "REPL pin action".to_string(),
            hint.split('·')
                .nth(1)
                .unwrap_or_default()
                .trim()
                .split(' ')
                .next()
                .unwrap_or_default(),
            CaseLawKind::Excluded,
        );
    }
    // The `:turns` listing prints a machine-readable status token, and the
    // same spelling feeds the notebook kernel JSON and the wire protocol
    // (`ReplTurnStatus::Ok => "ok"`). It is parseable state, not chrome, so
    // the case law leaves it alone rather than break every consumer.
    for line in jet::REPL::run_transcript(&["1 + 2", ":turns"], None).lines() {
        if line.starts_with('#') {
            if let Some(status) = line.split_whitespace().nth(1) {
                add(
                    format!("REPL status {status}"),
                    status,
                    CaseLawKind::Excluded,
                );
            }
        }
    }

    sources
}

/// D-CASE-PROSE1=A governs the FIRST thing on the line and nothing after it.
/// A line opening with a quoted fragment, a `{placeholder}`, a flag, or an
/// identifier keeps that fragment's own case and is never force-capitalised —
/// the ratified sample reads ``\`--offline\` forbids network access``, with
/// `forbids` staying lowercase. So this deliberately does NOT reuse
/// `first_diagnostic_prose_token`, which skips code-ish tokens and walks on:
/// that would capitalise a word in the middle of the sentence.
pub(crate) fn sentence_case_violation(value: &str) -> Option<String> {
    let mut offset = 0;
    let ch = loop {
        let ch = value[offset..].chars().next()?;
        if ch.is_whitespace() || matches!(ch, '*' | '~' | '_') {
            offset += ch.len_utf8();
            continue;
        }
        break ch;
    };
    // Opens with code or a substitution: the line keeps whatever case it has.
    if matches!(ch, '`' | '"' | '\'' | '{') {
        return None;
    }
    let rest = &value[offset..];
    let end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '!'))
        .unwrap_or(rest.len());
    let token = &rest[..end];
    if token.is_empty() || diagnostic_token_keeps_case(token) {
        return None;
    }
    token
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
        .then(|| {
            format!("sentence prose starts with lowercase `{token}` at byte {offset} in {value:?}")
        })
}

pub(crate) fn title_case_violation(value: &str) -> Option<String> {
    // `<id>` is a metavariable telling the user what to substitute, not a
    // label, so it keeps the spelling the user must type.
    if value.starts_with('<') && value.ends_with('>') {
        return None;
    }
    let words: Vec<&str> = value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    for (index, word) in words.iter().enumerate() {
        let lower = word.to_ascii_lowercase();
        let minor = title_case_minor_word(&lower);
        let is_first_or_last = index == 0 || index + 1 == words.len();
        if minor && !is_first_or_last {
            if *word != lower {
                return Some(format!("minor word `{word}` must stay lowercase in {value:?}"));
            }
        } else if word
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase())
        {
            return Some(format!("word `{word}` must start uppercase in {value:?}"));
        }
    }
    None
}

/// D-CASE-CHROME1=C: the ratified law has one closed list of minor words.
/// Keep it here, beside the title-case checker, so every label uses the same
/// per-word rule instead of growing a call-site-specific exception list.
pub(crate) const TITLE_CASE_MINOR_WORDS: &[&str] = &[
    "a", "an", "the", "and", "but", "or", "nor", "for", "as", "at", "by", "in", "of",
    "on", "to", "via",
];

fn title_case_minor_word(word: &str) -> bool {
    TITLE_CASE_MINOR_WORDS.contains(&word)
}

// This module is included by more than one test binary and each uses a
// different subset; `diagnostics_format` calls this one, `diagnostic_snapshots`
// does not.
#[allow(dead_code)]
pub(crate) fn first_diagnostic_prose_token(input: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    while offset < input.len() {
        let rest = &input[offset..];
        let ch = rest.chars().next()?;
        if ch.is_whitespace() || matches!(ch, '*' | '~' | '_') {
            offset += ch.len_utf8();
            continue;
        }
        if matches!(ch, '`' | '"' | '\'') {
            let after = &rest[ch.len_utf8()..];
            if let Some(close) = after.find(ch) {
                offset += ch.len_utf8() + close + ch.len_utf8();
                continue;
            }
        }
        if ch == '{' {
            if let Some(close) = rest.find('}') {
                offset += close + 1;
                continue;
            }
        }
        let start = offset;
        let mut end = 0;
        while end < rest.len() {
            let value = rest[end..]
                .chars()
                .next()
                .expect("diagnostic token index is on a character boundary");
            if end > 0
                && (value.is_whitespace()
                    || matches!(value, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '!'))
            {
                break;
            }
            if value == '.' {
                let next = rest[end + value.len_utf8()..].chars().next();
                if next.is_none_or(|next| {
                    next.is_whitespace()
                        || matches!(next, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '!')
                }) && end > 0
                {
                    break;
                }
            }
            end += value.len_utf8();
        }
        let end = offset + end;
        let token = &input[start..end];
        if token.is_empty() {
            offset += ch.len_utf8();
            continue;
        }
        offset = end;
        if diagnostic_token_keeps_case(token) {
            continue;
        }
        return Some((start, end));
    }
    None
}

pub(crate) fn diagnostic_token_keeps_case(token: &str) -> bool {
    if jet_foundation::Syntax::JET_KEYWORD_LIST.contains(&token)
        || jet_foundation::Syntax::JET_TYPE_LIST.contains(&token)
        || jet_foundation::CoreModuleExports::is_core_type_name(token)
        || jet_foundation::Collections::is_reserved_type(token)
        || matches!(token.chars().next(), Some('-' | '#' | '@'))
        || token == "C"
        || matches!(
            token,
            // Proper nouns, then the binary names a user actually types. A
            // Fix line may BE a command (`jetpack env -- jet build`), and
            // capitalising its first word would misstate the command.
            "App" | "Hangar" | "Jet" | "Jetpack" | "Nix" | "Runtime" | "Store"
                | "jet" | "jetpack" | "cargo" | "nix" | "git" | "rustc"
        )
    {
        return true;
    }
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    let has_structural_case = token
        .chars()
        .any(|ch| matches!(ch, '_' | '/' | '\\' | '@' | '#' | '.'));
    let all_code = token
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'));
    let camel_case = token
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
        && token.chars().skip(1).any(|ch| ch.is_ascii_uppercase());
    has_digit || has_structural_case || all_code || camel_case || token.starts_with("C-")
}
