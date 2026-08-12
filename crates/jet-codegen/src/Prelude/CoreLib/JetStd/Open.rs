mod jet_std {
    // The one outcome carrier: from the flat Prelude under AOT, from the host
    // module when another tier includes this file.
    #[allow(unused_imports)]
    use super::*;
    // D-IOERROR-TREE1=A: one public context shape for every byte-stream error.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum IOOperation {
        Read,
        Write,
        Flush,
        Connect,
        Accept,
        Close,
        Resolve,
        Codec,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct IOContext {
        pub operation: IOOperation,
        pub resource: JetOutcome<String, JetAbsent>,
        pub os_code: JetOutcome<i64, JetAbsent>,
        pub cause: JetOutcome<String, JetAbsent>,
    }

    impl IOContext {
        // The constructor still takes Rust plumbing so every host call site reads
        // the same; the carrier starts here, once.
        pub fn new(operation: IOOperation, resource: Option<String>, os_code: Option<i64>, cause: Option<String>) -> Self {
            Self {
                operation,
                resource: jet_outcome_of(resource),
                os_code: jet_outcome_of(os_code),
                cause: jet_outcome_of(cause),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum IOError {
        InvalidInput(IOContext),
        NotFound(IOContext),
        PermissionDenied(IOContext),
        TimedOut(IOContext),
        Cancelled(IOContext),
        Closed(IOContext),
        Protocol(IOContext),
        Other(IOContext),
    }

    impl IOError {
        pub fn other(operation: IOOperation, resource: Option<String>, cause: impl ToString) -> Self {
            Self::Other(IOContext::new(operation, resource, None, Some(cause.to_string())))
        }
    }

    // D-ENV-MUTATE1=A: failures never carry input or host-backend text.
    #[derive(Clone, Debug, PartialEq)]
    pub enum EnvError {
        InvalidName,
        InvalidValue,
        NonUnicode,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct UTF8Error {
        pub message: String,
    }

    // D-TEXTWIDTH1=B: `TextWidth.{ ambiguous: .Wide, controls: .Reject }` —
    // the explicit-policy override for `core.text.display_width`. The
    // one-arg call uses the portable default (Narrow/Zero) directly and
    // never constructs this type.
    #[derive(Clone, Debug, PartialEq)]
    pub enum TextWidthAmbiguous {
        Narrow,
        Wide,
    }
    #[derive(Clone, Debug, PartialEq)]
    pub enum TextWidthControls {
        Zero,
        Reject,
    }
    #[derive(Clone, Debug, PartialEq)]
    pub struct TextWidth {
        pub ambiguous: TextWidthAmbiguous,
        pub controls: TextWidthControls,
    }
    #[derive(Clone, Debug, PartialEq)]
    pub struct TextError {
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct ProcessResult {
        pub code: i64,
        pub output: String,
        pub errors: String,
        pub success: bool,
        pub signal: Option<i64>,
        pub timed_out: bool,
    }

    #[derive(Clone, Debug)]
    pub struct JetURL {
        pub scheme: String,
        pub username: Option<String>,
        pub password: Option<String>,
        pub host: Option<String>,
        pub port: Option<i64>,
        pub path: String,
        pub query: Vec<(String, String)>,
        pub fragment: Option<String>,
        pub typed_host: Option<Vec<(String, bool)>>,
        pub typed_path: Option<Vec<(String, bool)>>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct JetMIME {
        pub top: String,
        pub sub: String,
        pub params: Vec<(String, String)>,
    }

    #[derive(Clone, Debug)]
    pub struct RegexFlags {
        pub case_insensitive: bool,
        pub multiline: bool,
        pub dotall: bool,
    }

    #[derive(Clone, Debug)]
    pub struct JetRegex {
        pattern: String,
        flags: RegexFlags,
        program: RegexProgram,
        group_names: Vec<Option<String>>,
        groups: usize,
    }

    #[derive(Clone, Debug)]
    pub struct JetRegexMatch {
        text: String,
        spans: Vec<Option<(usize, usize)>>,
        names: Vec<Option<String>>,
    }

    #[derive(Clone, Debug)]
    struct RegexProgram {
        insts: Vec<RegexInst>,
        start: usize,
    }

    #[derive(Clone, Debug)]
    enum RegexInst {
        Consume(RegexMatcher, Option<usize>),
        Save(usize, Option<usize>),
        Split(usize, Option<usize>),
        AssertStart(Option<usize>),
        AssertEnd(Option<usize>),
        Match,
    }

    #[derive(Clone, Debug)]
    enum RegexMatcher {
        Literal(char),
        Any,
        Class(RegexClass),
    }

    #[derive(Clone, Debug)]
    struct RegexClass {
        negated: bool,
        items: Vec<RegexClassItem>,
    }

    #[derive(Clone, Debug)]
    enum RegexClassItem {
        Char(char),
        Range(char, char),
        Digit,
        Word,
        Space,
        UnicodeLetter,
        UnicodeNumber,
        UnicodeAlphabetic,
        UnicodeWhitespace,
    }

    #[derive(Clone, Debug)]
    enum RegexNode {
        Seq(Vec<RegexPiece>),
        Alt(Vec<RegexNode>),
    }

    #[derive(Clone, Debug)]
    struct RegexPiece {
        atom: RegexAtom,
        quant: RegexQuant,
    }

    #[derive(Clone, Debug)]
    enum RegexAtom {
        Literal(char),
        Any,
        Class(RegexClass),
        Group(usize, Box<RegexNode>),
        Start,
        End,
    }

    #[derive(Clone, Debug)]
    enum RegexQuant {
        One,
        ZeroOrMore,
        OneOrMore,
        ZeroOrOne,
        Range { min: usize, max: Option<usize> },
    }

    #[derive(Clone, Copy)]
    enum RegexPatch {
        Next(usize),
        SplitB(usize),
    }

    struct RegexFrag {
        start: usize,
        outs: Vec<RegexPatch>,
    }

    #[derive(Clone)]
    struct RegexThread {
        pc: usize,
        caps: Vec<Option<usize>>,
    }

    impl Default for RegexFlags {
        fn default() -> Self {
            Self {
                case_insensitive: false,
                multiline: false,
                dotall: false,
            }
        }
    }

    impl crate::JetShow for RegexFlags {
        fn jet_show(&self) -> String {
            let mut s = String::new();
            if self.case_insensitive {
                s.push('i');
            }
            if self.multiline {
                s.push('m');
            }
            if self.dotall {
                s.push('s');
            }
            format!("RegexFlags({})", s)
        }
    }

    impl crate::JetShow for JetRegex {
        fn jet_show(&self) -> String {
            format!("Regex({})", self.pattern)
        }
    }

    impl crate::JetShow for JetRegexMatch {
        fn jet_show(&self) -> String {
            self.group(0).unwrap_or_default()
        }
    }

    impl JetRegexMatch {
        pub fn group(&self, n: i64) -> JetOutcome<String, JetAbsent> {
            let Ok(n) = usize::try_from(n) else { return Err(JetAbsent) };
            let Some((start, end)) = self.spans.get(n).copied().flatten() else { return Err(JetAbsent) };
            Ok(self.text[start..end].to_string())
        }

        pub fn name(&self, name: &str) -> JetOutcome<String, JetAbsent> {
            let Some(idx) = self
                .names
                .iter()
                .position(|n| n.as_deref() == Some(name))
            else {
                return Err(JetAbsent);
            };
            self.group(idx as i64)
        }

        pub fn start(&self) -> i64 {
            self.group_start(0).unwrap_or(-1)
        }

        pub fn end(&self) -> i64 {
            self.group_end(0).unwrap_or(-1)
        }

        pub fn group_start(&self, n: i64) -> JetOutcome<i64, JetAbsent> {
            let Ok(n) = usize::try_from(n) else { return Err(JetAbsent) };
            jet_outcome_of(
                self.spans
                    .get(n)
                    .copied()
                    .flatten()
                    .map(|(start, _)| start as i64),
            )
        }

        pub fn group_end(&self, n: i64) -> JetOutcome<i64, JetAbsent> {
            let Ok(n) = usize::try_from(n) else { return Err(JetAbsent) };
            jet_outcome_of(
                self.spans
                    .get(n)
                    .copied()
                    .flatten()
                    .map(|(_, end)| end as i64),
            )
        }

        /// Named capture pairs as `[[name, value], …]` (unnamed groups omitted).
        pub fn named_captures(&self) -> Vec<Vec<String>> {
            self.names
                .iter()
                .enumerate()
                .filter_map(|(i, n)| {
                    let name = n.as_ref()?.clone();
                    let value = self.group(i as i64).ok()?;
                    Some(vec![name, value])
                })
                .collect()
        }
    }

    impl JetRegex {
        pub fn pattern(&self) -> String {
            self.pattern.clone()
        }

        pub fn source(&self) -> String {
            self.pattern.clone()
        }

        pub fn flags(&self) -> String {
            let mut s = String::new();
            if self.flags.case_insensitive {
                s.push('i');
            }
            if self.flags.multiline {
                s.push('m');
            }
            if self.flags.dotall {
                s.push('s');
            }
            s
        }

        pub fn options(&self) -> String {
            self.flags()
        }

        pub fn names(&self) -> Vec<String> {
            self.group_names.iter().filter_map(|n| n.clone()).collect()
        }

        pub fn count(&self, text: &str) -> i64 {
            self.matches(text).len() as i64
        }

        pub fn is_match(&self, text: &str) -> bool {
            self.find_match(text).is_some()
        }

        pub fn match_value(&self, text: &str) -> JetOutcome<JetRegexMatch, JetAbsent> {
            jet_outcome_of(self.find_match(text))
        }

        pub fn find(&self, text: &str) -> JetOutcome<String, JetAbsent> {
            jet_outcome_of(self.find_match(text).and_then(|m| m.group(0).ok()))
        }

        pub fn find_all(&self, text: &str) -> Vec<String> {
            self.matches(text)
                .into_iter()
                .filter_map(|m| m.group(0).ok())
                .collect()
        }

        pub fn matches(&self, text: &str) -> Vec<JetRegexMatch> {
            let mut out = Vec::new();
            let mut pos = 0;
            while pos <= text.len() {
                let Some(m) = self.find_from(text, pos) else {
                    break;
                };
                let end = m.spans[0].map(|(_, e)| e).unwrap_or(pos);
                let start = m.spans[0].map(|(s, _)| s).unwrap_or(pos);
                out.push(m);
                pos = regex_next_search_pos(text, start, end);
            }
            out
        }

        pub fn replace(&self, text: &str, repl: &str) -> String {
            self.replace_impl(text, |m| expand_regex_replacement(repl, m), false)
        }

        pub fn replace_all(&self, text: &str, repl: &str) -> String {
            self.replace_impl(text, |m| expand_regex_replacement(repl, m), true)
        }

        pub fn replace_all_with<F>(&self, text: &str, f: F) -> String
        where
            F: Fn(JetRegexMatch) -> String,
        {
            self.replace_impl(text, |m| f(m.clone()), true)
        }

        pub fn split(&self, text: &str) -> Vec<String> {
            self.split_limit(text, 0)
        }

        pub fn split_limit(&self, text: &str, limit: i64) -> Vec<String> {
            let mut out = Vec::new();
            let mut pos = 0;
            let mut splits = 0i64;
            while pos <= text.len() {
                if limit > 0 && splits >= limit - 1 {
                    break;
                }
                let Some(m) = self.find_from(text, pos) else {
                    break;
                };
                let Some((start, end)) = m.spans[0] else {
                    break;
                };
                out.push(text[pos..start].to_string());
                pos = regex_next_search_pos(text, start, end);
                splits += 1;
            }
            out.push(text[pos.min(text.len())..].to_string());
            out
        }

        fn replace_impl<F>(&self, text: &str, repl: F, all: bool) -> String
        where
            F: Fn(&JetRegexMatch) -> String,
        {
            let mut out = String::new();
            let mut pos = 0;
            while pos <= text.len() {
                let Some(m) = self.find_from(text, pos) else {
                    break;
                };
                let Some((start, end)) = m.spans[0] else {
                    break;
                };
                out.push_str(&text[pos..start]);
                out.push_str(&repl(&m));
                pos = regex_next_search_pos(text, start, end);
                if !all {
                    break;
                }
            }
            out.push_str(&text[pos.min(text.len())..]);
            out
        }

        fn find_match(&self, text: &str) -> Option<JetRegexMatch> {
            self.find_from(text, 0)
        }

        fn find_from(&self, text: &str, start: usize) -> Option<JetRegexMatch> {
            for pos in regex_search_positions(text, start) {
                if let Some(spans) = self.run_at(text, pos) {
                    return Some(JetRegexMatch {
                        text: text.to_string(),
                        spans,
                        names: self.group_names.clone(),
                    });
                }
            }
            None
        }

        fn run_at(&self, text: &str, start: usize) -> Option<Vec<Option<(usize, usize)>>> {
            let mut current = Vec::new();
            let caps = vec![None; (self.groups + 1) * 2];
            self.add_thread(&mut current, self.program.start, caps, start, text);
            let mut pos = start;
            let mut last_match: Option<Vec<Option<(usize, usize)>>> = None;
            loop {
                if let Some(found) = current
                    .iter()
                    .find(|t| matches!(self.program.insts[t.pc], RegexInst::Match))
                {
                    let mut caps = found.caps.clone();
                    caps[0] = Some(start);
                    caps[1] = Some(pos);
                    last_match = Some(regex_slots_to_spans(&caps));
                }
                let Some(ch) = text[pos..].chars().next() else {
                    return last_match;
                };
                let next_pos = pos + ch.len_utf8();
                let mut next = Vec::new();
                for thread in current {
                    if let RegexInst::Consume(matcher, Some(target)) =
                        &self.program.insts[thread.pc]
                    {
                        if regex_matcher_matches(matcher, ch, &self.flags) {
                            self.add_thread(&mut next, *target, thread.caps, next_pos, text);
                        }
                    }
                }
                if next.is_empty() {
                    return last_match;
                }
                current = next;
                pos = next_pos;
            }
        }

        fn add_thread(
            &self,
            out: &mut Vec<RegexThread>,
            pc: usize,
            caps: Vec<Option<usize>>,
            pos: usize,
            text: &str,
        ) {
            let mut stack = vec![(pc, caps)];
            let mut seen = std::collections::BTreeSet::new();
            while let Some((pc, caps)) = stack.pop() {
                if !seen.insert(pc) {
                    continue;
                }
                match &self.program.insts[pc] {
                    RegexInst::Save(slot, Some(next)) => {
                        let mut next_caps = caps;
                        if *slot < next_caps.len() {
                            next_caps[*slot] = Some(pos);
                        }
                        stack.push((*next, next_caps));
                    }
                    RegexInst::Split(a, Some(b)) => {
                        stack.push((*b, caps.clone()));
                        stack.push((*a, caps));
                    }
                    RegexInst::AssertStart(Some(next)) => {
                        if pos == 0
                            || (self.flags.multiline
                                && text[..pos].chars().last().is_some_and(|ch| ch == '\n'))
                        {
                            stack.push((*next, caps));
                        }
                    }
                    RegexInst::AssertEnd(Some(next)) => {
                        if pos == text.len()
                            || (self.flags.multiline
                                && text[pos..].chars().next().is_some_and(|ch| ch == '\n'))
                        {
                            stack.push((*next, caps));
                        }
                    }
                    RegexInst::Match | RegexInst::Consume(_, _) => {
                        out.push(RegexThread { pc, caps });
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn jet_regex_flags(
        case_insensitive: bool,
        multiline: bool,
        dotall: bool,
    ) -> RegexFlags {
        RegexFlags {
            case_insensitive,
            multiline,
            dotall,
        }
    }

    /// Escape regex metacharacters so `text` matches literally.
    pub fn jet_regex_escape(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            if matches!(
                ch,
                '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|'
            ) {
                out.push('\\');
            }
            out.push(ch);
        }
        out
    }

    pub fn jet_regex_compile(pattern: &str) -> Result<JetRegex, String> {
        jet_regex_compile_with(pattern, &RegexFlags::default())
    }

    pub fn jet_regex_literal(pattern: &str) -> JetRegex {
        match jet_regex_compile(pattern) {
            Ok(regex) => regex,
            Err(error) => unreachable!("sema accepted an invalid Regex literal: {error}"),
        }
    }

    pub fn jet_regex_compile_with(pattern: &str, flags: &RegexFlags) -> Result<JetRegex, String> {
        super::jet_regex_syntax::validate(pattern).map_err(|error| {
            format!(
                "invalid regex `{pattern}` at position {}: {}",
                error.offset, error.reason
            )
        })?;
        let mut parser = RegexParser {
            chars: pattern.chars().collect(),
            pos: 0,
            groups: 0,
            names: vec![None],
        };
        let root = parser.parse_alt(None)?;
        if parser.pos != parser.chars.len() {
            return Err(format!("invalid regex `{}`: unexpected trailing input", pattern));
        }
        let mut compiler = RegexCompiler { insts: Vec::new() };
        let frag = compiler.compile_node(&root)?;
        let match_idx = compiler.push(RegexInst::Match);
        compiler.patch(&frag.outs, match_idx);
        Ok(JetRegex {
            pattern: pattern.to_string(),
            flags: flags.clone(),
            program: RegexProgram {
                insts: compiler.insts,
                start: frag.start,
            },
            group_names: parser.names,
            groups: parser.groups,
        })
    }

    pub fn jet_regex_is_match(pattern: &JetRegex, text: &str) -> bool {
        pattern.is_match(text)
    }

    // Core answers the emit boundary with Rust plumbing; `jet_outcome_of` at the
    // call site turns it into the carrier, exactly once.
    pub fn jet_regex_match(pattern: &JetRegex, text: &str) -> Option<JetRegexMatch> {
        pattern.match_value(text).ok()
    }

    pub fn jet_regex_find(pattern: &JetRegex, text: &str) -> Option<String> {
        pattern.find(text).ok()
    }

    pub fn jet_regex_find_all(pattern: &JetRegex, text: &str) -> Vec<String> {
        pattern.find_all(text)
    }

    pub fn jet_regex_matches(pattern: &JetRegex, text: &str) -> Vec<JetRegexMatch> {
        pattern.matches(text)
    }

    pub fn jet_regex_replace(pattern: &JetRegex, text: &str, repl: &str) -> String {
        pattern.replace(text, repl)
    }

    pub fn jet_regex_replace_all(pattern: &JetRegex, text: &str, repl: &str) -> String {
        pattern.replace_all(text, repl)
    }

    pub fn jet_regex_split(pattern: &JetRegex, text: &str) -> Vec<String> {
        pattern.split(text)
    }

    pub fn jet_regex_split_limit(
        pattern: &JetRegex,
        text: &str,
        limit: i64,
    ) -> Vec<String> {
        pattern.split_limit(text, limit)
    }

    struct RegexParser {
        chars: Vec<char>,
        pos: usize,
        groups: usize,
        names: Vec<Option<String>>,
    }

    impl RegexParser {
        fn parse_alt(&mut self, terminator: Option<char>) -> Result<RegexNode, String> {
            let mut arms = vec![self.parse_seq(terminator)?];
            while self.peek() == Some('|') {
                self.pos += 1;
                arms.push(self.parse_seq(terminator)?);
            }
            if let Some(end) = terminator {
                if self.peek() != Some(end) {
                    return Err(format!("invalid regex: missing `{end}`"));
                }
                self.pos += 1;
            }
            if arms.len() == 1 {
                Ok(arms.remove(0))
            } else {
                Ok(RegexNode::Alt(arms))
            }
        }

        fn parse_seq(&mut self, terminator: Option<char>) -> Result<RegexNode, String> {
            let mut pieces = Vec::new();
            while let Some(ch) = self.peek() {
                if Some(ch) == terminator || ch == '|' {
                    break;
                }
                let atom = self.parse_atom()?;
                let quant = self.parse_quant()?;
                pieces.push(RegexPiece { atom, quant });
            }
            Ok(RegexNode::Seq(pieces))
        }

        fn parse_atom(&mut self) -> Result<RegexAtom, String> {
            let Some(ch) = self.bump() else {
                return Err("invalid regex: empty atom".to_string());
            };
            match ch {
                '.' => Ok(RegexAtom::Any),
                '^' => Ok(RegexAtom::Start),
                '$' => Ok(RegexAtom::End),
                '(' => self.parse_group(),
                ')' => Err("invalid regex: unmatched `)`".to_string()),
                '[' => Ok(RegexAtom::Class(self.parse_class()?)),
                '\\' => self.parse_escape_atom(),
                '*' | '+' | '?' => Err(format!("invalid regex: `{ch}` has nothing to repeat")),
                '{' => Err("invalid regex: `{n}` has nothing to repeat".to_string()),
                other => Ok(RegexAtom::Literal(other)),
            }
        }

        fn parse_group(&mut self) -> Result<RegexAtom, String> {
            if self.peek() == Some('?') {
                self.pos += 1;
                return match self.bump() {
                    Some(':') => Ok(RegexAtom::Group(0, Box::new(self.parse_alt(Some(')'))?))),
                    Some('<') => {
                        let name = self.parse_group_name()?;
                        self.groups += 1;
                        let idx = self.groups;
                        self.names.push(Some(name));
                        Ok(RegexAtom::Group(idx, Box::new(self.parse_alt(Some(')'))?)))
                    }
                    Some('=') | Some('!') => {
                        Err("invalid regex: lookaround is not supported; use a linear rewrite".to_string())
                    }
                    Some(other) => Err(format!("invalid regex: unsupported group `?{other}`")),
                    None => Err("invalid regex: missing group kind after `?`".to_string()),
                };
            }
            self.groups += 1;
            let idx = self.groups;
            self.names.push(None);
            Ok(RegexAtom::Group(idx, Box::new(self.parse_alt(Some(')'))?)))
        }

        fn parse_group_name(&mut self) -> Result<String, String> {
            let start = self.pos;
            while self.peek().is_some_and(|ch| ch != '>') {
                self.pos += 1;
            }
            if self.bump() != Some('>') {
                return Err("invalid regex: missing `>` in named group".to_string());
            }
            let name: String = self.chars[start..self.pos - 1].iter().collect();
            if name.is_empty()
                || !name
                    .chars()
                    .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
            {
                return Err("invalid regex: named group needs an identifier".to_string());
            }
            Ok(name)
        }

        fn parse_quant(&mut self) -> Result<RegexQuant, String> {
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    Ok(RegexQuant::ZeroOrMore)
                }
                Some('+') => {
                    self.pos += 1;
                    Ok(RegexQuant::OneOrMore)
                }
                Some('?') => {
                    self.pos += 1;
                    Ok(RegexQuant::ZeroOrOne)
                }
                Some('{') => {
                    self.pos += 1;
                    let min = self.parse_number()?;
                    let max = if self.peek() == Some(',') {
                        self.pos += 1;
                        if self.peek() == Some('}') {
                            None
                        } else {
                            Some(self.parse_number()?)
                        }
                    } else {
                        Some(min)
                    };
                    if self.bump() != Some('}') {
                        return Err("invalid regex: missing `}` in quantifier".to_string());
                    }
                    if max.is_some_and(|m| m < min) {
                        return Err("invalid regex: quantifier max is below min".to_string());
                    }
                    Ok(RegexQuant::Range { min, max })
                }
                _ => Ok(RegexQuant::One),
            }
        }

        fn parse_class(&mut self) -> Result<RegexClass, String> {
            let negated = if self.peek() == Some('^') {
                self.pos += 1;
                true
            } else {
                false
            };
            let mut items = Vec::new();
            while let Some(ch) = self.peek() {
                if ch == ']' {
                    self.pos += 1;
                    return Ok(RegexClass { negated, items });
                }
                let first = self.parse_class_item()?;
                if self.peek() == Some('-') && self.peek_n(1) != Some(']') {
                    self.pos += 1;
                    let second = self.parse_class_char()?;
                    let RegexClassItem::Char(a) = first else {
                        return Err("invalid regex: class range needs literal endpoints".to_string());
                    };
                    items.push(RegexClassItem::Range(a, second));
                } else {
                    items.push(first);
                }
            }
            Err("invalid regex: missing `]`".to_string())
        }

        fn parse_class_item(&mut self) -> Result<RegexClassItem, String> {
            if self.peek() == Some('\\') {
                self.pos += 1;
                return match self.bump() {
                    Some('d') => Ok(RegexClassItem::Digit),
                    Some('w') => Ok(RegexClassItem::Word),
                    Some('s') => Ok(RegexClassItem::Space),
                    Some('p') => self.parse_unicode_class(false),
                    Some('P') => Err("invalid regex: negated Unicode classes belong outside `[]` today".to_string()),
                    Some(ch) => Ok(RegexClassItem::Char(regex_escaped_literal(ch))),
                    None => Err("invalid regex: missing escape".to_string()),
                };
            }
            self.parse_class_char().map(RegexClassItem::Char)
        }

        fn parse_class_char(&mut self) -> Result<char, String> {
            match self.bump() {
                Some(']') | None => Err("invalid regex: missing class character".to_string()),
                Some('\\') => self
                    .bump()
                    .map(regex_escaped_literal)
                    .ok_or_else(|| "invalid regex: missing escape".to_string()),
                Some(ch) => Ok(ch),
            }
        }

        fn parse_escape_atom(&mut self) -> Result<RegexAtom, String> {
            match self.bump() {
                Some('d') => Ok(RegexAtom::Class(RegexClass {
                    negated: false,
                    items: vec![RegexClassItem::Digit],
                })),
                Some('D') => Ok(RegexAtom::Class(RegexClass {
                    negated: true,
                    items: vec![RegexClassItem::Digit],
                })),
                Some('w') => Ok(RegexAtom::Class(RegexClass {
                    negated: false,
                    items: vec![RegexClassItem::Word],
                })),
                Some('W') => Ok(RegexAtom::Class(RegexClass {
                    negated: true,
                    items: vec![RegexClassItem::Word],
                })),
                Some('s') => Ok(RegexAtom::Class(RegexClass {
                    negated: false,
                    items: vec![RegexClassItem::Space],
                })),
                Some('S') => Ok(RegexAtom::Class(RegexClass {
                    negated: true,
                    items: vec![RegexClassItem::Space],
                })),
                Some('p') => Ok(RegexAtom::Class(RegexClass {
                    negated: false,
                    items: vec![self.parse_unicode_class(false)?],
                })),
                Some('P') => Ok(RegexAtom::Class(RegexClass {
                    negated: true,
                    items: vec![self.parse_unicode_class(true)?],
                })),
                Some(ch) if ch.is_ascii_digit() => {
                    Err("invalid regex: backreferences are not supported; captures stay linear".to_string())
                }
                Some(ch) => Ok(RegexAtom::Literal(regex_escaped_literal(ch))),
                None => Err("invalid regex: missing escape".to_string()),
            }
        }

        fn parse_unicode_class(&mut self, _negated: bool) -> Result<RegexClassItem, String> {
            if self.bump() != Some('{') {
                return Err("invalid regex: Unicode class needs `{...}`".to_string());
            }
            let start = self.pos;
            while self.peek().is_some_and(|ch| ch != '}') {
                self.pos += 1;
            }
            if self.bump() != Some('}') {
                return Err("invalid regex: missing `}` in Unicode class".to_string());
            }
            let name: String = self.chars[start..self.pos - 1].iter().collect();
            match name.as_str() {
                "L" | "Letter" => Ok(RegexClassItem::UnicodeLetter),
                "N" | "Number" => Ok(RegexClassItem::UnicodeNumber),
                "Alphabetic" => Ok(RegexClassItem::UnicodeAlphabetic),
                "White_Space" | "Whitespace" => Ok(RegexClassItem::UnicodeWhitespace),
                _ => Err(format!("invalid regex: unsupported Unicode class `{name}`")),
            }
        }

        fn parse_number(&mut self) -> Result<usize, String> {
            let start = self.pos;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == start {
                return Err("invalid regex: quantifier needs a number".to_string());
            }
            self.chars[start..self.pos]
                .iter()
                .collect::<String>()
                .parse::<usize>()
                .map_err(|_| "invalid regex: quantifier is too large".to_string())
        }

        fn peek(&self) -> Option<char> {
            self.chars.get(self.pos).copied()
        }

        fn peek_n(&self, n: usize) -> Option<char> {
            self.chars.get(self.pos + n).copied()
        }

        fn bump(&mut self) -> Option<char> {
            let ch = self.peek()?;
            self.pos += 1;
            Some(ch)
        }
    }

    struct RegexCompiler {
        insts: Vec<RegexInst>,
    }

    impl RegexCompiler {
        fn push(&mut self, inst: RegexInst) -> usize {
            let idx = self.insts.len();
            self.insts.push(inst);
            idx
        }

        fn patch(&mut self, patches: &[RegexPatch], target: usize) {
            for patch in patches {
                match *patch {
                    RegexPatch::Next(idx) => match &mut self.insts[idx] {
                        RegexInst::Consume(_, next)
                        | RegexInst::Save(_, next)
                        | RegexInst::AssertStart(next)
                        | RegexInst::AssertEnd(next) => *next = Some(target),
                        _ => {}
                    },
                    RegexPatch::SplitB(idx) => {
                        if let RegexInst::Split(_, b) = &mut self.insts[idx] {
                            *b = Some(target);
                        }
                    }
                }
            }
        }

        fn compile_node(&mut self, node: &RegexNode) -> Result<RegexFrag, String> {
            match node {
                RegexNode::Seq(pieces) => self.compile_seq(pieces),
                RegexNode::Alt(arms) => self.compile_alt(arms),
            }
        }

        fn compile_seq(&mut self, pieces: &[RegexPiece]) -> Result<RegexFrag, String> {
            let mut iter = pieces.iter();
            let Some(first) = iter.next() else {
                let idx = self.push(RegexInst::Save(usize::MAX, None));
                return Ok(RegexFrag {
                    start: idx,
                    outs: vec![RegexPatch::Next(idx)],
                });
            };
            let mut frag = self.compile_piece(first)?;
            for piece in iter {
                let next = self.compile_piece(piece)?;
                self.patch(&frag.outs, next.start);
                frag = RegexFrag {
                    start: frag.start,
                    outs: next.outs,
                };
            }
            Ok(frag)
        }

        fn compile_alt(&mut self, arms: &[RegexNode]) -> Result<RegexFrag, String> {
            if arms.is_empty() {
                return self.compile_seq(&[]);
            }
            let mut compiled = Vec::new();
            for arm in arms {
                compiled.push(self.compile_node(arm)?);
            }
            let mut frag = compiled.pop().unwrap();
            while let Some(left) = compiled.pop() {
                let split = self.push(RegexInst::Split(left.start, Some(frag.start)));
                let mut outs = left.outs;
                outs.extend(frag.outs);
                frag = RegexFrag { start: split, outs };
            }
            Ok(frag)
        }

        fn compile_piece(&mut self, piece: &RegexPiece) -> Result<RegexFrag, String> {
            match piece.quant {
                RegexQuant::One => self.compile_atom(&piece.atom),
                RegexQuant::ZeroOrMore => {
                    let atom = self.compile_atom(&piece.atom)?;
                    let split = self.push(RegexInst::Split(atom.start, None));
                    self.patch(&atom.outs, split);
                    Ok(RegexFrag {
                        start: split,
                        outs: vec![RegexPatch::SplitB(split)],
                    })
                }
                RegexQuant::OneOrMore => {
                    let atom = self.compile_atom(&piece.atom)?;
                    let split = self.push(RegexInst::Split(atom.start, None));
                    self.patch(&atom.outs, split);
                    Ok(RegexFrag {
                        start: atom.start,
                        outs: vec![RegexPatch::SplitB(split)],
                    })
                }
                RegexQuant::ZeroOrOne => {
                    let atom = self.compile_atom(&piece.atom)?;
                    let split = self.push(RegexInst::Split(atom.start, None));
                    let mut outs = atom.outs;
                    outs.push(RegexPatch::SplitB(split));
                    Ok(RegexFrag { start: split, outs })
                }
                RegexQuant::Range { min, max } => self.compile_range(&piece.atom, min, max),
            }
        }

        fn compile_range(
            &mut self,
            atom: &RegexAtom,
            min: usize,
            max: Option<usize>,
        ) -> Result<RegexFrag, String> {
            let mut frag = None;
            for _ in 0..min {
                let next = self.compile_atom(atom)?;
                if let Some(prev) = frag.take() {
                    let prev: RegexFrag = prev;
                    self.patch(&prev.outs, next.start);
                    frag = Some(RegexFrag {
                        start: prev.start,
                        outs: next.outs,
                    });
                } else {
                    frag = Some(next);
                }
            }
            let mut base = frag.unwrap_or_else(|| {
                let idx = self.push(RegexInst::Save(usize::MAX, None));
                RegexFrag {
                    start: idx,
                    outs: vec![RegexPatch::Next(idx)],
                }
            });
            match max {
                Some(m) => {
                    for _ in min..m {
                        let opt_piece = RegexPiece {
                            atom: atom.clone(),
                            quant: RegexQuant::ZeroOrOne,
                        };
                        let opt = self.compile_piece(&opt_piece)?;
                        self.patch(&base.outs, opt.start);
                        base = RegexFrag {
                            start: base.start,
                            outs: opt.outs,
                        };
                    }
                    Ok(base)
                }
                None => {
                    let star_piece = RegexPiece {
                        atom: atom.clone(),
                        quant: RegexQuant::ZeroOrMore,
                    };
                    let star = self.compile_piece(&star_piece)?;
                    self.patch(&base.outs, star.start);
                    Ok(RegexFrag {
                        start: base.start,
                        outs: star.outs,
                    })
                }
            }
        }

        fn compile_atom(&mut self, atom: &RegexAtom) -> Result<RegexFrag, String> {
            match atom {
                RegexAtom::Literal(ch) => Ok(self.consume(RegexMatcher::Literal(*ch))),
                RegexAtom::Any => Ok(self.consume(RegexMatcher::Any)),
                RegexAtom::Class(class) => Ok(self.consume(RegexMatcher::Class(class.clone()))),
                RegexAtom::Start => {
                    let idx = self.push(RegexInst::AssertStart(None));
                    Ok(RegexFrag {
                        start: idx,
                        outs: vec![RegexPatch::Next(idx)],
                    })
                }
                RegexAtom::End => {
                    let idx = self.push(RegexInst::AssertEnd(None));
                    Ok(RegexFrag {
                        start: idx,
                        outs: vec![RegexPatch::Next(idx)],
                    })
                }
                RegexAtom::Group(0, node) => self.compile_node(node),
                RegexAtom::Group(idx, node) => {
                    let start_save = self.push(RegexInst::Save(idx * 2, None));
                    let inner = self.compile_node(node)?;
                    self.patch(&[RegexPatch::Next(start_save)], inner.start);
                    let end_save = self.push(RegexInst::Save(idx * 2 + 1, None));
                    self.patch(&inner.outs, end_save);
                    Ok(RegexFrag {
                        start: start_save,
                        outs: vec![RegexPatch::Next(end_save)],
                    })
                }
            }
        }

        fn consume(&mut self, matcher: RegexMatcher) -> RegexFrag {
            let idx = self.push(RegexInst::Consume(matcher, None));
            RegexFrag {
                start: idx,
                outs: vec![RegexPatch::Next(idx)],
            }
        }
    }

    fn regex_matcher_matches(matcher: &RegexMatcher, ch: char, flags: &RegexFlags) -> bool {
        match matcher {
            RegexMatcher::Literal(expected) => {
                if flags.case_insensitive {
                    super::jet_text_simple_fold(*expected as u32) == super::jet_text_simple_fold(ch as u32)
                } else {
                    *expected == ch
                }
            }
            RegexMatcher::Any => flags.dotall || ch != '\n',
            RegexMatcher::Class(class) => regex_class_matches(class, ch, flags),
        }
    }

    fn regex_class_matches(class: &RegexClass, ch: char, flags: &RegexFlags) -> bool {
        let yes = class.items.iter().any(|item| match item {
            RegexClassItem::Char(c) => {
                if flags.case_insensitive {
                    super::jet_text_simple_fold(*c as u32) == super::jet_text_simple_fold(ch as u32)
                } else {
                    *c == ch
                }
            }
            RegexClassItem::Range(a, b) => {
                if flags.case_insensitive {
                    let lc = char::from_u32(super::jet_text_simple_fold(ch as u32)).unwrap_or(ch);
                    let la = char::from_u32(super::jet_text_simple_fold(*a as u32)).unwrap_or(*a);
                    let lb = char::from_u32(super::jet_text_simple_fold(*b as u32)).unwrap_or(*b);
                    la <= lc && lc <= lb
                } else {
                    *a <= ch && ch <= *b
                }
            }
            RegexClassItem::Digit => ch.is_ascii_digit(),
            RegexClassItem::Word => ch == '_' || ch.is_ascii_alphanumeric(),
            RegexClassItem::Space => super::jet_text_whitespace(ch as u32),
            RegexClassItem::UnicodeLetter => super::jet_text_letter(ch as u32),
            RegexClassItem::UnicodeNumber => super::jet_text_numeric(ch as u32),
            RegexClassItem::UnicodeAlphabetic => super::jet_text_alphabetic(ch as u32),
            RegexClassItem::UnicodeWhitespace => super::jet_text_whitespace(ch as u32),
        });
        if class.negated {
            !yes
        } else {
            yes
        }
    }

    fn regex_escaped_literal(ch: char) -> char {
        match ch {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            other => other,
        }
    }

    fn regex_slots_to_spans(slots: &[Option<usize>]) -> Vec<Option<(usize, usize)>> {
        slots
            .chunks(2)
            .map(|pair| match pair {
                [Some(start), Some(end)] => Some((*start, *end)),
                _ => None,
            })
            .collect()
    }

    fn regex_search_positions(text: &str, start: usize) -> impl Iterator<Item = usize> + '_ {
        text.char_indices()
            .map(|(idx, _)| idx)
            .filter(move |idx| *idx >= start)
            .chain(std::iter::once(text.len()).filter(move |idx| *idx >= start))
    }

    fn regex_next_search_pos(text: &str, start: usize, end: usize) -> usize {
        if end > start {
            return end;
        }
        text[end..]
            .chars()
            .next()
            .map(|ch| end + ch.len_utf8())
            .unwrap_or(text.len() + 1)
    }

    fn expand_regex_replacement(repl: &str, mat: &JetRegexMatch) -> String {
        let mut out = String::new();
        let mut chars = repl.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '$' {
                out.push(ch);
                continue;
            }
            match chars.peek().copied() {
                Some('$') => {
                    chars.next();
                    out.push('$');
                }
                Some('{') => {
                    chars.next();
                    let mut name = String::new();
                    while let Some(c) = chars.next() {
                        if c == '}' {
                            break;
                        }
                        name.push(c);
                    }
                    if let Ok(value) = mat.name(&name) {
                        out.push_str(&value);
                    }
                }
                Some(c) if c.is_ascii_digit() => {
                    let mut num = String::new();
                    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                        num.push(chars.next().unwrap());
                    }
                    if let Ok(idx) = num.parse::<i64>() {
                        if let Ok(value) = mat.group(idx) {
                            out.push_str(&value);
                        }
                    }
                }
                _ => out.push('$'),
            }
        }
        out
    }
