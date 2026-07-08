mod jet_std {
    #[derive(Clone, Debug, PartialEq)]
    pub enum IoError {
        NotFound { path: String },
        PermissionDenied { path: String },
        Other { message: String },
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Utf8Error {
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

    #[derive(Clone, Debug, PartialEq)]
    pub struct JetUrl {
        pub scheme: String,
        pub host: Option<String>,
        pub port: Option<i64>,
        pub path: String,
        pub query: Vec<(String, String)>,
        pub fragment: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct JetMime {
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
        pub fn group(&self, n: i64) -> Option<String> {
            let n = usize::try_from(n).ok()?;
            let (start, end) = self.spans.get(n).copied().flatten()?;
            Some(self.text[start..end].to_string())
        }

        pub fn name(&self, name: &str) -> Option<String> {
            let idx = self
                .names
                .iter()
                .position(|n| n.as_deref() == Some(name))?;
            self.group(idx as i64)
        }

        pub fn start(&self) -> i64 {
            self.group_start(0).unwrap_or(-1)
        }

        pub fn end(&self) -> i64 {
            self.group_end(0).unwrap_or(-1)
        }

        pub fn group_start(&self, n: i64) -> Option<i64> {
            let n = usize::try_from(n).ok()?;
            self.spans
                .get(n)
                .copied()
                .flatten()
                .map(|(start, _)| start as i64)
        }

        pub fn group_end(&self, n: i64) -> Option<i64> {
            let n = usize::try_from(n).ok()?;
            self.spans
                .get(n)
                .copied()
                .flatten()
                .map(|(_, end)| end as i64)
        }
    }

    impl JetRegex {
        pub fn is_match(&self, text: &str) -> bool {
            self.find_match(text).is_some()
        }

        pub fn match_value(&self, text: &str) -> Option<JetRegexMatch> {
            self.find_match(text)
        }

        pub fn find(&self, text: &str) -> Option<String> {
            self.find_match(text).and_then(|m| m.group(0))
        }

        pub fn find_all(&self, text: &str) -> Vec<String> {
            self.matches(text)
                .into_iter()
                .filter_map(|m| m.group(0))
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

    pub fn jet_regex_compile(pattern: &str) -> Result<JetRegex, String> {
        jet_regex_compile_with(pattern, &RegexFlags::default())
    }

    pub fn jet_regex_compile_with(pattern: &str, flags: &RegexFlags) -> Result<JetRegex, String> {
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

    pub fn jet_regex_is_match(pattern: &str, text: &str) -> Result<bool, String> {
        Ok(jet_regex_compile(pattern)?.is_match(text))
    }

    pub fn jet_regex_match(pattern: &str, text: &str) -> Result<Option<JetRegexMatch>, String> {
        Ok(jet_regex_compile(pattern)?.match_value(text))
    }

    pub fn jet_regex_find(pattern: &str, text: &str) -> Result<Option<String>, String> {
        Ok(jet_regex_compile(pattern)?.find(text))
    }

    pub fn jet_regex_find_all(pattern: &str, text: &str) -> Result<Vec<String>, String> {
        Ok(jet_regex_compile(pattern)?.find_all(text))
    }

    pub fn jet_regex_matches(pattern: &str, text: &str) -> Result<Vec<JetRegexMatch>, String> {
        Ok(jet_regex_compile(pattern)?.matches(text))
    }

    pub fn jet_regex_replace(pattern: &str, text: &str, repl: &str) -> Result<String, String> {
        Ok(jet_regex_compile(pattern)?.replace(text, repl))
    }

    pub fn jet_regex_replace_all(pattern: &str, text: &str, repl: &str) -> Result<String, String> {
        Ok(jet_regex_compile(pattern)?.replace_all(text, repl))
    }

    pub fn jet_regex_split(pattern: &str, text: &str) -> Result<Vec<String>, String> {
        Ok(jet_regex_compile(pattern)?.split(text))
    }

    pub fn jet_regex_split_limit(
        pattern: &str,
        text: &str,
        limit: i64,
    ) -> Result<Vec<String>, String> {
        Ok(jet_regex_compile(pattern)?.split_limit(text, limit))
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
                    expected.to_lowercase().to_string() == ch.to_lowercase().to_string()
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
                    c.to_lowercase().to_string() == ch.to_lowercase().to_string()
                } else {
                    *c == ch
                }
            }
            RegexClassItem::Range(a, b) => {
                if flags.case_insensitive {
                    let lc = ch.to_lowercase().next().unwrap_or(ch);
                    let la = a.to_lowercase().next().unwrap_or(*a);
                    let lb = b.to_lowercase().next().unwrap_or(*b);
                    la <= lc && lc <= lb
                } else {
                    *a <= ch && ch <= *b
                }
            }
            RegexClassItem::Digit => ch.is_ascii_digit(),
            RegexClassItem::Word => ch == '_' || ch.is_ascii_alphanumeric(),
            RegexClassItem::Space => ch.is_whitespace(),
            RegexClassItem::UnicodeLetter => ch.is_alphabetic(),
            RegexClassItem::UnicodeNumber => ch.is_numeric(),
            RegexClassItem::UnicodeAlphabetic => ch.is_alphabetic(),
            RegexClassItem::UnicodeWhitespace => ch.is_whitespace(),
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
                    if let Some(value) = mat.name(&name) {
                        out.push_str(&value);
                    }
                }
                Some(c) if c.is_ascii_digit() => {
                    let mut num = String::new();
                    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                        num.push(chars.next().unwrap());
                    }
                    if let Ok(idx) = num.parse::<i64>() {
                        if let Some(value) = mat.group(idx) {
                            out.push_str(&value);
                        }
                    }
                }
                _ => out.push('$'),
            }
        }
        out
    }

    impl JetUrl {
        pub fn parse(input: &String) -> Result<Self, String> {
            let raw = input.trim();
            let Some(colon) = raw.find(':') else {
                return Err("URL needs a scheme".to_string());
            };
            let scheme_raw = &raw[..colon];
            if !jet_url_valid_scheme(scheme_raw) {
                return Err(format!("invalid URL scheme `{}`", scheme_raw));
            }
            let scheme = scheme_raw.to_ascii_lowercase();
            let mut rest = &raw[colon + 1..];
            let mut fragment = None;
            if let Some(i) = rest.find('#') {
                fragment = Some(jet_url_percent_decode_str(&rest[i + 1..])?);
                rest = &rest[..i];
            }
            let mut query = Vec::new();
            if let Some(i) = rest.find('?') {
                query = jet_url_parse_query(&rest[i + 1..])?;
                rest = &rest[..i];
            }
            let mut host = None;
            let mut port = None;
            let path;
            if let Some(after_slashes) = rest.strip_prefix("//") {
                let auth_end = after_slashes.find('/').unwrap_or(after_slashes.len());
                let authority = &after_slashes[..auth_end];
                let path_raw = &after_slashes[auth_end..];
                let (h, p) = jet_url_parse_authority(authority)?;
                host = Some(h);
                port = p;
                path = if path_raw.is_empty() {
                    "/".to_string()
                } else {
                    jet_url_percent_decode_str(path_raw)?
                };
            } else if matches!(scheme.as_str(), "http" | "https") {
                return Err(format!("{} URL needs `//host`", scheme));
            } else if scheme == "data" {
                path = rest.to_string();
            } else {
                path = jet_url_percent_decode_str(rest)?;
            }
            let mut url = JetUrl {
                scheme,
                host,
                port,
                path,
                query,
                fragment,
            };
            url = url.normalize();
            Ok(url)
        }

        pub fn from_parts(
            scheme: &String,
            host: &String,
            path: &String,
            query: &Vec<Vec<String>>,
            fragment: &String,
        ) -> Result<Self, String> {
            if !jet_url_valid_scheme(scheme) {
                return Err(format!("invalid URL scheme `{}`", scheme));
            }
            let host = if host.is_empty() {
                None
            } else {
                Some(jet_url_host_to_ascii(host)?)
            };
            let fragment = if fragment.is_empty() {
                None
            } else {
                Some(fragment.clone())
            };
            Ok(JetUrl {
                scheme: scheme.to_ascii_lowercase(),
                host,
                port: None,
                path: path.clone(),
                query: jet_url_pairs_from_rows(query),
                fragment,
            }
            .normalize())
        }

        pub fn file(path: &String) -> Self {
            JetUrl {
                scheme: "file".to_string(),
                host: Some(String::new()),
                port: None,
                path: if path.starts_with('/') {
                    path.clone()
                } else {
                    format!("/{}", path)
                },
                query: Vec::new(),
                fragment: None,
            }
        }

        pub fn data(mime: &JetMime, text: &String) -> Self {
            JetUrl {
                scheme: "data".to_string(),
                host: None,
                port: None,
                path: format!(
                    "{},{}",
                    mime.to_string_value(),
                    jet_url_percent_encode(text, false)
                ),
                query: Vec::new(),
                fragment: None,
            }
        }

        pub fn scheme(&self) -> String {
            self.scheme.clone()
        }
        pub fn host(&self) -> Option<String> {
            self.host.clone().filter(|h| !h.is_empty())
        }
        pub fn port(&self) -> Option<i64> {
            self.port
        }
        pub fn path(&self) -> String {
            self.path.clone()
        }
        pub fn path_segments(&self) -> Vec<String> {
            self.path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        }
        pub fn query(&self) -> String {
            jet_url_render_query(&self.query)
        }
        pub fn query_pairs(&self) -> Vec<Vec<String>> {
            self.query
                .iter()
                .map(|(k, v)| vec![k.clone(), v.clone()])
                .collect()
        }
        pub fn fragment(&self) -> Option<String> {
            self.fragment.clone()
        }
        pub fn normalize(&self) -> Self {
            let mut out = self.clone();
            out.scheme = out.scheme.to_ascii_lowercase();
            if let Some(h) = &out.host {
                if let Ok(ascii) = jet_url_host_to_ascii(h) {
                    out.host = Some(ascii);
                }
            }
            if out.scheme != "data" {
                out.path = jet_url_remove_dot_segments(&out.path);
            }
            out
        }
        pub fn join(&self, rel: &String) -> Result<Self, String> {
            if let Some(colon) = rel.find(':') {
                let before_slash = rel.find('/').map_or(true, |slash| colon < slash);
                if before_slash && jet_url_valid_scheme(&rel[..colon]) {
                    return JetUrl::parse(rel);
                }
            }
            if rel.starts_with("//") {
                return JetUrl::parse(&format!("{}:{}", self.scheme, rel));
            }
            let mut out = self.clone();
            let mut rest = rel.as_str();
            out.fragment = None;
            if let Some(i) = rest.find('#') {
                out.fragment = Some(jet_url_percent_decode_str(&rest[i + 1..])?);
                rest = &rest[..i];
            }
            out.query.clear();
            if let Some(i) = rest.find('?') {
                out.query = jet_url_parse_query(&rest[i + 1..])?;
                rest = &rest[..i];
            }
            let path = jet_url_percent_decode_str(rest)?;
            out.path = if path.starts_with('/') {
                path
            } else {
                let base = self.path.rsplit_once('/').map(|(b, _)| b).unwrap_or("");
                if base.is_empty() {
                    format!("/{}", path)
                } else {
                    format!("{}/{}", base, path)
                }
            };
            Ok(out.normalize())
        }
        pub fn set_query(&self, key: &String, value: &String) -> Self {
            let mut out = self.clone();
            out.query.retain(|(k, _)| k != key);
            out.query.push((key.clone(), value.clone()));
            out
        }
        pub fn add_query(&self, key: &String, value: &String) -> Self {
            let mut out = self.clone();
            out.query.push((key.clone(), value.clone()));
            out
        }
        pub fn to_string_value(&self) -> String {
            let mut out = format!("{}:", self.scheme);
            if let Some(host) = &self.host {
                out.push_str("//");
                out.push_str(host);
                if let Some(port) = self.port {
                    out.push(':');
                    out.push_str(&port.to_string());
                }
            }
            if self.scheme == "data" && self.host.is_none() {
                out.push_str(&self.path);
            } else {
                out.push_str(&jet_url_percent_encode(&self.path, true));
            }
            if !self.query.is_empty() {
                out.push('?');
                out.push_str(&jet_url_render_query(&self.query));
            }
            if let Some(fragment) = &self.fragment {
                out.push('#');
                out.push_str(&jet_url_percent_encode(fragment, false));
            }
            out
        }
    }

    impl crate::JetShow for JetUrl {
        fn jet_show(&self) -> String {
            self.to_string_value()
        }
    }

    impl JetMime {
        pub fn parse(input: &String) -> Result<Self, String> {
            let mut parts = input.split(';');
            let essence = parts.next().unwrap_or("").trim();
            let Some((top, sub)) = essence.split_once('/') else {
                return Err("MIME type needs `type/subtype`".to_string());
            };
            let top = top.trim().to_ascii_lowercase();
            let sub = sub.trim().to_ascii_lowercase();
            if top.is_empty() || sub.is_empty() || !jet_mime_token(&top) || !jet_mime_token(&sub) {
                return Err(format!("invalid MIME type `{}`", essence));
            }
            let mut params = Vec::new();
            for p in parts {
                let p = p.trim();
                if p.is_empty() {
                    continue;
                }
                let Some((k, v)) = p.split_once('=') else {
                    return Err(format!("invalid MIME parameter `{}`", p));
                };
                let key = k.trim().to_ascii_lowercase();
                let val = v.trim().trim_matches('"').to_string();
                if key.is_empty() || !jet_mime_token(&key) {
                    return Err(format!("invalid MIME parameter `{}`", k.trim()));
                }
                params.push((key, val));
            }
            Ok(JetMime { top, sub, params })
        }
        pub fn media_type(&self) -> String {
            self.top.clone()
        }
        pub fn subtype(&self) -> String {
            self.sub.clone()
        }
        pub fn essence(&self) -> String {
            format!("{}/{}", self.top, self.sub)
        }
        pub fn param(&self, name: &String) -> Option<String> {
            let needle = name.to_ascii_lowercase();
            self.params
                .iter()
                .find(|(k, _)| k == &needle)
                .map(|(_, v)| v.clone())
        }
        pub fn params(&self) -> Vec<Vec<String>> {
            self.params
                .iter()
                .map(|(k, v)| vec![k.clone(), v.clone()])
                .collect()
        }
        pub fn to_string_value(&self) -> String {
            let mut out = self.essence();
            for (k, v) in &self.params {
                out.push_str("; ");
                out.push_str(k);
                out.push('=');
                out.push_str(v);
            }
            out
        }
    }

    impl crate::JetShow for JetMime {
        fn jet_show(&self) -> String {
            self.to_string_value()
        }
    }

    fn jet_url_valid_scheme(s: &str) -> bool {
        let mut chars = s.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    }

    fn jet_url_parse_authority(authority: &str) -> Result<(String, Option<i64>), String> {
        let host_port = authority.rsplit_once('@').map(|(_, h)| h).unwrap_or(authority);
        if host_port.is_empty() {
            return Err("URL host is empty".to_string());
        }
        if host_port.starts_with('[') {
            let Some(end) = host_port.find(']') else {
                return Err("IPv6 host is missing `]`".to_string());
            };
            let host = host_port[..=end].to_ascii_lowercase();
            let port = if host_port[end + 1..].starts_with(':') {
                Some(jet_url_parse_port(&host_port[end + 2..])?)
            } else {
                None
            };
            return Ok((host, port));
        }
        let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
            if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
                (h, Some(jet_url_parse_port(p)?))
            } else {
                (host_port, None)
            }
        } else {
            (host_port, None)
        };
        Ok((jet_url_host_to_ascii(host)?, port))
    }

    fn jet_url_parse_port(p: &str) -> Result<i64, String> {
        let n: i64 = p.parse().map_err(|_| format!("invalid URL port `{}`", p))?;
        if !(0..=65535).contains(&n) {
            return Err(format!("URL port out of range `{}`", p));
        }
        Ok(n)
    }

    fn jet_url_host_to_ascii(host: &str) -> Result<String, String> {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        let mut labels = Vec::new();
        for label in host.split('.') {
            if label.is_empty() {
                return Err("URL host has an empty label".to_string());
            }
            labels.push(if label.is_ascii() {
                label.to_string()
            } else {
                format!("xn--{}", jet_punycode_encode(label)?)
            });
        }
        Ok(labels.join("."))
    }

    fn jet_punycode_encode(input: &str) -> Result<String, String> {
        const BASE: u32 = 36;
        const TMIN: u32 = 1;
        const TMAX: u32 = 26;
        const SKEW: u32 = 38;
        const DAMP: u32 = 700;
        const INITIAL_BIAS: u32 = 72;
        const INITIAL_N: u32 = 128;
        let codepoints: Vec<u32> = input.chars().map(|c| c as u32).collect();
        let mut out = String::new();
        for &cp in &codepoints {
            if cp < 0x80 {
                out.push(char::from_u32(cp).ok_or_else(|| "bad codepoint".to_string())?);
            }
        }
        let basic = out.chars().count() as u32;
        let mut handled = basic;
        if basic > 0 && handled < codepoints.len() as u32 {
            out.push('-');
        }
        let mut n = INITIAL_N;
        let mut delta = 0u32;
        let mut bias = INITIAL_BIAS;
        while handled < codepoints.len() as u32 {
            let m = *codepoints
                .iter()
                .filter(|&&cp| cp >= n)
                .min()
                .ok_or_else(|| "bad punycode input".to_string())?;
            delta = delta
                .checked_add((m - n).saturating_mul(handled + 1))
                .ok_or_else(|| "punycode overflow".to_string())?;
            n = m;
            for &cp in &codepoints {
                if cp < n {
                    delta = delta.checked_add(1).ok_or_else(|| "punycode overflow".to_string())?;
                } else if cp == n {
                    let mut q = delta;
                    let mut k = BASE;
                    loop {
                        let t = if k <= bias {
                            TMIN
                        } else if k >= bias + TMAX {
                            TMAX
                        } else {
                            k - bias
                        };
                        if q < t {
                            break;
                        }
                        out.push(jet_punycode_digit(t + ((q - t) % (BASE - t)))?);
                        q = (q - t) / (BASE - t);
                        k += BASE;
                    }
                    out.push(jet_punycode_digit(q)?);
                    bias = jet_punycode_adapt(delta, handled + 1, handled == basic);
                    delta = 0;
                    handled += 1;
                }
            }
            delta = delta.checked_add(1).ok_or_else(|| "punycode overflow".to_string())?;
            n = n.checked_add(1).ok_or_else(|| "punycode overflow".to_string())?;
        }
        Ok(out)
    }

    fn jet_punycode_digit(d: u32) -> Result<char, String> {
        char::from_u32(if d < 26 { b'a' as u32 + d } else { b'0' as u32 + d - 26 })
            .ok_or_else(|| "bad punycode digit".to_string())
    }

    fn jet_punycode_adapt(mut delta: u32, points: u32, first: bool) -> u32 {
        delta = if first { delta / 700 } else { delta / 2 };
        delta += delta / points;
        let mut k = 0;
        while delta > ((36 - 1) * 26) / 2 {
            delta /= 36 - 1;
            k += 36;
        }
        k + (((36 - 1 + 1) * delta) / (delta + 38))
    }

    fn jet_url_parse_query(q: &str) -> Result<Vec<(String, String)>, String> {
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for part in q.split('&') {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            out.push((jet_url_percent_decode_str(k)?, jet_url_percent_decode_str(v)?));
        }
        Ok(out)
    }

    fn jet_url_pairs_from_rows(rows: &Vec<Vec<String>>) -> Vec<(String, String)> {
        rows.iter()
            .filter(|r| !r.is_empty())
            .map(|r| {
                (
                    r.get(0).cloned().unwrap_or_default(),
                    r.get(1).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }

    fn jet_url_render_query(pairs: &[(String, String)]) -> String {
        pairs
            .iter()
            .map(|(k, v)| format!(
                "{}={}",
                jet_url_percent_encode(k, false),
                jet_url_percent_encode(v, false)
            ))
            .collect::<Vec<_>>()
            .join("&")
    }

    pub fn jet_url_percent_encode(s: &str, path: bool) -> String {
        let mut out = String::new();
        for b in s.as_bytes() {
            let keep = b.is_ascii_alphanumeric()
                || matches!(*b, b'-' | b'.' | b'_' | b'~')
                || (path && *b == b'/');
            if keep {
                out.push(*b as char);
            } else {
                out.push_str(&format!("%{:02X}", b));
            }
        }
        out
    }

    pub fn jet_url_percent_decode_str(s: &str) -> Result<String, String> {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                if i + 2 >= bytes.len() {
                    return Err("truncated percent escape".to_string());
                }
                let hex = &s[i + 1..i + 3];
                let byte = u8::from_str_radix(hex, 16)
                    .map_err(|_| format!("invalid percent escape `%{}`", hex))?;
                out.push(byte);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).map_err(|_| "percent escape is not valid UTF-8".to_string())
    }

    fn jet_url_remove_dot_segments(path: &str) -> String {
        if path.is_empty() || !path.contains('.') {
            return path.to_string();
        }
        let absolute = path.starts_with('/');
        let trailing = path.ends_with('/');
        let mut parts = Vec::new();
        for p in path.split('/') {
            match p {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                _ => parts.push(p),
            }
        }
        let mut out = String::new();
        if absolute {
            out.push('/');
        }
        out.push_str(&parts.join("/"));
        if trailing && !out.ends_with('/') {
            out.push('/');
        }
        if out.is_empty() && absolute {
            "/".to_string()
        } else {
            out
        }
    }

    fn jet_mime_token(s: &str) -> bool {
        !s.is_empty()
            && s.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(
                        b,
                        b'!' | b'#'
                            | b'$'
                            | b'&'
                            | b'-'
                            | b'^'
                            | b'_'
                            | b'.'
                            | b'+'
                    )
            })
    }

    pub fn jet_mime_from_extension(ext: &str) -> Option<&'static str> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "html" | "htm" => Some("text/html"),
            "css" => Some("text/css"),
            "csv" => Some("text/csv"),
            "txt" | "text" => Some("text/plain"),
            "md" => Some("text/markdown"),
            "json" => Some("application/json"),
            "js" | "mjs" => Some("text/javascript"),
            "wasm" => Some("application/wasm"),
            "pdf" => Some("application/pdf"),
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "svg" => Some("image/svg+xml"),
            "webp" => Some("image/webp"),
            "ico" => Some("image/x-icon"),
            "mp3" => Some("audio/mpeg"),
            "mp4" => Some("video/mp4"),
            "xml" => Some("application/xml"),
            "zip" => Some("application/zip"),
            "gz" => Some("application/gzip"),
            "tar" => Some("application/x-tar"),
            _ => None,
        }
    }

    pub fn jet_extension_from_mime(mime: &str) -> Option<&'static str> {
        match mime.to_ascii_lowercase().split(';').next().unwrap_or("").trim() {
            "text/html" => Some("html"),
            "text/css" => Some("css"),
            "text/csv" => Some("csv"),
            "text/plain" => Some("txt"),
            "text/markdown" => Some("md"),
            "application/json" => Some("json"),
            "text/javascript" | "application/javascript" => Some("js"),
            "application/wasm" => Some("wasm"),
            "application/pdf" => Some("pdf"),
            "image/png" => Some("png"),
            "image/jpeg" => Some("jpg"),
            "image/gif" => Some("gif"),
            "image/svg+xml" => Some("svg"),
            "image/webp" => Some("webp"),
            "image/x-icon" => Some("ico"),
            "audio/mpeg" => Some("mp3"),
            "video/mp4" => Some("mp4"),
            "application/xml" | "text/xml" => Some("xml"),
            "application/zip" => Some("zip"),
            "application/gzip" => Some("gz"),
            "application/x-tar" => Some("tar"),
            _ => None,
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum ProcessStreamMode {
        Capture,
        Inherit,
        Discard,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct ProcessSpec {
        pub cmd: Vec<String>,
        pub cwd: Option<String>,
        pub env_clear: bool,
        pub env_set: Vec<(String, String)>,
        pub env_remove: Vec<String>,
        pub stdin_text: Option<String>,
        pub stdout: ProcessStreamMode,
        pub stderr: ProcessStreamMode,
        pub timeout_ms: Option<i64>,
        pub output_limit: Option<i64>,
        pub detached: bool,
    }

    #[derive(Clone, Debug)]
    pub struct ProcessChild {
        pub inner: std::rc::Rc<std::cell::RefCell<Option<std::process::Child>>>,
        pub stdin: std::rc::Rc<std::cell::RefCell<Option<std::process::ChildStdin>>>,
        pub stdout:
            std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<std::process::ChildStdout>>>>,
        pub stderr:
            std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<std::process::ChildStderr>>>>,
        pub timeout_ms: Option<i64>,
        pub started: std::time::Instant,
    }

    impl PartialEq for ProcessChild {
        fn eq(&self, other: &Self) -> bool {
            std::rc::Rc::ptr_eq(&self.inner, &other.inner)
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DirEntry {
        pub name: String,
        pub path: String,
        pub is_dir: bool,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Stat {
        pub size: i64,
        pub modified_ms: i64,
        pub created_ms: i64,
        pub readonly: bool,
        pub is_file: bool,
        pub is_dir: bool,
        pub is_symlink: bool,
        pub kind: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct WalkEntry {
        pub path: String,
        pub relative: String,
        pub is_dir: bool,
        pub depth: i64,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct WatchEvent {
        pub domain: String,
        pub kind: String,
        pub path: String,
        pub detail: String,
        pub pid: i64,
        pub port: i64,
    }

    #[derive(Clone, Debug)]
    pub struct TempDir {
        pub path: String,
        pub cleanup: std::rc::Rc<()>,
    }

    #[derive(Clone, Debug)]
    pub struct TempFile {
        pub path: String,
        pub cleanup: std::rc::Rc<()>,
    }

    #[derive(Clone, Debug)]
    pub struct FileLock {
        pub path: String,
        pub cleanup: std::rc::Rc<()>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DataGroup {
        pub key: String,
        pub count: i64,
        pub sum: f64,
        pub mean: f64,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DataStatus {
        pub step: String,
        pub path: String,
        pub replacement: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DataSummary {
        pub count: i64,
        pub sum: f64,
        pub mean: f64,
        pub min: f64,
        pub max: f64,
        pub median: f64,
        pub variance: f64,
        pub stddev: f64,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct LogField {
        pub key: String,
        pub value: String,
        pub kind: String,
        pub redacted: bool,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct LogSpan {
        pub id: i64,
        pub name: String,
    }

    #[derive(Clone, Debug)]
    pub struct Stopwatch {
        pub start: std::time::Instant,
    }

    // D-DET1: deterministic injected Clock capability. `now` is the current value
    // in ms (starts at the caller's seed); `tick(ms)` advances it. No wall-clock
    // read — reproducible by construction.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Clock {
        pub now: i64,
    }

    // D-DET1: deterministic injected Rng capability. A SplitMix64 state stream
    // (std-only, no external crate — I6). The same seed yields the same draws on
    // every machine.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Rng {
        pub state: u64,
    }

    // D-SOLVER-LIB1=A: explicit finite solver state. This first slice records
    // ordinary Bool constraints in insertion order; no hidden backtracking.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Solver {
        pub seed: i64,
        pub checked: i64,
        pub failures: i64,
    }

    // D-DET-CAPAPI: a deterministic span of milliseconds. Minted by `time.ms(n)` /
    // `time.secs(n)` (pure value constructors). The injected `Clock` advances by one
    // with `clock.wait(d)`; read it back with `duration.millis()`. std-only (I6).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Duration {
        pub ms: i64,
    }

    // D-BIGINT1: arbitrary-precision integer (std-only limb arithmetic).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct JetBigInt {
        negative: bool,
        limbs: Vec<u32>, // little-endian base 10^9
    }

    const BI_BASE: u64 = 1_000_000_000;

    impl JetBigInt {
        pub fn from_int(n: i64) -> Self {
            if n == 0 {
                return JetBigInt {
                    negative: false,
                    limbs: vec![0],
                };
            }
            let negative = n < 0;
            let mut v = if negative {
                (n as i128).wrapping_neg() as u64
            } else {
                n as u64
            };
            let mut limbs = Vec::new();
            while v > 0 {
                limbs.push((v % BI_BASE) as u32);
                v /= BI_BASE;
            }
            JetBigInt { negative, limbs }
        }

        pub fn from_str(s: &str) -> Result<Self, String> {
            let t = s.trim();
            if t.is_empty() {
                return Err("empty BigInt string".to_string());
            }
            let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
                (true, rest)
            } else if let Some(rest) = t.strip_prefix('+') {
                (false, rest)
            } else {
                (false, t)
            };
            if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("invalid BigInt string `{s}`"));
            }
            let mut acc = JetBigInt {
                negative: false,
                limbs: vec![0],
            };
            for ch in body.chars() {
                let digit = ch.to_digit(10).unwrap() as u32;
                acc = acc.mul_small(10).add_small(digit);
            }
            acc.negative = negative && !(acc.limbs.len() == 1 && acc.limbs[0] == 0);
            Ok(acc)
        }

        fn normalize(mut self) -> Self {
            while self.limbs.len() > 1 && *self.limbs.last().unwrap() == 0 {
                self.limbs.pop();
            }
            if self.limbs.len() == 1 && self.limbs[0] == 0 {
                self.negative = false;
            }
            self
        }

        fn mul_small(&self, m: u32) -> Self {
            let mut carry = 0u64;
            let mut limbs = Vec::with_capacity(self.limbs.len() + 1);
            for &limb in &self.limbs {
                let prod = limb as u64 * m as u64 + carry;
                limbs.push((prod % BI_BASE) as u32);
                carry = prod / BI_BASE;
            }
            if carry > 0 {
                limbs.push(carry as u32);
            }
            JetBigInt {
                negative: self.negative,
                limbs,
            }
            .normalize()
        }

        fn add_small(&self, n: u32) -> Self {
            self.add(&JetBigInt::from_int(n as i64))
        }

        pub fn add(&self, other: &JetBigInt) -> JetBigInt {
            if self.negative == other.negative {
                let mut carry = 0u64;
                let len = self.limbs.len().max(other.limbs.len());
                let mut limbs = Vec::with_capacity(len + 1);
                for i in 0..len {
                    let a = *self.limbs.get(i).unwrap_or(&0) as u64;
                    let b = *other.limbs.get(i).unwrap_or(&0) as u64;
                    let sum = a + b + carry;
                    limbs.push((sum % BI_BASE) as u32);
                    carry = sum / BI_BASE;
                }
                if carry > 0 {
                    limbs.push(carry as u32);
                }
                JetBigInt {
                    negative: self.negative,
                    limbs,
                }
                .normalize()
            } else {
                let cmp = self.cmp_abs(other);
                if cmp == 0 {
                    JetBigInt::from_int(0)
                } else if cmp > 0 {
                    self.sub_abs(other).with_sign(self.negative)
                } else {
                    other.sub_abs(self).with_sign(other.negative)
                }
            }
        }

        fn with_sign(self, negative: bool) -> Self {
            JetBigInt {
                negative,
                limbs: self.limbs,
            }
        }

        pub fn sub(&self, other: &JetBigInt) -> JetBigInt {
            let mut neg_other = other.clone();
            neg_other.negative = !neg_other.negative;
            self.add(&neg_other)
        }

        fn sub_abs(&self, other: &JetBigInt) -> JetBigInt {
            let mut borrow = 0i64;
            let len = self.limbs.len();
            let mut limbs = Vec::with_capacity(len);
            for i in 0..len {
                let a = self.limbs[i] as i64;
                let b = *other.limbs.get(i).unwrap_or(&0) as i64;
                let mut cur = a - b - borrow;
                if cur < 0 {
                    cur += BI_BASE as i64;
                    borrow = 1;
                } else {
                    borrow = 0;
                }
                limbs.push(cur as u32);
            }
            JetBigInt {
                negative: false,
                limbs,
            }
            .normalize()
        }

        fn cmp_abs(&self, other: &JetBigInt) -> i8 {
            match self.limbs.len().cmp(&other.limbs.len()) {
                std::cmp::Ordering::Greater => 1,
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => {
                    for (a, b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
                        match a.cmp(b) {
                            std::cmp::Ordering::Greater => return 1,
                            std::cmp::Ordering::Less => return -1,
                            std::cmp::Ordering::Equal => {}
                        }
                    }
                    0
                }
            }
        }

        pub fn mul(&self, other: &JetBigInt) -> JetBigInt {
            let mut out = JetBigInt::from_int(0);
            for (i, &limb) in other.limbs.iter().enumerate() {
                if limb == 0 {
                    continue;
                }
                let mut part = self.mul_small(limb);
                for _ in 0..i {
                    part = part.mul_small(BI_BASE as u32);
                }
                out = out.add(&part);
            }
            JetBigInt {
                negative: self.negative != other.negative,
                limbs: out.limbs,
            }
            .normalize()
        }

        pub fn neg(&self) -> JetBigInt {
            if self.limbs.len() == 1 && self.limbs[0] == 0 {
                self.clone()
            } else {
                JetBigInt {
                    negative: !self.negative,
                    limbs: self.limbs.clone(),
                }
            }
        }

        pub fn to_string_rep(&self) -> String {
            if self.limbs.len() == 1 && self.limbs[0] == 0 {
                return "0".to_string();
            }
            let mut s = String::new();
            let top = *self.limbs.last().unwrap();
            s.push_str(&top.to_string());
            for &limb in self.limbs.iter().rev().skip(1) {
                s.push_str(&format!("{:09}", limb));
            }
            if self.negative {
                format!("-{s}")
            } else {
                s
            }
        }
    }

    impl super::JetShow for JetBigInt {
        fn jet_show(&self) -> String {
            self.to_string_rep()
        }
    }

    // D-DECIMAL1: exact base-10 decimal (scaled integer + scale).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct JetDecimal {
        negative: bool,
        digits: Vec<u8>, // big-endian mantissa digits 0-9, no dot
        scale: u32,
    }

    impl JetDecimal {
        pub fn from_str(s: &str) -> Result<Self, String> {
            let t = s.trim();
            if t.is_empty() {
                return Err("empty Decimal string".to_string());
            }
            let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
                (true, rest)
            } else if let Some(rest) = t.strip_prefix('+') {
                (false, rest)
            } else {
                (false, t)
            };
            let parts: Vec<&str> = body.split('.').collect();
            if parts.len() > 2 {
                return Err(format!("invalid Decimal string `{s}`"));
            }
            let (int_part, frac_part) = (parts[0], parts.get(1).copied().unwrap_or(""));
            if int_part.is_empty() && frac_part.is_empty() {
                return Err(format!("invalid Decimal string `{s}`"));
            }
            if !int_part.chars().all(|c| c.is_ascii_digit())
                || !frac_part.chars().all(|c| c.is_ascii_digit())
            {
                return Err(format!("invalid Decimal string `{s}`"));
            }
            let mut digits: Vec<u8> = int_part
                .chars()
                .chain(frac_part.chars())
                .map(|c| (c as u8 - b'0'))
                .collect();
            while digits.len() > 1 && digits.first() == Some(&0) {
                digits.remove(0);
            }
            if digits.is_empty() {
                digits.push(0);
            }
            let scale = frac_part.len() as u32;
            Ok(JetDecimal {
                negative,
                digits,
                scale,
            }
            .normalize())
        }

        fn normalize(mut self) -> Self {
            while self.digits.len() > 1 && self.digits.last() == Some(&0) {
                self.digits.pop();
            }
            if self.digits == [0] {
                self.negative = false;
            }
            self
        }

        fn align_scales(a: &JetDecimal, b: &JetDecimal) -> (JetDecimal, JetDecimal) {
            let scale = a.scale.max(b.scale);
            let mut left = a.clone();
            let mut right = b.clone();
            while left.scale < scale {
                left.digits.push(0);
                left.scale += 1;
            }
            while right.scale < scale {
                right.digits.push(0);
                right.scale += 1;
            }
            (left, right)
        }

        fn to_bigint(&self) -> JetBigInt {
            let mut s = String::new();
            for &d in &self.digits {
                s.push((b'0' + d) as char);
            }
            JetBigInt::from_str(&s).unwrap()
        }

        fn from_bigint(v: JetBigInt, scale: u32, negative: bool) -> JetDecimal {
            let s = v.to_string_rep();
            let body = if s.starts_with('-') { &s[1..] } else { &s };
            let digits: Vec<u8> = body.bytes().map(|b| b - b'0').collect();
            JetDecimal {
                negative,
                digits,
                scale,
            }
            .normalize()
        }

        pub fn add(&self, other: &JetDecimal) -> JetDecimal {
            let (a, b) = JetDecimal::align_scales(self, other);
            let sum = a.to_bigint().add(&b.to_bigint());
            let negative = if a.negative == b.negative {
                a.negative
            } else if a.to_bigint().cmp_abs(&b.to_bigint()) >= 0 {
                a.negative
            } else {
                b.negative
            };
            if a.negative == b.negative {
                JetDecimal::from_bigint(sum, a.scale, negative)
            } else {
                let diff = if a.to_bigint().cmp_abs(&b.to_bigint()) >= 0 {
                    a.to_bigint().sub_abs(&b.to_bigint())
                } else {
                    b.to_bigint().sub_abs(&a.to_bigint())
                };
                JetDecimal::from_bigint(diff, a.scale, negative)
            }
        }

        pub fn sub(&self, other: &JetDecimal) -> JetDecimal {
            let mut neg = other.clone();
            neg.negative = !neg.negative;
            self.add(&neg)
        }

        pub fn mul(&self, other: &JetDecimal) -> JetDecimal {
            let prod = self.to_bigint().mul(&other.to_bigint());
            JetDecimal::from_bigint(
                prod,
                self.scale + other.scale,
                self.negative != other.negative,
            )
        }

        pub fn to_string_rep(&self) -> String {
            if self.digits == [0] {
                return if self.scale == 0 {
                    "0".to_string()
                } else {
                    format!("0.{}", "0".repeat(self.scale as usize))
                };
            }
            let mut int_digits = self.digits.clone();
            let frac_len = self.scale as usize;
            let sign = if self.negative { "-" } else { "" };
            if frac_len == 0 {
                let s: String = int_digits.iter().map(|d| (b'0' + *d) as char).collect();
                return format!("{sign}{s}");
            }
            if int_digits.len() <= frac_len {
                let pad = frac_len - int_digits.len() + 1;
                int_digits.splice(0..0, vec![0; pad]);
            }
            let split = int_digits.len() - frac_len;
            let (whole, frac) = int_digits.split_at(split);
            let w: String = whole.iter().map(|d| (b'0' + *d) as char).collect();
            let f: String = frac.iter().map(|d| (b'0' + *d) as char).collect();
            format!("{sign}{w}.{f}")
        }
    }

    impl super::JetShow for JetDecimal {
        fn jet_show(&self) -> String {
            self.to_string_rep()
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct JsonError {
        pub line: i64,
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Json {
        Null,
        Boolean(bool),
        Number(f64),
        Text(String),
        Array(Vec<Json>),
        Object(std::collections::BTreeMap<String, Json>),
    }

    impl super::JetShow for IoError {
        fn jet_show(&self) -> String {
            format!("{:?}", self)
        }
    }
    impl super::JetShow for Utf8Error {
        fn jet_show(&self) -> String {
            self.message.clone()
        }
    }
    impl super::JetShow for ProcessResult {
        fn jet_show(&self) -> String {
            format!("{:?}", self)
        }
    }
    impl super::JetShow for ProcessSpec {
        fn jet_show(&self) -> String {
            format!("ProcessSpec({:?})", self.cmd)
        }
    }
    impl super::JetShow for ProcessChild {
        fn jet_show(&self) -> String {
            "ProcessChild".to_string()
        }
    }
    impl super::JetShow for DirEntry {
        fn jet_show(&self) -> String {
            format!(
                "DirEntry {{ name: {:?}, path: {:?}, is_dir: {} }}",
                self.name, self.path, self.is_dir
            )
        }
    }
    impl super::JetShow for Stat {
        fn jet_show(&self) -> String {
            format!("Stat {{ kind: {}, size: {} }}", self.kind, self.size)
        }
    }
    impl super::JetShow for WalkEntry {
        fn jet_show(&self) -> String {
            format!(
                "WalkEntry {{ path: {:?}, depth: {} }}",
                self.path, self.depth
            )
        }
    }
    impl super::JetShow for WatchEvent {
        fn jet_show(&self) -> String {
            format!(
                "WatchEvent {{ domain: {}, kind: {}, path: {}, detail: {} }}",
                self.domain, self.kind, self.path, self.detail
            )
        }
    }
    impl super::JetShow for TempDir {
        fn jet_show(&self) -> String {
            self.path.clone()
        }
    }
    impl super::JetShow for TempFile {
        fn jet_show(&self) -> String {
            self.path.clone()
        }
    }
    impl super::JetShow for FileLock {
        fn jet_show(&self) -> String {
            self.path.clone()
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            if std::rc::Rc::strong_count(&self.cleanup) == 1 {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }
    impl Drop for TempFile {
        fn drop(&mut self) {
            if std::rc::Rc::strong_count(&self.cleanup) == 1 {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
    impl Drop for FileLock {
        fn drop(&mut self) {
            if std::rc::Rc::strong_count(&self.cleanup) == 1 {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
    impl super::JetShow for Stopwatch {
        fn jet_show(&self) -> String {
            format!("{:?}", self.start)
        }
    }
    impl super::JetShow for Clock {
        fn jet_show(&self) -> String {
            format!("Clock {{ now: {} }}", self.now)
        }
    }
    impl super::JetShow for Rng {
        fn jet_show(&self) -> String {
            format!("Rng {{ .. }}")
        }
    }
    impl super::JetShow for Solver {
        fn jet_show(&self) -> String {
            format!(
                "Solver {{ seed: {}, checked: {}, failures: {} }}",
                self.seed, self.checked, self.failures
            )
        }
    }
    impl super::JetShow for Duration {
        fn jet_show(&self) -> String {
            format!("{}ms", self.ms)
        }
    }
    impl super::JetShow for JsonError {
        fn jet_show(&self) -> String {
            format!("line {}: {}", self.line, self.message)
        }
    }
    impl super::JetShow for Json {
        fn jet_show(&self) -> String {
            render_json(self, false, 0)
        }
    }

    // D-SERDE-ACCESS=B: accessor methods on Json (= Data).
    impl Json {
        pub fn field(&self, name: &str) -> Result<Json, String> {
            match self {
                Json::Object(map) => map
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("field `{}` not found", name)),
                _ => Err(format!(
                    "expected object, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
        pub fn at(&self, i: i64) -> Result<Json, String> {
            match self {
                Json::Array(items) => {
                    let idx = if i < 0 {
                        items.len().wrapping_sub((-i) as usize)
                    } else {
                        i as usize
                    };
                    items
                        .get(idx)
                        .cloned()
                        .ok_or_else(|| format!("index {} out of bounds (len {})", i, items.len()))
                }
                _ => Err(format!(
                    "expected array, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
        pub fn int(&self) -> Result<i64, String> {
            match self {
                Json::Number(f) => {
                    let n = *f as i64;
                    if (n as f64 - f).abs() < 0.5 {
                        Ok(n)
                    } else {
                        Err(format!("{} is not an integer", f))
                    }
                }
                _ => Err(format!(
                    "expected number, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
        pub fn text(&self) -> Result<String, String> {
            match self {
                Json::Text(s) => Ok(s.clone()),
                _ => Err(format!(
                    "expected text, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
        pub fn bool(&self) -> Result<bool, String> {
            match self {
                Json::Boolean(b) => Ok(*b),
                _ => Err(format!(
                    "expected bool, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
        pub fn float(&self) -> Result<f64, String> {
            match self {
                Json::Number(f) => Ok(*f),
                _ => Err(format!(
                    "expected number, got {}",
                    render_json(self, false, 0)
                )),
            }
        }
    }

    // ── core.db: the tagged SQL parameter/column value (D-DBDRIVER1) ───────────
    // `DbValue` mirrors `Json`'s dynamic-value construction mechanism
    // (`DbValue.Int(n)` / `.Float(f)` / `.Text(s)` / `.Bool(b)` / `.Null`) but is
    // SQL-shaped: `Int` keeps the full 64-bit width SQLite integers carry (never
    // routed through `f64`, which would lose precision above 2^53). A `Row` is
    // `Map<String, DbValue>` — the built-in `Map` type already gives `.get`/
    // `.keys`/`.values`, so no separate nominal `Row` type is needed (I8).
    #[derive(Clone, Debug, PartialEq)]
    pub enum DbValue {
        Null,
        Int(i64),
        Float(f64),
        Text(String),
        Bool(bool),
    }

    impl super::JetShow for DbValue {
        fn jet_show(&self) -> String {
            render_db_value(self)
        }
    }

    fn render_db_value(v: &DbValue) -> String {
        match v {
            DbValue::Null => "null".to_string(),
            DbValue::Int(n) => n.to_string(),
            DbValue::Float(f) => f.to_string(),
            DbValue::Text(s) => s.clone(),
            DbValue::Bool(b) => b.to_string(),
        }
    }

    impl DbValue {
        pub fn is_null(&self) -> bool {
            matches!(self, DbValue::Null)
        }
        pub fn int(&self) -> Result<i64, String> {
            match self {
                DbValue::Int(n) => Ok(*n),
                _ => Err(format!("expected an int, got {}", render_db_value(self))),
            }
        }
        pub fn float(&self) -> Result<f64, String> {
            match self {
                DbValue::Float(f) => Ok(*f),
                DbValue::Int(n) => Ok(*n as f64),
                _ => Err(format!("expected a float, got {}", render_db_value(self))),
            }
        }
        pub fn text(&self) -> Result<String, String> {
            match self {
                DbValue::Text(s) => Ok(s.clone()),
                _ => Err(format!("expected text, got {}", render_db_value(self))),
            }
        }
        pub fn bool(&self) -> Result<bool, String> {
            match self {
                DbValue::Bool(b) => Ok(*b),
                _ => Err(format!("expected a bool, got {}", render_db_value(self))),
            }
        }
    }

    pub fn jet_db_row_value(
        row: &std::collections::BTreeMap<String, DbValue>,
        key: &String,
    ) -> Result<DbValue, String> {
        row.get(key)
            .cloned()
            .ok_or_else(|| format!("missing column `{}`", key))
    }

    pub fn jet_db_row_int(
        row: &std::collections::BTreeMap<String, DbValue>,
        key: &String,
    ) -> Result<i64, String> {
        jet_db_row_value(row, key).and_then(|v| v.int())
    }

    pub fn jet_db_row_float(
        row: &std::collections::BTreeMap<String, DbValue>,
        key: &String,
    ) -> Result<f64, String> {
        jet_db_row_value(row, key).and_then(|v| v.float())
    }

    pub fn jet_db_row_text(
        row: &std::collections::BTreeMap<String, DbValue>,
        key: &String,
    ) -> Result<String, String> {
        jet_db_row_value(row, key).and_then(|v| v.text())
    }

    pub fn jet_db_row_bool(
        row: &std::collections::BTreeMap<String, DbValue>,
        key: &String,
    ) -> Result<bool, String> {
        jet_db_row_value(row, key).and_then(|v| v.bool())
    }

    /// D-DBDRIVER1: `.query`/`.query_one`/`.execute` fail with a `DbError`
    /// carrying the driver's message (SQLite's error text) — never the raw SQL.
    #[derive(Clone, Debug, PartialEq)]
    pub struct DbError {
        pub message: String,
    }

    impl super::JetShow for DbError {
        fn jet_show(&self) -> String {
            self.message.clone()
        }
    }

    // ── core.db wire codec ──────────────────────────────────────────────────────
    // The FFI bridge crate (built only when a program uses `jet.db`, Source/FFI.rs)
    // and this always-compiled prelude are two independently built Rust crates —
    // they can't share types, so bind params and result rows cross that boundary as
    // plain `String`s in a small tagged-length wire format (mirrored byte-for-byte
    // in Source/Prelude/Db.rs). A value is `<tag><decimal-length>:<payload-bytes>`;
    // a list is a decimal item count + `:` + that many back-to-back items. Every
    // length is a byte count, so arbitrary text — including an "injection-looking"
    // literal — round-trips exactly with no escaping.
    fn db_encode_tagged(tag: char, payload: &str) -> String {
        format!("{tag}{}:{payload}", payload.len())
    }

    pub fn jet_db_encode_params(params: &Vec<DbValue>) -> String {
        let mut out = String::new();
        out.push_str(&params.len().to_string());
        out.push(':');
        for p in params {
            out.push_str(&match p {
                DbValue::Null => db_encode_tagged('N', ""),
                DbValue::Int(n) => db_encode_tagged('I', &n.to_string()),
                DbValue::Float(f) => db_encode_tagged('F', &f.to_string()),
                DbValue::Text(s) => db_encode_tagged('T', s),
                DbValue::Bool(b) => db_encode_tagged('B', if *b { "1" } else { "0" }),
            });
        }
        out
    }

    fn db_read_tagged(bytes: &[u8], pos: &mut usize) -> Option<(char, String)> {
        let tag = *bytes.get(*pos)? as char;
        *pos += 1;
        let len_start = *pos;
        while *bytes.get(*pos)? != b':' {
            *pos += 1;
        }
        let len: usize = std::str::from_utf8(&bytes[len_start..*pos])
            .ok()?
            .parse()
            .ok()?;
        *pos += 1; // skip ':'
        let payload = std::str::from_utf8(bytes.get(*pos..*pos + len)?)
            .ok()?
            .to_string();
        *pos += len;
        Some((tag, payload))
    }

    fn db_decode_value(tag: char, payload: &str) -> DbValue {
        match tag {
            'I' => DbValue::Int(payload.parse().unwrap_or(0)),
            'F' => DbValue::Float(payload.parse().unwrap_or(0.0)),
            'T' => DbValue::Text(payload.to_string()),
            'B' => DbValue::Bool(payload == "1"),
            _ => DbValue::Null,
        }
    }

    /// Decode the `"O:" + rows`/`"E:" + message` wire produced by `jet_db_query`.
    pub fn jet_db_decode_query_result(
        wire: &str,
    ) -> Result<Vec<std::collections::BTreeMap<String, DbValue>>, DbError> {
        let Some(body) = wire.strip_prefix("O:") else {
            let msg = wire.strip_prefix("E:").unwrap_or(wire);
            return Err(DbError {
                message: msg.to_string(),
            });
        };
        let bytes = body.as_bytes();
        let mut pos = 0usize;
        let Some(colon) = bytes.iter().position(|b| *b == b':') else {
            return Ok(Vec::new());
        };
        let row_count: usize = std::str::from_utf8(&bytes[..colon])
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        pos = colon + 1;
        let mut rows = Vec::with_capacity(row_count);
        for _ in 0..row_count {
            let Some(col_colon) = bytes[pos..].iter().position(|b| *b == b':') else {
                break;
            };
            let col_count: usize = std::str::from_utf8(&bytes[pos..pos + col_colon])
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);
            pos += col_colon + 1;
            let mut row = std::collections::BTreeMap::new();
            for _ in 0..col_count {
                let Some((_, name)) = db_read_tagged(bytes, &mut pos) else {
                    break;
                };
                let Some((vtag, vpayload)) = db_read_tagged(bytes, &mut pos) else {
                    break;
                };
                row.insert(name, db_decode_value(vtag, &vpayload));
            }
            rows.push(row);
        }
        Ok(rows)
    }

    /// Decode the `"O:" + count`/`"E:" + message` wire produced by `jet_db_execute`.
    pub fn jet_db_decode_execute_result(wire: &str) -> Result<i64, DbError> {
        if let Some(n) = wire.strip_prefix("O:") {
            return Ok(n.parse().unwrap_or(0));
        }
        let msg = wire.strip_prefix("E:").unwrap_or(wire);
        Err(DbError {
            message: msg.to_string(),
        })
    }

    pub fn jet_db_params_from_sql(sql: &(String, Vec<String>)) -> Vec<DbValue> {
        sql.1.iter().map(|s| DbValue::Text(s.clone())).collect()
    }

    pub fn jet_db_migration_checksum(steps: &Vec<String>) -> String {
        let mut hash: u64 = 0xcbf29ce484222325;
        for step in steps {
            for b in step.as_bytes() {
                hash ^= *b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            hash ^= 0xff;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{hash:016x}")
    }

    // ── D-DEP-WASM1=A / D-PLUGIN1=B (c81): core.plugin wire helpers ────────────
    // `Plugin.call`/`.call_int` cross the sandboxed Component Model boundary as
    // plain wire text — the always-compiled prelude here and the hidden FFI
    // bridge crate (`Prelude/Plugin.rs`, `jet_plugin_call`) are built
    // independently and share no Rust types, only this tagged-length text
    // (same house style as `jet_db_encode_params`/`jet_db_decode_query_result`
    // above; `pluginw_`-prefixed so nothing here collides with `db_*`).
    fn pluginw_encode_tagged(tag: char, payload: &str) -> String {
        format!("{tag}{}:{payload}", payload.len())
    }

    fn pluginw_read_tagged(bytes: &[u8], pos: &mut usize) -> Option<(char, String)> {
        let tag = *bytes.get(*pos)? as char;
        *pos += 1;
        let len_start = *pos;
        while *bytes.get(*pos)? != b':' {
            *pos += 1;
        }
        let len: usize = std::str::from_utf8(&bytes[len_start..*pos])
            .ok()?
            .parse()
            .ok()?;
        *pos += 1; // skip ':'
        let payload = std::str::from_utf8(bytes.get(*pos..*pos + len)?)
            .ok()?
            .to_string();
        *pos += len;
        Some((tag, payload))
    }

    /// Encode a `[Float]` argument list for `plugin.call(name, args)`.
    pub fn jet_plugin_encode_args_float(args: &Vec<f64>) -> String {
        let mut out = String::new();
        out.push_str(&args.len().to_string());
        out.push(':');
        for a in args {
            out.push_str(&pluginw_encode_tagged('F', &a.to_string()));
        }
        out
    }

    /// Encode an `[Int]` argument list for `plugin.call_int(name, args)`.
    pub fn jet_plugin_encode_args_int(args: &Vec<i64>) -> String {
        let mut out = String::new();
        out.push_str(&args.len().to_string());
        out.push(':');
        for a in args {
            out.push_str(&pluginw_encode_tagged('I', &a.to_string()));
        }
        out
    }

    /// Decode the `"O:<handle>"`/`"E:<message>"` wire produced by
    /// `jet_plugin_load`. Returns the handle, or `0` (the invalid-handle
    /// sentinel, mirroring `jet_db_open`'s style) when the load failed — every
    /// later `.call`/`.call_int` on handle `0` reports "no plugin loaded for
    /// this handle" rather than ever panicking (I2).
    pub fn jet_plugin_load_handle(wire: &str) -> u64 {
        wire.strip_prefix("O:")
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
    }

    /// Decode the `"O:F<len>:<val>"`/`"E:<message>"` wire produced by
    /// `jet_plugin_call` for a `.call` (Float) invocation.
    pub fn jet_plugin_decode_result_float(wire: &str) -> Result<f64, String> {
        let Some(body) = wire.strip_prefix("O:") else {
            return Err(wire.strip_prefix("E:").unwrap_or(wire).to_string());
        };
        let bytes = body.as_bytes();
        let mut pos = 0usize;
        match pluginw_read_tagged(bytes, &mut pos) {
            Some((_, payload)) => payload
                .parse::<f64>()
                .map_err(|_| "plugin returned a malformed Float result".to_string()),
            None => Err("plugin returned a malformed result".to_string()),
        }
    }

    /// Decode the `"O:I<len>:<val>"`/`"E:<message>"` wire produced by
    /// `jet_plugin_call` for a `.call_int` (Int) invocation.
    pub fn jet_plugin_decode_result_int(wire: &str) -> Result<i64, String> {
        let Some(body) = wire.strip_prefix("O:") else {
            return Err(wire.strip_prefix("E:").unwrap_or(wire).to_string());
        };
        let bytes = body.as_bytes();
        let mut pos = 0usize;
        match pluginw_read_tagged(bytes, &mut pos) {
            Some((_, payload)) => payload
                .parse::<i64>()
                .map_err(|_| "plugin returned a malformed Int result".to_string()),
            None => Err("plugin returned a malformed result".to_string()),
        }
    }

    // ── core.encoding: format-agnostic value tree (D-SERDE2 = A) ───────────────
    // The one tree every format adapter speaks. The built-in `@[Codable]` derive
    // (D-ENC1) lowers `encode`/`decode` to walks over this; each adapter turns it
    // into / parses it from wire text. Distinct from the dynamic `Json` enum:
    // `DataTree` preserves field order (ordered `Object`) and keeps Int vs Float.
    #[derive(Clone, Debug, PartialEq)]
    pub enum DataTree {
        Null,
        Bool(bool),
        Int(i64),
        Float(f64),
        Text(String),
        Bytes(Vec<u8>),
        Array(Vec<DataTree>),
        Object(Vec<(String, DataTree)>),
    }

    // D-SERDE2 = A: the decode-side error carries a field path (`order.items[2]`)
    // and a plain reason. Encode is infallible, so no `EncodeError` is minted (I8).
    #[derive(Clone, Debug, PartialEq)]
    pub struct DecodeError {
        pub path: String,
        pub reason: String,
    }

    impl DecodeError {
        pub fn new(reason: impl Into<String>) -> DecodeError {
            DecodeError {
                path: String::new(),
                reason: reason.into(),
            }
        }
        // Prefix a child error with the field/index segment it occurred under.
        pub fn under(seg: &str, mut e: DecodeError) -> DecodeError {
            e.path = if e.path.is_empty() {
                seg.to_string()
            } else if e.path.starts_with('[') {
                format!("{}{}", seg, e.path)
            } else {
                format!("{}.{}", seg, e.path)
            };
            e
        }
    }

    impl super::JetShow for DataTree {
        fn jet_show(&self) -> String {
            render_datatree_json(self, false, 0)
        }
    }

    // D-MIGRATE3=A / D-MIGRATE4=A: decode-time migration transparency plus the
    // runtime engine. `decode_traced<T>` sits beside `decode<T>` on every codec
    // that shares this decode machinery. Decoding a `@PublishedSchema` type
    // with `migration { }` blocks tries the current shape first; on mismatch
    // the type's generated `jet_decode_traced` override detects which
    // historical shape the data's key set matches and walks the step functions
    // forward (oldest matching version → current). Plain `decode` walks the
    // same chain silently; `decode_traced` reports it here — `migrated`,
    // `from` (the source shape's version label), and `steps` (one entry per
    // step applied, "v1->v2" style). Types without migrations keep the trait's
    // default identity path: `migrated` false, `from`/`steps` empty, no
    // per-type code emitted.
    #[derive(Clone, Debug, PartialEq)]
    pub struct MigrationStatus {
        pub migrated: bool,
        pub from: String,
        pub steps: Vec<String>,
    }

    impl MigrationStatus {
        pub fn fresh() -> MigrationStatus {
            MigrationStatus {
                migrated: false,
                from: String::new(),
                steps: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DecodeResult<T> {
        pub value: T,
        pub migration: MigrationStatus,
    }

    /// D-MIGRATE4: the sorted set of top-level object keys of a `DataTree`, used
    /// by a `@PublishedSchema` type's migration chain-walker to detect which
    /// historical shape a decoded record matches. A non-object tree has no keys.
    pub fn jet_datatree_key_set(t: &DataTree) -> std::collections::BTreeSet<String> {
        match t {
            DataTree::Object(pairs) => pairs.iter().map(|(k, _)| k.clone()).collect(),
            _ => std::collections::BTreeSet::new(),
        }
    }

    // D-SERDE-ACCESS=B: dynamic accessor methods on DataTree.
    impl DataTree {
        pub fn field(&self, name: &str) -> Result<DataTree, String> {
            match self {
                DataTree::Object(pairs) => pairs
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| format!("field `{}` not found", name)),
                _ => Err(format!(
                    "expected object, got {}",
                    render_datatree_json(self, false, 0)
                )),
            }
        }
        pub fn at(&self, i: i64) -> Result<DataTree, String> {
            match self {
                DataTree::Array(items) => {
                    let idx = if i < 0 {
                        items.len().wrapping_sub((-i) as usize)
                    } else {
                        i as usize
                    };
                    items
                        .get(idx)
                        .cloned()
                        .ok_or_else(|| format!("index {} out of bounds (len {})", i, items.len()))
                }
                _ => Err(format!(
                    "expected array, got {}",
                    render_datatree_json(self, false, 0)
                )),
            }
        }
        pub fn int(&self) -> Result<i64, String> {
            match self {
                DataTree::Int(n) => Ok(*n),
                _ => Err(format!(
                    "expected int, got {}",
                    render_datatree_json(self, false, 0)
                )),
            }
        }
        pub fn text(&self) -> Result<String, String> {
            match self {
                DataTree::Text(s) => Ok(s.clone()),
                _ => Err(format!(
                    "expected text, got {}",
                    render_datatree_json(self, false, 0)
                )),
            }
        }
        pub fn bool(&self) -> Result<bool, String> {
            match self {
                DataTree::Bool(b) => Ok(*b),
                _ => Err(format!(
                    "expected bool, got {}",
                    render_datatree_json(self, false, 0)
                )),
            }
        }
        pub fn float(&self) -> Result<f64, String> {
            match self {
                DataTree::Float(f) => Ok(*f),
                DataTree::Int(n) => Ok(*n as f64),
                _ => Err(format!(
                    "expected float, got {}",
                    render_datatree_json(self, false, 0)
                )),
            }
        }
    }

    impl super::JetShow for DecodeError {
        fn jet_show(&self) -> String {
            if self.path.is_empty() {
                self.reason.clone()
            } else {
                format!("at `{}`: {}", self.path, self.reason)
            }
        }
    }

    // ── D-SIMD2 / D-LINALG1: built-in math value types ───────────────────────────
    // SIMD lanes + linear-algebra vectors/matrices. The pinned stable rustc has no
    // `std::simd` (portable_simd is unstable), so lane types are a SCALAR-ARRAY
    // fallback: a `[f32; 4]` / `[f64; 2]` newtype with element-wise ops. This is
    // correct and memory-safe by construction (I1) — no intrinsics, no feature gate,
    // no `un`+`safe`. A `std::simd` backend can replace these structs later behind
    // the same surface without touching generated code. Linalg types are column-major
    // F64 arrays. All ops return fresh values (value semantics); `Copy` for ergonomics.

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct F32x4(pub [f32; 4]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct F64x2(pub [f64; 2]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Vec2(pub [f64; 2]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Vec3(pub [f64; 3]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Vec4(pub [f64; 4]);
    // Column-major: element (row r, col c) is `.0[c * N + r]`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Mat3(pub [f64; 9]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Mat4(pub [f64; 16]);

    macro_rules! jet_lane_ops {
        ($T:ident, $E:ty, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] + o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] - o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] * o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Div for $T {
                type Output = $T;
                fn div(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] / o.0[i];
                    }
                    $T(r)
                }
            }
        };
    }
    jet_lane_ops!(F32x4, f32, 4);
    jet_lane_ops!(F64x2, f64, 2);

    macro_rules! jet_vec_ops {
        ($T:ident, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] + o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] - o.0[i];
                    }
                    $T(r)
                }
            }
            // `v * w` is element-wise (Hadamard); the dot/cross products are methods.
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] * o.0[i];
                    }
                    $T(r)
                }
            }
        };
    }
    jet_vec_ops!(Vec2, 2);
    jet_vec_ops!(Vec3, 3);
    jet_vec_ops!(Vec4, 4);

    macro_rules! jet_mat_ops {
        ($T:ident, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..($N * $N) {
                        r[i] = self.0[i] + o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..($N * $N) {
                        r[i] = self.0[i] - o.0[i];
                    }
                    $T(r)
                }
            }
            // `m * n` is matrix multiply (column-major).
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = [0.0f64; $N * $N];
                    for c in 0..$N {
                        for row in 0..$N {
                            let mut acc = 0.0f64;
                            for k in 0..$N {
                                acc += self.0[k * $N + row] * o.0[c * $N + k];
                            }
                            r[c * $N + row] = acc;
                        }
                    }
                    $T(r)
                }
            }
        };
    }
    jet_mat_ops!(Mat3, 3);
    jet_mat_ops!(Mat4, 4);

    // `Mat * Vec` transforms the vector (column-major).
    impl std::ops::Mul<Vec3> for Mat3 {
        type Output = Vec3;
        fn mul(self, v: Vec3) -> Vec3 {
            let mut r = [0.0f64; 3];
            for row in 0..3 {
                let mut a = 0.0f64;
                for k in 0..3 {
                    a += self.0[k * 3 + row] * v.0[k];
                }
                r[row] = a;
            }
            Vec3(r)
        }
    }
    impl std::ops::Mul<Vec4> for Mat4 {
        type Output = Vec4;
        fn mul(self, v: Vec4) -> Vec4 {
            let mut r = [0.0f64; 4];
            for row in 0..4 {
                let mut a = 0.0f64;
                for k in 0..4 {
                    a += self.0[k * 4 + row] * v.0[k];
                }
                r[row] = a;
            }
            Vec4(r)
        }
    }

    impl super::JetShow for F32x4 {
        fn jet_show(&self) -> String {
            format!("F32x4({:?})", self.0)
        }
    }
    impl super::JetShow for F64x2 {
        fn jet_show(&self) -> String {
            format!("F64x2({:?})", self.0)
        }
    }
    impl super::JetShow for Vec2 {
        fn jet_show(&self) -> String {
            format!("Vec2({:?})", self.0)
        }
    }
    impl super::JetShow for Vec3 {
        fn jet_show(&self) -> String {
            format!("Vec3({:?})", self.0)
        }
    }
    impl super::JetShow for Vec4 {
        fn jet_show(&self) -> String {
            format!("Vec4({:?})", self.0)
        }
    }
    impl super::JetShow for Mat3 {
        fn jet_show(&self) -> String {
            format!("Mat3({:?})", self.0)
        }
    }
    impl super::JetShow for Mat4 {
        fn jet_show(&self) -> String {
            format!("Mat4({:?})", self.0)
        }
    }

    pub struct JetTask<T: Send + 'static> {
        handle: Option<super::JetSchedulerJoin<T>>,
        control: std::sync::Arc<super::JetTaskControl>,
    }
    impl<T: Send + 'static> Default for JetTask<T> {
        fn default() -> Self {
            JetTask {
                handle: None,
                control: super::JetTaskControl::new(),
            }
        }
    }
    impl<T: Send + 'static> JetTask<T> {
        pub fn spawn<F: FnOnce() -> T + Send + 'static>(f: F) -> JetTask<T> {
            let inherited_deadline = super::jet_ctx_deadline_ms();
            let control = super::JetTaskControl::new();
            JetTask {
                handle: Some(super::jet_scheduler_spawn_with_control(
                    move || {
                        let _deadline_guard = inherited_deadline.map(super::jet_ctx_push_deadline);
                        f()
                    },
                    control.clone(),
                )),
                control,
            }
        }
        // D-COROUTINE1=A: control-plane hooks on the M:N scheduler substrate.
        pub fn pause(&self) {
            self.control.pause();
        }
        pub fn resume(&self) {
            self.control.resume();
        }
        pub fn cancel(&self) {
            self.control.cancel();
        }
        pub fn trace(&self) -> String {
            let paused = self
                .control
                .paused
                .load(std::sync::atomic::Ordering::Relaxed);
            let cancel = self
                .control
                .cancelled
                .load(std::sync::atomic::Ordering::Relaxed);
            format!("paused={},cancel={}", paused, cancel)
        }
        pub fn join(mut self) -> T {
            super::jet_deadline_check("task join");
            let v = self.handle.take().unwrap().join();
            super::jet_deadline_check("task join");
            v
        }
    }

    /// D-CONCCOMB1=A: join every handle; fail fast and cancel siblings on error.
    pub fn jet_task_all<T: Send + 'static>(tasks: Vec<JetTask<T>>) -> Vec<T> {
        let entries: Vec<_> = tasks
            .into_iter()
            .map(|mut t| {
                (
                    t.handle.take().expect("all: task already joined"),
                    t.control,
                )
            })
            .collect();
        super::jet_scheduler_all(entries)
    }

    /// D-CONCCOMB1=A + D-RACEWIN1: first successful result; cancel siblings via scheduler.
    pub fn jet_task_race<T: Send + 'static>(tasks: Vec<JetTask<T>>) -> T {
        let entries: Vec<_> = tasks
            .into_iter()
            .map(|mut t| {
                (
                    t.handle.take().expect("race: task already joined"),
                    t.control,
                )
            })
            .collect();
        super::jet_scheduler_race(entries)
    }

    /// D-CONCCOMB1=A: first completed result (success or failure path — v1 propagates panic).
    pub fn jet_task_any<T: Send + 'static>(tasks: Vec<JetTask<T>>) -> T {
        let entries: Vec<_> = tasks
            .into_iter()
            .map(|mut t| {
                (
                    t.handle.take().expect("any: task already joined"),
                    t.control,
                )
            })
            .collect();
        super::jet_scheduler_any(entries)
    }

    /// D-CONCSELECT1=A: fluent select builder accumulated at compile time, executed at `.wait()`.
    pub struct JetSelectBuilder<T: Send + 'static> {
        recvs: Vec<JetReceiver<T>>,
        after_values: Vec<(i64, T)>,
    }

    impl<T: Send + 'static> JetSelectBuilder<T> {
        pub fn start() -> JetSelectBuilder<T> {
            JetSelectBuilder {
                recvs: Vec::new(),
                after_values: Vec::new(),
            }
        }
        pub fn recv(mut self, ch: JetReceiver<T>) -> JetSelectBuilder<T> {
            self.recvs.push(ch);
            self
        }
        pub fn after(mut self, ms: i64) -> JetSelectBuilder<T>
        where
            T: Default,
        {
            self.after_values.push((ms, T::default()));
            self
        }
        pub fn after_value(mut self, ms: i64, value: T) -> JetSelectBuilder<T> {
            self.after_values.push((ms, value));
            self
        }
        pub fn read(self, _stream: super::JetTcpStream) -> JetSelectBuilder<T> {
            self
        }
        pub fn wait(self) -> T {
            let recv_refs: Vec<&JetReceiver<T>> = self.recvs.iter().collect();
            jet_select_wait(&recv_refs, self.after_values)
        }
    }

    /// D-CONCSELECT1=A: multiplex channel/timer arms registered by `g.select()`.
    pub fn jet_select_wait<T: Send + 'static>(
        recvs: &[&JetReceiver<T>],
        after_values: Vec<(i64, T)>,
    ) -> T {
        let inners: Vec<_> = recvs.iter().map(|c| c.inner.select_inner()).collect();
        let timers: Vec<u64> = after_values.iter().map(|(ms, _)| (*ms).max(0) as u64).collect();
        match super::jet_scheduler_select(inners, timers) {
            super::JetSelectOutcome::Recv { value, .. } => value,
            super::JetSelectOutcome::After { arm } => after_values
                .into_iter()
                .nth(arm)
                .map(|(_, value)| value)
                .unwrap_or_else(|| {
                    eprintln!("panic: select timer arm missing value");
                    std::process::exit(70);
                }),
            super::JetSelectOutcome::Closed => {
                eprintln!("panic: select closed");
                std::process::exit(70);
            }
        }
    }

    /// D-TUPLE-DESTRUCT1: `tasks.channel<T>()` — mirrors Rust's `mpsc::channel()`:
    /// returns the `(Sender<T>, Receiver<T>)` pair directly (no combined "Channel"
    /// handle, and no `.sender()` method — a second sender is `tx.clone()`).
    pub fn channel<T: Send>() -> (JetSender<T>, JetReceiver<T>) {
        let inner = super::JetSchedulerChannel::new();
        let tx = inner.sender();
        (JetSender { tx }, JetReceiver { inner })
    }

    /// D-TASKRUNTIME1=A: bounded channel; `capacity` is a real memory/backpressure bound.
    pub fn channel_bounded<T: Send>(capacity: i64) -> (JetSender<T>, JetReceiver<T>) {
        let inner = super::JetSchedulerChannel::bounded(capacity.max(1) as usize);
        let tx = inner.sender();
        (JetSender { tx }, JetReceiver { inner })
    }

    /// D-TASKRUNTIME1=A: one-shot timer channel; wakes through the scheduler timer wheel.
    pub fn after(ms: i64) -> JetReceiver<()> {
        let (tx, rx) = channel::<()>();
        let delay = ms.max(0) as u64;
        let _ = super::jet_scheduler_spawn(move || {
            super::jet_scheduler_sleep_ms(delay);
            tx.send(());
        });
        rx
    }

    /// D-TASKRUNTIME1=A: one-shot typed timer channel for select timeout values.
    pub fn after_value<T: Send + 'static>(ms: i64, value: T) -> JetReceiver<T> {
        let (tx, rx) = channel::<T>();
        let delay = ms.max(0) as u64;
        let _ = super::jet_scheduler_spawn(move || {
            super::jet_scheduler_sleep_ms(delay);
            tx.send(value);
        });
        rx
    }

    /// D-TASKRUNTIME1=A: interval timer channel; sends 1, 2, ... until process exit.
    pub fn interval(ms: i64) -> JetReceiver<i64> {
        let (tx, rx) = channel::<i64>();
        let delay = ms.max(1) as u64;
        let _ = std::thread::spawn(move || {
            let mut tick = 1i64;
            loop {
                super::jet_scheduler_sleep_ms(delay);
                if !tx.tx.send(tick) {
                    break;
                }
                tick += 1;
            }
        });
        rx
    }

    pub struct JetReceiver<T> {
        inner: super::JetSchedulerChannel<T>,
    }
    // D-TUPLE-DESTRUCT1: the tuple-destructure bind convention clones each
    // extracted field (`(tx, rx) := tasks.channel<T>()` clones `rx` off the
    // synthesized `(Sender<T>, Receiver<T>)` struct, same as `Sender` below). The
    // underlying scheduler channel is `Arc`-backed and already supports concurrent
    // receivers (the same substrate `g.select()` races multiple receive arms
    // against), so cloning a `Receiver` is a cheap, sound pointer copy — not a
    // single-consumer `std::sync::mpsc::Receiver`.
    impl<T> Clone for JetReceiver<T> {
        fn clone(&self) -> Self {
            JetReceiver {
                inner: self.inner.clone(),
            }
        }
    }
    impl<T: Send> JetReceiver<T> {
        pub fn receive(&self) -> Result<T, Closed> {
            if super::jet_scheduler_task_cancelled() {
                return Err(Closed::Closed);
            }
            if let Some(remaining) = super::jet_deadline_remaining_ms() {
                if remaining <= 0 {
                    super::jet_deadline_exceeded("channel receive");
                }
            }
            match self.inner.receive() {
                Some(v) => {
                    super::jet_deadline_check("channel receive");
                    Ok(v)
                }
                None => Err(Closed::Closed),
            }
        }
    }

    pub struct JetSender<T> {
        tx: super::JetSchedulerSender<T>,
    }
    impl<T: Send> JetSender<T> {
        pub fn send(&self, value: T) {
            let _ = self.tx.send(value);
        }
    }
    impl<T> Clone for JetSender<T> {
        fn clone(&self) -> Self {
            JetSender {
                tx: self.tx.clone(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Closed {
        Closed,
    }

    // D-MEM1 S6 (D-SHARED-API1=A): `Shared<T>` — a lock-guarded shared handle,
    // "a copyable door". `Shared.new(x)` constructs; `.read(f)`/`.edit(f)` run a
    // closure against a read- or write-locked view, the lock scoped to the
    // closure call only (no guard object ever escapes it). Cloning is always a
    // cheap `Arc` clone, never a deep copy of `T` — that's what lets it cross a
    // `tasks.spawn` boundary with no `take`.
    pub struct JetShared<T>(std::sync::Arc<std::sync::RwLock<T>>);
    impl<T> JetShared<T> {
        pub fn new(value: T) -> Self {
            JetShared(std::sync::Arc::new(std::sync::RwLock::new(value)))
        }
        pub fn read<F, R>(&self, f: F) -> R
        where
            F: FnOnce(&T) -> R,
        {
            let guard = self.0.read().unwrap_or_else(|e| e.into_inner());
            f(&*guard)
        }
        pub fn edit<F, R>(&self, f: F) -> R
        where
            F: FnOnce(&mut T) -> R,
        {
            let mut guard = self.0.write().unwrap_or_else(|e| e.into_inner());
            f(&mut *guard)
        }
    }
    impl<T> Clone for JetShared<T> {
        fn clone(&self) -> Self {
            JetShared(self.0.clone())
        }
    }
    // D-MEM1 S6: an opaque-handle placeholder, mirroring `JetTcpListener`'s
    // `JetShow` (Prelude/CoreLib.rs) — `Shared<T>`'s point is the lock-guarded
    // access methods, not a direct print of the handle itself.
    impl<T> super::JetShow for JetShared<T> {
        fn jet_show(&self) -> String {
            "Shared(..)".to_string()
        }
    }

    // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>` — a generational arena. `Id<T>` is
    // a lightweight index+generation handle: plain data, `Copy`, comparable,
    // regardless of whether `T` itself is (it never touches `T` at runtime —
    // hand-written impls below, not `#[derive]`, so no `T: Copy`/`Clone`/`Eq`
    // bound leaks onto every `Id<T>`).
    enum JetPoolSlot<T> {
        Occupied(u32, T),
        Vacant(u32),
    }

    pub struct JetPool<T> {
        slots: Vec<JetPoolSlot<T>>,
        free: Vec<usize>,
    }

    impl<T> JetPool<T> {
        pub fn new() -> Self {
            JetPool {
                slots: Vec::new(),
                free: Vec::new(),
            }
        }

        pub fn add(&mut self, value: T) -> JetId<T> {
            if let Some(idx) = self.free.pop() {
                let gen = match self.slots[idx] {
                    JetPoolSlot::Vacant(g) => g,
                    JetPoolSlot::Occupied(..) => {
                        unreachable!("a free-list slot is always Vacant")
                    }
                };
                self.slots[idx] = JetPoolSlot::Occupied(gen, value);
                return JetId::new(idx as u32, gen);
            }
            let idx = self.slots.len();
            self.slots.push(JetPoolSlot::Occupied(0, value));
            JetId::new(idx as u32, 0)
        }

        /// D-POOLID-API1=A: removes the slot `id` names, bumping its generation
        /// so any other copy of `id` becomes stale — mirrors `Map.remove`'s
        /// `Option<T>` convention (a miss returns `None`, not a panic).
        pub fn remove(&mut self, id: JetId<T>) -> Option<T> {
            let idx = id.index as usize;
            let occupied = matches!(
                self.slots.get(idx),
                Some(JetPoolSlot::Occupied(g, _)) if *g == id.generation
            );
            if !occupied {
                return None;
            }
            let next_gen = id.generation.wrapping_add(1);
            let old = std::mem::replace(&mut self.slots[idx], JetPoolSlot::Vacant(next_gen));
            self.free.push(idx);
            match old {
                JetPoolSlot::Occupied(_, v) => Some(v),
                JetPoolSlot::Vacant(_) => unreachable!("just checked Occupied above"),
            }
        }

        /// A snapshot `Vec` of every live id — small, `Copy` elements, so a
        /// fresh allocation per call is the simplest correct thing (D-MEM1 S6
        /// notes deferred a genuine lazy `Iterator` as unneeded polish).
        pub fn ids(&self) -> Vec<JetId<T>> {
            self.slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| match s {
                    JetPoolSlot::Occupied(g, _) => Some(JetId::new(i as u32, *g)),
                    JetPoolSlot::Vacant(_) => None,
                })
                .collect()
        }
    }
    // D-MEM1 S6: an opaque-handle placeholder, same rationale as `JetShared`'s
    // `JetShow` just above.
    impl<T> super::JetShow for JetPool<T> {
        fn jet_show(&self) -> String {
            format!("Pool({} slots)", self.slots.len())
        }
    }

    pub struct JetId<T> {
        index: u32,
        generation: u32,
        _marker: std::marker::PhantomData<fn() -> T>,
    }
    impl<T> JetId<T> {
        fn new(index: u32, generation: u32) -> Self {
            JetId {
                index,
                generation,
                _marker: std::marker::PhantomData,
            }
        }
    }
    impl<T> Clone for JetId<T> {
        fn clone(&self) -> Self {
            *self
        }
    }
    impl<T> Copy for JetId<T> {}
    impl<T> PartialEq for JetId<T> {
        fn eq(&self, other: &Self) -> bool {
            self.index == other.index && self.generation == other.generation
        }
    }
    impl<T> Eq for JetId<T> {}
    impl<T> std::hash::Hash for JetId<T> {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.index.hash(state);
            self.generation.hash(state);
        }
    }
    impl<T> std::fmt::Debug for JetId<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Id(#{}@{})", self.index, self.generation)
        }
    }
    // D-MEM1 S6: print/interpolation/derived-Debug support — `Id<T>` shows up as
    // an ordinary struct field (`parent: Id<Node>?`), whose containing struct's
    // generated `jet_debug()` calls `.jet_debug()` on every field.
    impl<T> super::JetShow for JetId<T> {
        fn jet_show(&self) -> String {
            format!("Id(#{}@{})", self.index, self.generation)
        }
    }
    impl<T> super::JetDisplay for JetId<T> {
        fn jet_display(&self) -> String {
            format!("Id(#{}@{})", self.index, self.generation)
        }
    }
    impl<T> super::JetDebug for JetId<T> {
        fn jet_debug(&self) -> String {
            format!("Id(#{}@{})", self.index, self.generation)
        }
    }

    /// `pool[id]` read (`Expr::Index`, `IndexKind::Pool`): a generation-checked
    /// clone of `T`. Panics naming the stale-access class on a mismatched or
    /// vacant slot, mirroring the array-out-of-bounds panic precedent
    /// (`jet_index_vec`) — a runtime panic, not a new diagnostic code.
    pub fn jet_pool_get<T: Clone>(pool: &JetPool<T>, id: JetId<T>, file: &str, line: u32) -> T {
        match pool.slots.get(id.index as usize) {
            Some(JetPoolSlot::Occupied(gen, v)) if *gen == id.generation => v.clone(),
            _ => super::jet_panic(
                file,
                line,
                "this Id no longer refers to a live value — its pool slot was removed",
            ),
        }
    }

    /// `pool[id] = v` / `pool[id].field = v` (`LValue::Index` / `LValue::Field`
    /// nested on a `Pool` index): a genuine mutable place, not a value
    /// round-trip — a nested field write edits the real slot. Same stale-access
    /// panic as `jet_pool_get`.
    pub fn jet_pool_get_mut<'a, T>(
        pool: &'a mut JetPool<T>,
        id: JetId<T>,
        file: &str,
        line: u32,
    ) -> &'a mut T {
        let idx = id.index as usize;
        let valid = matches!(
            pool.slots.get(idx),
            Some(JetPoolSlot::Occupied(gen, _)) if *gen == id.generation
        );
        if !valid {
            super::jet_panic(
                file,
                line,
                "this Id no longer refers to a live value — its pool slot was removed",
            );
        }
        match &mut pool.slots[idx] {
            JetPoolSlot::Occupied(_, v) => v,
            JetPoolSlot::Vacant(_) => unreachable!("just checked Occupied above"),
        }
    }

    // ── D-REACT1=B: opt-in reactive runtime (signals / derived / effects) ──────
    // Reactivity is a LIBRARY, not core semantics (option B): ordinary bindings are
    // unchanged; these types are the explicit, opt-in surface. Pure std — no external
    // crate (I6) and no raw-memory tier (interior mutability via Rc/RefCell). Dependency
    // tracking is explicit-by-read: a `.get()` evaluated while an observer (a derived
    // recompute or an effect run) is on the thread-local stack subscribes that
    // observer to the signal. A `.set(v)` re-runs every subscribed observer.
    use std::cell::RefCell;
    use std::rc::Rc;

    type Observer = Rc<dyn Fn()>;

    thread_local! {
        // The stack of observers currently (re)computing. The top is the active one.
        static JET_REACTIVE_OBSERVERS: RefCell<Vec<Observer>> = const { RefCell::new(Vec::new()) };
    }

    fn jet_reactive_active_observer() -> Option<Observer> {
        JET_REACTIVE_OBSERVERS.with(|s| s.borrow().last().cloned())
    }

    fn jet_reactive_run_observed(obs: &Observer, body: &dyn Fn()) {
        JET_REACTIVE_OBSERVERS.with(|s| s.borrow_mut().push(obs.clone()));
        body();
        JET_REACTIVE_OBSERVERS.with(|s| {
            s.borrow_mut().pop();
        });
    }

    struct SignalCell<T> {
        value: T,
        // Subscribers are re-run on set. Held as weak-free Rc closures; an effect or
        // derived keeps its own observer alive, so these stay valid for the run.
        subs: Vec<Observer>,
    }

    pub struct JetSignal<T> {
        cell: Rc<RefCell<SignalCell<T>>>,
    }

    impl<T> Clone for JetSignal<T> {
        fn clone(&self) -> Self {
            JetSignal {
                cell: self.cell.clone(),
            }
        }
    }

    impl<T: Clone> JetSignal<T> {
        pub fn new(initial: T) -> JetSignal<T> {
            JetSignal {
                cell: Rc::new(RefCell::new(SignalCell {
                    value: initial,
                    subs: Vec::new(),
                })),
            }
        }
        pub fn get(&self) -> T {
            if let Some(obs) = jet_reactive_active_observer() {
                let mut c = self.cell.borrow_mut();
                if !c.subs.iter().any(|s| Rc::ptr_eq(s, &obs)) {
                    c.subs.push(obs);
                }
            }
            self.cell.borrow().value.clone()
        }
        pub fn set(&self, value: T) {
            let subs = {
                let mut c = self.cell.borrow_mut();
                c.value = value;
                c.subs.clone()
            };
            for s in subs {
                s();
            }
        }
    }

    // A derived value is itself observable: it holds a current value plus its own
    // subscriber list, so effects (and other deriveds) that read it re-run when it
    // recomputes. The `_observer` it registers with its source signals recomputes the
    // value and then notifies the derived's own subscribers.
    pub struct JetDerived<T> {
        cell: Rc<RefCell<SignalCell<T>>>,
        _observer: Observer,
    }

    impl<T> Clone for JetDerived<T> {
        fn clone(&self) -> Self {
            JetDerived {
                cell: self.cell.clone(),
                _observer: self._observer.clone(),
            }
        }
    }

    impl<T: Clone + 'static> JetDerived<T> {
        pub fn new<F: Fn() -> T + 'static>(compute: F) -> JetDerived<T> {
            let compute = Rc::new(compute);
            let cell: Rc<RefCell<SignalCell<T>>> = Rc::new(RefCell::new(SignalCell {
                value: (compute)(),
                subs: Vec::new(),
            }));
            // The observer recomputes the value, then notifies the derived's own subs.
            let cell_for_obs = cell.clone();
            let compute_for_obs = compute.clone();
            let observer: Observer = Rc::new(move || {
                let v = (compute_for_obs)();
                let subs = {
                    let mut c = cell_for_obs.borrow_mut();
                    c.value = v;
                    c.subs.clone()
                };
                for s in subs {
                    s();
                }
            });
            // Run once under observation to record the source-signal dependency set.
            jet_reactive_run_observed(&observer, &{
                let cell = cell.clone();
                let compute = compute.clone();
                move || {
                    let v = (compute)();
                    cell.borrow_mut().value = v;
                }
            });
            JetDerived {
                cell,
                _observer: observer,
            }
        }
        pub fn get(&self) -> T {
            // Reading a derived inside an observer subscribes that observer to it.
            if let Some(obs) = jet_reactive_active_observer() {
                let mut c = self.cell.borrow_mut();
                if !c.subs.iter().any(|s| Rc::ptr_eq(s, &obs)) {
                    c.subs.push(obs);
                }
            }
            self.cell.borrow().value.clone()
        }
    }

    /// `reactive.effect(body)` — run `body` now, and again whenever a signal it read
    /// changes. The first run records the effect's dependencies; each subscribed
    /// signal then holds an `Rc` to the observer, keeping the effect alive for as long
    /// as a signal it reads is alive (a long-lived reactive sink). An effect that reads
    /// no signal simply runs once.
    pub fn jet_reactive_effect<F: Fn() + 'static>(body: F) {
        let observer: Observer = Rc::new(body);
        let run = observer.clone();
        jet_reactive_run_observed(&observer, &move || {
            run();
        });
    }

    /// D-REACTCORE1: `#Reactive` scope marker — alias for `jet_reactive_effect`.
    pub fn jet_reactive_scope<F: Fn() + 'static>(body: F) {
        jet_reactive_effect(body);
    }

    // D-EVENT1: first-party typed Event/Hook family. Values are ordinary Core
    // handles; the compiler knows their generic payload/result types.
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering};

    static JET_EVENT_NEXT_ID: AtomicU64 = AtomicU64::new(1);

    #[derive(Clone)]
    pub struct JetEventPolicy {
        async_buffer: Option<usize>,
    }

    impl JetEventPolicy {
        pub fn sync() -> Self {
            JetEventPolicy { async_buffer: None }
        }
        pub fn async_buffered(buffer: i64) -> Self {
            JetEventPolicy {
                async_buffer: Some(buffer.max(0) as usize),
            }
        }
    }

    #[derive(Clone)]
    pub struct JetEventTrace {
        delivered: i64,
        queued: i64,
        dropped: i64,
        summary: String,
    }

    impl JetEventTrace {
        pub fn delivered(&self) -> i64 {
            self.delivered
        }
        pub fn queued(&self) -> i64 {
            self.queued
        }
        pub fn dropped(&self) -> i64 {
            self.dropped
        }
        pub fn summary(&self) -> String {
            self.summary.clone()
        }
    }

    #[derive(Clone)]
    pub struct JetSubscription {
        active: Rc<Cell<bool>>,
    }

    impl JetSubscription {
        fn new() -> Self {
            JetSubscription {
                active: Rc::new(Cell::new(true)),
            }
        }
        pub fn unsubscribe(&self) {
            self.active.set(false);
        }
        pub fn active(&self) -> bool {
            self.active.get()
        }
    }

    #[derive(Clone)]
    pub struct JetEventScope {
        subs: Rc<RefCell<Vec<JetSubscription>>>,
    }

    impl JetEventScope {
        pub fn new() -> Self {
            JetEventScope {
                subs: Rc::new(RefCell::new(Vec::new())),
            }
        }
        pub fn track(&self, sub: JetSubscription) -> JetSubscription {
            self.subs.borrow_mut().push(sub.clone());
            sub
        }
        pub fn cancel(&self) {
            for sub in self.subs.borrow().iter() {
                sub.unsubscribe();
            }
        }
        pub fn active_count(&self) -> i64 {
            self.subs.borrow().iter().filter(|s| s.active()).count() as i64
        }
    }

    impl Drop for JetEventScope {
        fn drop(&mut self) {
            if Rc::strong_count(&self.subs) == 1 {
                self.cancel();
            }
        }
    }

    struct JetListener<T> {
        id: u64,
        priority: i64,
        once: bool,
        sub: JetSubscription,
        handler: Rc<dyn Fn(T)>,
    }

    pub struct JetEvent<T: Clone + 'static> {
        policy: JetEventPolicy,
        listeners: Rc<RefCell<Vec<JetListener<T>>>>,
        queue: Rc<RefCell<Vec<T>>>,
        dropped: Rc<Cell<i64>>,
    }

    impl<T: Clone + 'static> Clone for JetEvent<T> {
        fn clone(&self) -> Self {
            JetEvent {
                policy: self.policy.clone(),
                listeners: self.listeners.clone(),
                queue: self.queue.clone(),
                dropped: self.dropped.clone(),
            }
        }
    }

    impl<T: Clone + 'static> JetEvent<T> {
        pub fn new() -> Self {
            Self::with_policy(JetEventPolicy::sync())
        }
        pub fn with_policy(policy: JetEventPolicy) -> Self {
            JetEvent {
                policy,
                listeners: Rc::new(RefCell::new(Vec::new())),
                queue: Rc::new(RefCell::new(Vec::new())),
                dropped: Rc::new(Cell::new(0)),
            }
        }
        pub fn on<F: Fn(T) + 'static>(&self, scope: &JetEventScope, handler: F) -> JetSubscription {
            self.on_priority(scope, 0, handler)
        }
        pub fn once<F: Fn(T) + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.add(scope, 0, true, handler)
        }
        pub fn on_priority<F: Fn(T) + 'static>(
            &self,
            scope: &JetEventScope,
            priority: i64,
            handler: F,
        ) -> JetSubscription {
            self.add(scope, priority, false, handler)
        }
        fn add<F: Fn(T) + 'static>(
            &self,
            scope: &JetEventScope,
            priority: i64,
            once: bool,
            handler: F,
        ) -> JetSubscription {
            let sub = JetSubscription::new();
            self.listeners.borrow_mut().push(JetListener {
                id: JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed),
                priority,
                once,
                sub: sub.clone(),
                handler: Rc::new(handler),
            });
            scope.track(sub)
        }
        pub fn emit(&self, payload: T) -> JetEventTrace {
            self.dispatch(payload, false)
        }
        pub fn emit_async(&self, payload: T) -> JetEventTrace {
            self.dispatch(payload, true)
        }
        fn dispatch(&self, payload: T, queued: bool) -> JetEventTrace {
            let mut queued_count = 0;
            if queued || self.policy.async_buffer.is_some() {
                queued_count = 1;
                if let Some(limit) = self.policy.async_buffer {
                    let mut q = self.queue.borrow_mut();
                    if limit == 0 {
                        self.dropped.set(self.dropped.get() + 1);
                    } else {
                        if q.len() >= limit {
                            q.remove(0);
                            self.dropped.set(self.dropped.get() + 1);
                        }
                        q.push(payload.clone());
                    }
                    q.clear();
                }
            }
            let mut entries: Vec<(i64, u64, bool, JetSubscription, Rc<dyn Fn(T)>)> = self
                .listeners
                .borrow()
                .iter()
                .filter(|l| l.sub.active())
                .map(|l| (l.priority, l.id, l.once, l.sub.clone(), l.handler.clone()))
                .collect();
            entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let mut delivered = 0;
            for (_, _, once, sub, handler) in entries {
                if sub.active() {
                    handler(payload.clone());
                    delivered += 1;
                    if once {
                        sub.unsubscribe();
                    }
                }
            }
            self.listeners.borrow_mut().retain(|l| l.sub.active());
            JetEventTrace {
                delivered,
                queued: queued_count,
                dropped: self.dropped.get(),
                summary: format!(
                    "event delivered={delivered} queued={queued_count} dropped={}",
                    self.dropped.get()
                ),
            }
        }
        pub fn listener_count(&self) -> i64 {
            self.listeners
                .borrow()
                .iter()
                .filter(|l| l.sub.active())
                .count() as i64
        }
        pub fn queued_count(&self) -> i64 {
            self.queue.borrow().len() as i64
        }
        pub fn trace(&self) -> String {
            format!(
                "listeners={} queued={} dropped={}",
                self.listener_count(),
                self.queued_count(),
                self.dropped.get()
            )
        }
    }

    struct JetHookListener<T, R> {
        id: u64,
        priority: i64,
        once: bool,
        sub: JetSubscription,
        handler: Rc<dyn Fn(T) -> R>,
    }

    pub struct JetHook<T: Clone + 'static, R: Clone + 'static> {
        fallback: R,
        listeners: Rc<RefCell<Vec<JetHookListener<T, R>>>>,
    }

    impl<T: Clone + 'static, R: Clone + 'static> Clone for JetHook<T, R> {
        fn clone(&self) -> Self {
            JetHook {
                fallback: self.fallback.clone(),
                listeners: self.listeners.clone(),
            }
        }
    }

    impl<T: Clone + 'static, R: Clone + 'static> JetHook<T, R> {
        pub fn new(fallback: R) -> Self {
            JetHook {
                fallback,
                listeners: Rc::new(RefCell::new(Vec::new())),
            }
        }
        pub fn on<F: Fn(T) -> R + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.on_priority(scope, 0, handler)
        }
        pub fn once<F: Fn(T) -> R + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.add(scope, 0, true, handler)
        }
        pub fn on_priority<F: Fn(T) -> R + 'static>(
            &self,
            scope: &JetEventScope,
            priority: i64,
            handler: F,
        ) -> JetSubscription {
            self.add(scope, priority, false, handler)
        }
        fn add<F: Fn(T) -> R + 'static>(
            &self,
            scope: &JetEventScope,
            priority: i64,
            once: bool,
            handler: F,
        ) -> JetSubscription {
            let sub = JetSubscription::new();
            self.listeners.borrow_mut().push(JetHookListener {
                id: JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed),
                priority,
                once,
                sub: sub.clone(),
                handler: Rc::new(handler),
            });
            scope.track(sub)
        }
        pub fn run(&self, payload: T, fallback: R) -> R {
            let mut entries: Vec<(i64, u64, bool, JetSubscription, Rc<dyn Fn(T) -> R>)> = self
                .listeners
                .borrow()
                .iter()
                .filter(|l| l.sub.active())
                .map(|l| (l.priority, l.id, l.once, l.sub.clone(), l.handler.clone()))
                .collect();
            entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let mut result = if entries.is_empty() {
                fallback
            } else {
                self.fallback.clone()
            };
            for (_, _, once, sub, handler) in entries {
                if sub.active() {
                    result = handler(payload.clone());
                    if once {
                        sub.unsubscribe();
                    }
                }
            }
            self.listeners.borrow_mut().retain(|l| l.sub.active());
            result
        }
        pub fn listener_count(&self) -> i64 {
            self.listeners
                .borrow()
                .iter()
                .filter(|l| l.sub.active())
                .count() as i64
        }
        pub fn trace(&self) -> String {
            format!("hook listeners={}", self.listener_count())
        }
    }

    #[derive(Clone)]
    enum JetWatchTarget {
        Files { root: String },
        Process { pid: i64 },
        Port { host: String, port: i64 },
    }

    type JetWatchSnapshot = std::collections::BTreeMap<String, (u64, i64, bool)>;

    #[derive(Clone)]
    struct JetWatchState {
        target: JetWatchTarget,
        snapshot: JetWatchSnapshot,
        seen_ready: bool,
        active: bool,
    }

    #[derive(Clone)]
    pub struct WatchHandle {
        state: Rc<RefCell<JetWatchState>>,
        event: JetEvent<WatchEvent>,
    }

    #[derive(Clone)]
    pub struct WatchSet {
        handles: Rc<RefCell<Vec<WatchHandle>>>,
    }

    impl WatchHandle {
        pub fn files(path: String) -> Result<Self, IoError> {
            let snapshot = jet_watch_snapshot(&path)?;
            Ok(WatchHandle {
                state: Rc::new(RefCell::new(JetWatchState {
                    target: JetWatchTarget::Files { root: path },
                    snapshot,
                    seen_ready: false,
                    active: true,
                })),
                event: JetEvent::new(),
            })
        }

        pub fn process_pid(pid: i64) -> Self {
            WatchHandle {
                state: Rc::new(RefCell::new(JetWatchState {
                    target: JetWatchTarget::Process { pid },
                    snapshot: JetWatchSnapshot::new(),
                    seen_ready: jet_process_alive(pid),
                    active: true,
                })),
                event: JetEvent::new(),
            }
        }

        pub fn port(host: String, port: i64) -> Self {
            WatchHandle {
                state: Rc::new(RefCell::new(JetWatchState {
                    target: JetWatchTarget::Port { host, port },
                    snapshot: JetWatchSnapshot::new(),
                    seen_ready: false,
                    active: true,
                })),
                event: JetEvent::new(),
            }
        }

        pub fn poll(&self) -> Vec<WatchEvent> {
            let mut state = self.state.borrow_mut();
            if !state.active {
                return Vec::new();
            }
            let target = state.target.clone();
            let events = match target {
                JetWatchTarget::Files { root } => match jet_watch_snapshot(&root) {
                    Ok(next) => {
                        let events = jet_watch_diff(&state.snapshot, &next);
                        state.snapshot = next;
                        events
                    }
                    Err(e) => vec![WatchEvent {
                        domain: "file".to_string(),
                        kind: "Error".to_string(),
                        path: root.clone(),
                        detail: format!("{:?}", e),
                        pid: 0,
                        port: 0,
                    }],
                },
                JetWatchTarget::Process { pid } => {
                    let alive = jet_process_alive(pid);
                    if state.seen_ready && !alive {
                        state.seen_ready = false;
                        vec![WatchEvent {
                            domain: "process".to_string(),
                            kind: "Exited".to_string(),
                            path: String::new(),
                            detail: "process exited".to_string(),
                            pid,
                            port: 0,
                        }]
                    } else if !state.seen_ready && !alive {
                        vec![WatchEvent {
                            domain: "process".to_string(),
                            kind: "Exited".to_string(),
                            path: String::new(),
                            detail: "process is not running".to_string(),
                            pid,
                            port: 0,
                        }]
                    } else {
                        Vec::new()
                    }
                }
                JetWatchTarget::Port { host, port } => {
                    let ready = std::net::TcpStream::connect((host.as_str(), port as u16)).is_ok();
                    if ready && !state.seen_ready {
                        state.seen_ready = true;
                        vec![WatchEvent {
                            domain: "port".to_string(),
                            kind: "Ready".to_string(),
                            path: String::new(),
                            detail: format!("{}:{}", host, port),
                            pid: 0,
                            port,
                        }]
                    } else {
                        Vec::new()
                    }
                }
            };
            drop(state);
            for ev in events.iter().cloned() {
                self.event.emit(ev);
            }
            events
        }

        pub fn events(&self) -> Vec<WatchEvent> {
            self.poll()
        }

        pub fn on<F: Fn(WatchEvent) + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.event.on(scope, handler)
        }

        pub fn once<F: Fn(WatchEvent) + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.event.once(scope, handler)
        }

        pub fn cancel(&self) {
            self.state.borrow_mut().active = false;
        }

        pub fn active(&self) -> bool {
            self.state.borrow().active
        }

        pub fn summary(&self) -> String {
            match &self.state.borrow().target {
                JetWatchTarget::Files { root } => format!("watch file {}", root),
                JetWatchTarget::Process { pid } => format!("watch process {}", pid),
                JetWatchTarget::Port { host, port } => format!("watch port {}:{}", host, port),
            }
        }
    }

    impl WatchSet {
        pub fn new() -> Self {
            WatchSet {
                handles: Rc::new(RefCell::new(Vec::new())),
            }
        }
        pub fn add(&mut self, handle: WatchHandle) {
            self.handles.borrow_mut().push(handle);
        }
        pub fn poll(&self) -> Vec<WatchEvent> {
            let mut out = Vec::new();
            for handle in self.handles.borrow().iter() {
                out.extend(handle.poll());
            }
            out
        }
        pub fn events(&self) -> Vec<WatchEvent> {
            self.poll()
        }
        pub fn summary(&self) -> String {
            format!("watchset handles={}", self.handles.borrow().len())
        }
    }

    fn jet_watch_snapshot(root: &str) -> Result<JetWatchSnapshot, IoError> {
        let mut out = JetWatchSnapshot::new();
        let mut stack = vec![std::path::PathBuf::from(root)];
        while let Some(path) = stack.pop() {
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|e| io_error(path.to_string_lossy().as_ref(), e))?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let path_s = path.to_string_lossy().to_string();
            let is_dir = meta.is_dir();
            out.insert(path_s, (modified, meta.len() as i64, is_dir));
            if is_dir {
                for entry in std::fs::read_dir(&path)
                    .map_err(|e| io_error(path.to_string_lossy().as_ref(), e))?
                {
                    let entry = entry.map_err(|e| io_error(path.to_string_lossy().as_ref(), e))?;
                    stack.push(entry.path());
                }
            }
        }
        Ok(out)
    }

    fn jet_watch_diff(old: &JetWatchSnapshot, new: &JetWatchSnapshot) -> Vec<WatchEvent> {
        let mut out = Vec::new();
        for (path, facts) in new {
            match old.get(path) {
                None => out.push(jet_watch_event("Created", path, facts.2)),
                Some(prev) if prev != facts => out.push(jet_watch_event("Modified", path, facts.2)),
                _ => {}
            }
        }
        for (path, facts) in old {
            if !new.contains_key(path) {
                out.push(jet_watch_event("Removed", path, facts.2));
            }
        }
        out
    }

    fn jet_watch_event(kind: &str, path: &str, is_dir: bool) -> WatchEvent {
        WatchEvent {
            domain: "file".to_string(),
            kind: kind.to_string(),
            path: path.to_string(),
            detail: if is_dir { "dir" } else { "file" }.to_string(),
            pid: 0,
            port: 0,
        }
    }

    fn jet_process_alive(pid: i64) -> bool {
        if pid <= 0 {
            return false;
        }
        #[cfg(target_os = "linux")]
        {
            std::path::Path::new(&format!("/proc/{}", pid)).exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            let current = std::process::id() as i64;
            pid == current
        }
    }

    impl super::JetShow for Closed {
        fn jet_show(&self) -> String {
            "Closed".to_string()
        }
    }

    // D-HONESTNUM1=A: Measurement<T> — a value paired with its standard uncertainty.
    // Arithmetic propagates uncertainty using the standard quadrature rules.
    // Only `JetMeasurement<f64>` (Float) is exposed to Jet programs.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct JetMeasurement<T: Copy> {
        value: T,
        uncertainty: T,
    }

    impl JetMeasurement<f64> {
        pub fn new(value: f64, uncertainty: f64) -> Self {
            JetMeasurement { value, uncertainty }
        }
        pub fn value(&self) -> f64 {
            self.value
        }
        pub fn uncertainty(&self) -> f64 {
            self.uncertainty
        }
        // Addition / subtraction: σ_z = sqrt(σ_a² + σ_b²)
        pub fn add(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value + other.value,
                uncertainty: (self.uncertainty * self.uncertainty
                    + other.uncertainty * other.uncertainty)
                    .sqrt(),
            }
        }
        pub fn sub(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value - other.value,
                uncertainty: (self.uncertainty * self.uncertainty
                    + other.uncertainty * other.uncertainty)
                    .sqrt(),
            }
        }
        // Multiplication: σ_z = sqrt((b·σ_a)² + (a·σ_b)²)
        pub fn mul(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value * other.value,
                uncertainty: ((other.value * self.uncertainty).powi(2)
                    + (self.value * other.uncertainty).powi(2))
                .sqrt(),
            }
        }
        // Division: σ_z = sqrt((σ_a/b)² + (a·σ_b/b²)²)
        pub fn div(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value / other.value,
                uncertainty: ((self.uncertainty / other.value).powi(2)
                    + (self.value * other.uncertainty / (other.value * other.value)).powi(2))
                .sqrt(),
            }
        }
    }

    impl super::JetShow for JetMeasurement<f64> {
        fn jet_show(&self) -> String {
            format!("{:?} \u{00b1} {:?}", self.value, self.uncertainty)
        }
    }

    pub fn io_error(path: &str, e: std::io::Error) -> IoError {
        match e.kind() {
            std::io::ErrorKind::NotFound => IoError::NotFound {
                path: path.to_string(),
            },
            std::io::ErrorKind::PermissionDenied => IoError::PermissionDenied {
                path: path.to_string(),
            },
            _ => IoError::Other {
                message: e.to_string(),
            },
        }
    }

    pub fn parse_json(text: &str) -> Result<Json, JsonError> {
        let mut p = JsonParser {
            chars: text.chars().collect(),
            pos: 0,
        };
        let v = p.value()?;
        p.ws();
        if p.pos != p.chars.len() {
            return Err(p.err("extra text after JSON value"));
        }
        Ok(v)
    }

    pub fn render_json(j: &Json, pretty: bool, depth: usize) -> String {
        match j {
            Json::Null => "null".to_string(),
            Json::Boolean(b) => b.to_string(),
            Json::Number(n) => format!("{:?}", n),
            Json::Text(s) => quote_json(s),
            Json::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> =
                        items.iter().map(|x| render_json(x, false, depth)).collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            Json::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{}:{}", quote_json(k), render_json(v, false, depth)))
                        .collect();
                    return format!("{{{}}}", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}{}: {}",
                            pad,
                            quote_json(k),
                            render_json(v, true, depth + 1)
                        )
                    })
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }

    fn quote_json(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\u{0008}' => out.push_str("\\b"),
                '\u{000c}' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    // Render a DataTree as JSON, preserving Object field order. Int prints with no
    // decimal (`5`), Float keeps its decimal (`5.0`); Bytes render as a number array.
    pub fn render_datatree_json(t: &DataTree, pretty: bool, depth: usize) -> String {
        match t {
            DataTree::Null => "null".to_string(),
            DataTree::Bool(b) => b.to_string(),
            DataTree::Int(n) => format!("{}", n),
            DataTree::Float(f) => format!("{:?}", f),
            DataTree::Text(s) => quote_json(s),
            DataTree::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                format!("[{}]", parts.join(","))
            }
            DataTree::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = items
                        .iter()
                        .map(|x| render_datatree_json(x, false, depth))
                        .collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_datatree_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            DataTree::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| {
                            format!(
                                "{}:{}",
                                quote_json(k),
                                render_datatree_json(v, false, depth)
                            )
                        })
                        .collect();
                    return format!("{{{}}}", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}{}: {}",
                            pad,
                            quote_json(k),
                            render_datatree_json(v, true, depth + 1)
                        )
                    })
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }

    // Json (dynamic, BTreeMap-keyed) → DataTree. Numbers that are integral collapse
    // to `Int`, so a round-trip through JSON keeps `5` an Int.
    pub fn datatree_from_json(j: &Json) -> DataTree {
        match j {
            Json::Null => DataTree::Null,
            Json::Boolean(b) => DataTree::Bool(*b),
            Json::Number(n) => {
                if n.fract() == 0.0
                    && n.is_finite()
                    && *n >= i64::MIN as f64
                    && *n <= i64::MAX as f64
                {
                    DataTree::Int(*n as i64)
                } else {
                    DataTree::Float(*n)
                }
            }
            Json::Text(s) => DataTree::Text(s.clone()),
            Json::Array(items) => DataTree::Array(items.iter().map(datatree_from_json).collect()),
            Json::Object(m) => DataTree::Object(
                m.iter()
                    .map(|(k, v)| (k.clone(), datatree_from_json(v)))
                    .collect(),
            ),
        }
    }

    // A short kind name for decode error messages.
    pub fn datatree_kind(t: &DataTree) -> &'static str {
        match t {
            DataTree::Null => "null",
            DataTree::Bool(_) => "Bool",
            DataTree::Int(_) => "Int",
            DataTree::Float(_) => "Float",
            DataTree::Text(_) => "Text",
            DataTree::Bytes(_) => "Bytes",
            DataTree::Array(_) => "a list",
            DataTree::Object(_) => "an object",
        }
    }

    // Look up a key in an ordered Object.
    pub fn datatree_get<'a>(t: &'a DataTree, key: &str) -> Option<&'a DataTree> {
        match t {
            DataTree::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    struct JsonParser {
        chars: Vec<char>,
        pos: usize,
    }

    impl JsonParser {
        fn err(&self, msg: &str) -> JsonError {
            let line = self.chars[..self.pos.min(self.chars.len())]
                .iter()
                .filter(|c| **c == '\n')
                .count() as i64
                + 1;
            JsonError {
                line,
                message: msg.to_string(),
            }
        }

        fn ws(&mut self) {
            while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
                self.pos += 1;
            }
        }

        fn value(&mut self) -> Result<Json, JsonError> {
            self.ws();
            match self.peek() {
                Some('n') => self.word("null", Json::Null),
                Some('t') => self.word("true", Json::Boolean(true)),
                Some('f') => self.word("false", Json::Boolean(false)),
                Some('"') => Ok(Json::Text(self.string()?)),
                Some('[') => self.array(),
                Some('{') => self.object(),
                Some('-') | Some('0'..='9') => self.number(),
                _ => Err(self.err("expected a JSON value")),
            }
        }

        fn peek(&self) -> Option<char> {
            self.chars.get(self.pos).copied()
        }

        fn word(&mut self, w: &str, v: Json) -> Result<Json, JsonError> {
            for ch in w.chars() {
                if self.peek() != Some(ch) {
                    return Err(self.err("expected a JSON word"));
                }
                self.pos += 1;
            }
            Ok(v)
        }

        fn string(&mut self) -> Result<String, JsonError> {
            if self.peek() != Some('"') {
                return Err(self.err("expected quoted text"));
            }
            self.pos += 1;
            let mut out = String::new();
            while let Some(c) = self.peek() {
                self.pos += 1;
                match c {
                    '"' => return Ok(out),
                    '\\' => {
                        let Some(e) = self.peek() else {
                            return Err(self.err("unfinished escape"));
                        };
                        self.pos += 1;
                        match e {
                            '"' => out.push('"'),
                            '\\' => out.push('\\'),
                            '/' => out.push('/'),
                            'b' => out.push('\u{0008}'),
                            'f' => out.push('\u{000c}'),
                            'n' => out.push('\n'),
                            'r' => out.push('\r'),
                            't' => out.push('\t'),
                            'u' => self.unicode_escape(&mut out)?,
                            _ => return Err(self.err("invalid escape in string")),
                        }
                    }
                    c if (c as u32) < 0x20 => return Err(self.err("control character in string")),
                    other => out.push(other),
                }
            }
            Err(self.err("missing closing quote"))
        }

        // A `\uXXXX` escape, already past the `u`. Combines a high+low surrogate
        // pair into one code point; rejects a lone or malformed surrogate.
        fn unicode_escape(&mut self, out: &mut String) -> Result<(), JsonError> {
            let cp = self.hex4()?;
            if (0xD800..=0xDBFF).contains(&cp) {
                if self.peek() != Some('\\') {
                    return Err(self.err("unpaired surrogate in string"));
                }
                self.pos += 1;
                if self.peek() != Some('u') {
                    return Err(self.err("unpaired surrogate in string"));
                }
                self.pos += 1;
                let lo = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&lo) {
                    return Err(self.err("unpaired surrogate in string"));
                }
                let combined = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                match char::from_u32(combined) {
                    Some(ch) => out.push(ch),
                    None => return Err(self.err("invalid unicode escape")),
                }
            } else if (0xDC00..=0xDFFF).contains(&cp) {
                return Err(self.err("unpaired surrogate in string"));
            } else {
                match char::from_u32(cp) {
                    Some(ch) => out.push(ch),
                    None => return Err(self.err("invalid unicode escape")),
                }
            }
            Ok(())
        }

        fn hex4(&mut self) -> Result<u32, JsonError> {
            let mut v = 0u32;
            for _ in 0..4 {
                let Some(c) = self.peek() else {
                    return Err(self.err("truncated unicode escape"));
                };
                let d = c
                    .to_digit(16)
                    .ok_or_else(|| self.err("invalid unicode escape"))?;
                v = v * 16 + d;
                self.pos += 1;
            }
            Ok(v)
        }

        fn number(&mut self) -> Result<Json, JsonError> {
            let start = self.pos;
            if self.peek() == Some('-') {
                self.pos += 1;
            }
            // Integer part: `0` alone, or a non-zero digit then more digits.
            match self.peek() {
                Some('0') => self.pos += 1,
                Some('1'..='9') => {
                    self.pos += 1;
                    while matches!(self.peek(), Some('0'..='9')) {
                        self.pos += 1;
                    }
                }
                _ => return Err(self.err("bad number")),
            }
            // Fraction: a `.` must be followed by at least one digit.
            if self.peek() == Some('.') {
                self.pos += 1;
                if !matches!(self.peek(), Some('0'..='9')) {
                    return Err(self.err("bad number"));
                }
                while matches!(self.peek(), Some('0'..='9')) {
                    self.pos += 1;
                }
            }
            // Exponent: `e`/`E`, optional sign, at least one digit.
            if matches!(self.peek(), Some('e') | Some('E')) {
                self.pos += 1;
                if matches!(self.peek(), Some('+') | Some('-')) {
                    self.pos += 1;
                }
                if !matches!(self.peek(), Some('0'..='9')) {
                    return Err(self.err("bad number"));
                }
                while matches!(self.peek(), Some('0'..='9')) {
                    self.pos += 1;
                }
            }
            let s: String = self.chars[start..self.pos].iter().collect();
            match s.parse::<f64>() {
                Ok(n) => Ok(Json::Number(n)),
                Err(_) => Err(self.err("bad number")),
            }
        }

        fn array(&mut self) -> Result<Json, JsonError> {
            self.pos += 1;
            let mut out = Vec::new();
            loop {
                self.ws();
                if self.peek() == Some(']') {
                    self.pos += 1;
                    return Ok(Json::Array(out));
                }
                out.push(self.value()?);
                self.ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some(']') => {}
                    _ => return Err(self.err("expected `,` or `]`")),
                }
            }
        }

        fn object(&mut self) -> Result<Json, JsonError> {
            self.pos += 1;
            let mut out = std::collections::BTreeMap::new();
            loop {
                self.ws();
                if self.peek() == Some('}') {
                    self.pos += 1;
                    return Ok(Json::Object(out));
                }
                let key = self.string()?;
                self.ws();
                if self.peek() != Some(':') {
                    return Err(self.err("expected `:` after object key"));
                }
                self.pos += 1;
                let value = self.value()?;
                out.insert(key, value);
                self.ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some('}') => {}
                    _ => return Err(self.err("expected `,` or `}`")),
                }
            }
        }
    }

    // ── core.encoding.toml: full TOML 1.0 → DataTree (D-ENC-DYN1=A+, c152) ────────
    // A complete std-only TOML 1.0 parser (ported from the compiler's
    // Source/Jetpack/TOML.rs, which the emitted prelude cannot reach) that lowers a
    // document onto the one rich `DataTree`. Strings (every escape + multi-line),
    // integers in every base, floats incl. inf/nan, booleans, datetimes (kept raw),
    // arrays, inline tables, dotted keys, `[table]` headers, and `[[array-of-tables]]`.
    pub mod toml {
        use super::DataTree;

        #[derive(Clone, Debug, PartialEq)]
        pub enum Value {
            String(String),
            Integer(i64),
            Float(f64),
            Boolean(bool),
            Datetime(String),
            Array(Vec<Value>),
            InlineTable(Vec<(String, Value)>),
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum Item {
            Header { path: Vec<String>, array: bool },
            KeyVal { path: Vec<String>, value: Value },
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct ParseError {
            pub line: usize,
            pub message: String,
        }

        pub fn parse_to_tree(raw: &str) -> Result<DataTree, ParseError> {
            let mut p = Parser {
                chars: raw.chars().collect(),
                pos: 0,
                line: 1,
            };
            let mut items = Vec::new();
            loop {
                p.skip_between_statements();
                if p.peek().is_none() {
                    break;
                }
                match p.statement()? {
                    Some(item) => items.push(item),
                    None => {}
                }
            }
            Ok(assemble(items))
        }

        // ── Assembly: a flat Item list → a nested ordered `DataTree::Object` ──────
        fn assemble(items: Vec<Item>) -> DataTree {
            let mut root = DataTree::Object(Vec::new());
            let mut current: Vec<String> = Vec::new();
            for item in items {
                match item {
                    Item::Header { path, array } => {
                        if array {
                            push_array_table(&mut root, &path);
                        } else {
                            // Ensure the table exists.
                            let _ = table_at(&mut root, &path);
                        }
                        current = path;
                    }
                    Item::KeyVal { path, value } => {
                        set_key(&mut root, &current, &path, value_to_tree(value));
                    }
                }
            }
            root
        }

        // Navigate to (creating along the way) the table at `path`. When a segment is
        // an array-of-tables, descend into its LAST element.
        fn table_at<'a>(mut node: &'a mut DataTree, path: &[String]) -> &'a mut DataTree {
            for seg in path {
                node = child_table_mut(node, seg);
            }
            node
        }

        fn child_table_mut<'a>(node: &'a mut DataTree, seg: &str) -> &'a mut DataTree {
            let entries = match node {
                DataTree::Object(entries) => entries,
                other => return other,
            };
            let idx = match entries.iter().position(|(k, _)| k == seg) {
                Some(i) => i,
                None => {
                    entries.push((seg.to_string(), DataTree::Object(Vec::new())));
                    entries.len() - 1
                }
            };
            // An existing array-of-tables: descend into its last element. Decide the
            // target index immutably first, then take exactly one mutable borrow per
            // branch of a match on the (non-borrowing) `Option` — sidesteps the NLL snag.
            let arr_last: Option<usize> = match &entries[idx].1 {
                DataTree::Array(arr) if !arr.is_empty() => Some(arr.len() - 1),
                _ => None,
            };
            match arr_last {
                Some(n) => match &mut entries[idx].1 {
                    DataTree::Array(arr) => &mut arr[n],
                    other => other,
                },
                None => &mut entries[idx].1,
            }
        }

        fn push_array_table(root: &mut DataTree, path: &[String]) {
            let (parent_path, last) = path.split_at(path.len() - 1);
            let parent = table_at(root, parent_path);
            if let DataTree::Object(entries) = parent {
                let idx = match entries.iter().position(|(k, _)| k == &last[0]) {
                    Some(i) => i,
                    None => {
                        entries.push((last[0].clone(), DataTree::Array(Vec::new())));
                        entries.len() - 1
                    }
                };
                if let DataTree::Array(arr) = &mut entries[idx].1 {
                    arr.push(DataTree::Object(Vec::new()));
                }
            }
        }

        fn set_key(root: &mut DataTree, current: &[String], key_path: &[String], value: DataTree) {
            let mut full: Vec<String> = current.to_vec();
            full.extend_from_slice(&key_path[..key_path.len() - 1]);
            let table = table_at(root, &full);
            let fk = &key_path[key_path.len() - 1];
            if let DataTree::Object(entries) = table {
                if let Some(slot) = entries.iter_mut().find(|(k, _)| k == fk) {
                    slot.1 = value;
                } else {
                    entries.push((fk.clone(), value));
                }
            }
        }

        fn value_to_tree(v: Value) -> DataTree {
            match v {
                Value::String(s) => DataTree::Text(s),
                Value::Integer(n) => DataTree::Int(n),
                Value::Float(f) => DataTree::Float(f),
                Value::Boolean(b) => DataTree::Bool(b),
                Value::Datetime(s) => DataTree::Text(s),
                Value::Array(xs) => DataTree::Array(xs.into_iter().map(value_to_tree).collect()),
                Value::InlineTable(es) => {
                    DataTree::Object(es.into_iter().map(|(k, v)| (k, value_to_tree(v))).collect())
                }
            }
        }

        // ── Render: a `DataTree` → TOML text (nested headers, arrays-of-tables) ───
        pub fn render(t: &DataTree) -> String {
            let mut out = String::new();
            render_table(t, &[], &mut out);
            out.trim_end().to_string()
        }

        fn is_table(v: &DataTree) -> bool {
            matches!(v, DataTree::Object(_))
        }
        fn is_array_of_tables(v: &DataTree) -> bool {
            matches!(v, DataTree::Array(arr)
                if !arr.is_empty() && arr.iter().all(|e| matches!(e, DataTree::Object(_))))
        }

        fn render_table(t: &DataTree, path: &[String], out: &mut String) {
            if let DataTree::Object(entries) = t {
                for (k, v) in entries {
                    if !is_table(v) && !is_array_of_tables(v) {
                        out.push_str(&format!("{} = {}\n", k, render_value(v)));
                    }
                }
                for (k, v) in entries {
                    if is_table(v) {
                        let mut p = path.to_vec();
                        p.push(k.clone());
                        out.push_str(&format!("\n[{}]\n", p.join(".")));
                        render_table(v, &p, out);
                    } else if is_array_of_tables(v) {
                        let mut p = path.to_vec();
                        p.push(k.clone());
                        if let DataTree::Array(arr) = v {
                            for elem in arr {
                                out.push_str(&format!("\n[[{}]]\n", p.join(".")));
                                render_table(elem, &p, out);
                            }
                        }
                    }
                }
            }
        }

        fn render_value(v: &DataTree) -> String {
            match v {
                DataTree::Null => "\"\"".to_string(),
                DataTree::Bool(b) => b.to_string(),
                DataTree::Int(n) => n.to_string(),
                DataTree::Float(f) => format!("{:?}", f),
                DataTree::Text(s) => super::quote_json(s),
                DataTree::Bytes(bs) => {
                    let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                    format!("[{}]", parts.join(", "))
                }
                DataTree::Array(items) => {
                    let parts: Vec<String> = items.iter().map(render_value).collect();
                    format!("[{}]", parts.join(", "))
                }
                // An inline (non-header) object renders as a TOML inline table.
                DataTree::Object(entries) => {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, val)| format!("{} = {}", k, render_value(val)))
                        .collect();
                    format!("{{ {} }}", parts.join(", "))
                }
            }
        }

        struct Parser {
            chars: Vec<char>,
            pos: usize,
            line: usize,
        }

        impl Parser {
            fn peek(&self) -> Option<char> {
                self.chars.get(self.pos).copied()
            }
            fn peek_at(&self, n: usize) -> Option<char> {
                self.chars.get(self.pos + n).copied()
            }
            fn bump(&mut self) -> Option<char> {
                let c = self.peek()?;
                self.pos += 1;
                if c == '\n' {
                    self.line += 1;
                }
                Some(c)
            }
            fn err(&self, message: impl Into<String>) -> ParseError {
                ParseError {
                    line: self.line,
                    message: message.into(),
                }
            }
            fn skip_inline_ws(&mut self) {
                while matches!(self.peek(), Some(' ' | '\t')) {
                    self.pos += 1;
                }
            }
            fn skip_between_statements(&mut self) {
                loop {
                    match self.peek() {
                        Some(' ' | '\t' | '\r' | '\n') => {
                            self.bump();
                        }
                        Some('#') => self.skip_comment(),
                        _ => break,
                    }
                }
            }
            fn skip_comment(&mut self) {
                while let Some(c) = self.peek() {
                    if c == '\n' {
                        break;
                    }
                    self.pos += 1;
                }
            }
            fn finish_line(&mut self) -> Result<(), ParseError> {
                self.skip_inline_ws();
                if self.peek() == Some('#') {
                    self.skip_comment();
                }
                match self.peek() {
                    None | Some('\n') | Some('\r') => Ok(()),
                    Some(c) => Err(self.err(format!("unexpected `{c}` after value"))),
                }
            }
            fn statement(&mut self) -> Result<Option<Item>, ParseError> {
                match self.peek() {
                    Some('[') => self.header().map(Some),
                    _ => self.key_value().map(Some),
                }
            }
            fn header(&mut self) -> Result<Item, ParseError> {
                self.bump(); // first '['
                let array = self.peek() == Some('[');
                if array {
                    self.bump();
                }
                self.skip_inline_ws();
                let path = self.key_path()?;
                self.skip_inline_ws();
                if self.peek() != Some(']') {
                    return Err(self.err("expected `]` to close a table header"));
                }
                self.bump();
                if array {
                    if self.peek() != Some(']') {
                        return Err(self.err("expected `]]` to close an array-of-tables header"));
                    }
                    self.bump();
                }
                if path.is_empty() {
                    return Err(self.err("a table header must name a table"));
                }
                self.finish_line()?;
                Ok(Item::Header { path, array })
            }
            fn key_value(&mut self) -> Result<Item, ParseError> {
                let path = self.key_path()?;
                if path.is_empty() {
                    return Err(self.err("expected a key"));
                }
                self.skip_inline_ws();
                if self.peek() != Some('=') {
                    return Err(self.err(format!("expected `=` after key `{}`", path.join("."))));
                }
                self.bump();
                self.skip_inline_ws();
                let value = self.value()?;
                self.finish_line()?;
                Ok(Item::KeyVal { path, value })
            }
            fn key_path(&mut self) -> Result<Vec<String>, ParseError> {
                let mut path = Vec::new();
                loop {
                    self.skip_inline_ws();
                    path.push(self.simple_key()?);
                    self.skip_inline_ws();
                    if self.peek() == Some('.') {
                        self.bump();
                    } else {
                        break;
                    }
                }
                Ok(path)
            }
            fn simple_key(&mut self) -> Result<String, ParseError> {
                match self.peek() {
                    Some('"') => self.basic_string(),
                    Some('\'') => self.literal_string(),
                    Some(c) if is_bare_key_char(c) => {
                        let mut s = String::new();
                        while let Some(c) = self.peek() {
                            if is_bare_key_char(c) {
                                s.push(c);
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                        Ok(s)
                    }
                    Some(c) => Err(self.err(format!("`{c}` is not a valid key character"))),
                    None => Err(self.err("expected a key")),
                }
            }
            fn value(&mut self) -> Result<Value, ParseError> {
                match self.peek() {
                    Some('"') => Ok(Value::String(self.basic_string()?)),
                    Some('\'') => Ok(Value::String(self.literal_string()?)),
                    Some('[') => self.array(),
                    Some('{') => self.inline_table(),
                    Some('t') | Some('f') => self.boolean(),
                    Some('+') | Some('-') | Some('0'..='9') | Some('i') | Some('n') => {
                        self.number_or_datetime()
                    }
                    Some(c) => Err(self.err(format!("`{c}` does not start a valid value"))),
                    None => Err(self.err("expected a value")),
                }
            }
            fn boolean(&mut self) -> Result<Value, ParseError> {
                if self.try_keyword("true") {
                    Ok(Value::Boolean(true))
                } else if self.try_keyword("false") {
                    Ok(Value::Boolean(false))
                } else {
                    Err(self.err("expected `true` or `false`"))
                }
            }
            fn try_keyword(&mut self, kw: &str) -> bool {
                let chars: Vec<char> = kw.chars().collect();
                for (i, c) in chars.iter().enumerate() {
                    if self.peek_at(i) != Some(*c) {
                        return false;
                    }
                }
                if let Some(after) = self.peek_at(chars.len()) {
                    if is_bare_key_char(after) || after == '.' {
                        return false;
                    }
                }
                for _ in 0..chars.len() {
                    self.bump();
                }
                true
            }
            fn basic_string(&mut self) -> Result<String, ParseError> {
                if self.peek() == Some('"')
                    && self.peek_at(1) == Some('"')
                    && self.peek_at(2) == Some('"')
                {
                    return self.multiline_basic_string();
                }
                self.bump();
                let mut out = String::new();
                loop {
                    match self.bump() {
                        None | Some('\n') => return Err(self.err("unterminated string")),
                        Some('"') => return Ok(out),
                        Some('\\') => out.push(self.string_escape()?),
                        Some(c) if (c as u32) < 0x20 => {
                            return Err(self.err("control character in string"))
                        }
                        Some(c) => out.push(c),
                    }
                }
            }
            fn multiline_basic_string(&mut self) -> Result<String, ParseError> {
                self.bump();
                self.bump();
                self.bump();
                if self.peek() == Some('\r') {
                    self.bump();
                }
                if self.peek() == Some('\n') {
                    self.bump();
                }
                let mut out = String::new();
                loop {
                    if self.peek() == Some('"')
                        && self.peek_at(1) == Some('"')
                        && self.peek_at(2) == Some('"')
                    {
                        self.bump();
                        self.bump();
                        self.bump();
                        return Ok(out);
                    }
                    match self.bump() {
                        None => return Err(self.err("unterminated multi-line string")),
                        Some('\\') => {
                            if matches!(
                                self.peek(),
                                Some('\n') | Some('\r') | Some(' ') | Some('\t')
                            ) {
                                let mut sawline = false;
                                let save = self.pos;
                                let saveline = self.line;
                                while matches!(
                                    self.peek(),
                                    Some(' ') | Some('\t') | Some('\r') | Some('\n')
                                ) {
                                    if self.peek() == Some('\n') {
                                        sawline = true;
                                    }
                                    self.bump();
                                }
                                if !sawline {
                                    self.pos = save;
                                    self.line = saveline;
                                    out.push(self.string_escape()?);
                                }
                            } else {
                                out.push(self.string_escape()?);
                            }
                        }
                        Some(c) => out.push(c),
                    }
                }
            }
            fn string_escape(&mut self) -> Result<char, ParseError> {
                match self.bump() {
                    Some('"') => Ok('"'),
                    Some('\\') => Ok('\\'),
                    Some('b') => Ok('\u{0008}'),
                    Some('f') => Ok('\u{000c}'),
                    Some('n') => Ok('\n'),
                    Some('r') => Ok('\r'),
                    Some('t') => Ok('\t'),
                    Some('u') => self.unicode_escape(4),
                    Some('U') => self.unicode_escape(8),
                    Some(c) => Err(self.err(format!("invalid escape `\\{c}`"))),
                    None => Err(self.err("unterminated escape")),
                }
            }
            fn unicode_escape(&mut self, n: usize) -> Result<char, ParseError> {
                let mut v = 0u32;
                for _ in 0..n {
                    let Some(c) = self.peek() else {
                        return Err(self.err("truncated unicode escape"));
                    };
                    let Some(d) = c.to_digit(16) else {
                        return Err(self.err("invalid unicode escape"));
                    };
                    v = v * 16 + d;
                    self.pos += 1;
                }
                char::from_u32(v).ok_or_else(|| self.err("invalid unicode scalar value"))
            }
            fn literal_string(&mut self) -> Result<String, ParseError> {
                if self.peek() == Some('\'')
                    && self.peek_at(1) == Some('\'')
                    && self.peek_at(2) == Some('\'')
                {
                    return self.multiline_literal_string();
                }
                self.bump();
                let mut out = String::new();
                loop {
                    match self.bump() {
                        None | Some('\n') => return Err(self.err("unterminated literal string")),
                        Some('\'') => return Ok(out),
                        Some(c) => out.push(c),
                    }
                }
            }
            fn multiline_literal_string(&mut self) -> Result<String, ParseError> {
                self.bump();
                self.bump();
                self.bump();
                if self.peek() == Some('\r') {
                    self.bump();
                }
                if self.peek() == Some('\n') {
                    self.bump();
                }
                let mut out = String::new();
                loop {
                    if self.peek() == Some('\'')
                        && self.peek_at(1) == Some('\'')
                        && self.peek_at(2) == Some('\'')
                    {
                        self.bump();
                        self.bump();
                        self.bump();
                        return Ok(out);
                    }
                    match self.bump() {
                        None => return Err(self.err("unterminated multi-line literal string")),
                        Some(c) => out.push(c),
                    }
                }
            }
            fn array(&mut self) -> Result<Value, ParseError> {
                self.bump();
                let mut items = Vec::new();
                loop {
                    self.skip_ws_newlines_comments();
                    match self.peek() {
                        Some(']') => {
                            self.bump();
                            return Ok(Value::Array(items));
                        }
                        None => return Err(self.err("unterminated array")),
                        _ => {}
                    }
                    items.push(self.value()?);
                    self.skip_ws_newlines_comments();
                    match self.peek() {
                        Some(',') => {
                            self.bump();
                        }
                        Some(']') => {
                            self.bump();
                            return Ok(Value::Array(items));
                        }
                        Some(c) => {
                            return Err(
                                self.err(format!("expected `,` or `]` in array, found `{c}`"))
                            )
                        }
                        None => return Err(self.err("unterminated array")),
                    }
                }
            }
            fn skip_ws_newlines_comments(&mut self) {
                loop {
                    match self.peek() {
                        Some(' ' | '\t' | '\r' | '\n') => {
                            self.bump();
                        }
                        Some('#') => self.skip_comment(),
                        _ => break,
                    }
                }
            }
            fn inline_table(&mut self) -> Result<Value, ParseError> {
                self.bump();
                let mut entries = Vec::new();
                self.skip_inline_ws();
                if self.peek() == Some('}') {
                    self.bump();
                    return Ok(Value::InlineTable(entries));
                }
                loop {
                    self.skip_inline_ws();
                    let path = self.key_path()?;
                    self.skip_inline_ws();
                    if self.bump() != Some('=') {
                        return Err(self.err("expected `=` in inline table"));
                    }
                    self.skip_inline_ws();
                    let value = self.value()?;
                    entries.push((path.join("."), value));
                    self.skip_inline_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some('}') => return Ok(Value::InlineTable(entries)),
                        Some(c) => {
                            return Err(self
                                .err(format!("expected `,` or `}}` in inline table, found `{c}`")))
                        }
                        None => return Err(self.err("unterminated inline table")),
                    }
                }
            }
            fn number_or_datetime(&mut self) -> Result<Value, ParseError> {
                if self.looks_like_date() || self.looks_like_time() {
                    return self.datetime();
                }
                self.number()
            }
            fn looks_like_date(&self) -> bool {
                let d = |n: usize| self.peek_at(n).map_or(false, |c| c.is_ascii_digit());
                d(0) && d(1)
                    && d(2)
                    && d(3)
                    && self.peek_at(4) == Some('-')
                    && d(5)
                    && d(6)
                    && self.peek_at(7) == Some('-')
                    && d(8)
                    && d(9)
            }
            fn looks_like_time(&self) -> bool {
                let d = |n: usize| self.peek_at(n).map_or(false, |c| c.is_ascii_digit());
                d(0) && d(1) && self.peek_at(2) == Some(':') && d(3) && d(4)
            }
            fn datetime(&mut self) -> Result<Value, ParseError> {
                let mut s = String::new();
                let is_dt =
                    |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '.' | '+');
                while let Some(c) = self.peek() {
                    if is_dt(c) {
                        s.push(c);
                        self.pos += 1;
                    } else if c == ' '
                        && self.peek_at(1).map_or(false, |n| n.is_ascii_digit())
                        && self.peek_at(3) == Some(':')
                    {
                        s.push(' ');
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                Ok(Value::Datetime(s))
            }
            fn number(&mut self) -> Result<Value, ParseError> {
                if self.try_keyword("inf") {
                    return Ok(Value::Float(f64::INFINITY));
                }
                if self.try_keyword("nan") {
                    return Ok(Value::Float(f64::NAN));
                }
                let mut tok = String::new();
                if matches!(self.peek(), Some('+') | Some('-')) {
                    let sign = self.bump().unwrap();
                    tok.push(sign);
                    if self.try_keyword("inf") {
                        return Ok(Value::Float(if sign == '-' {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }));
                    }
                    if self.try_keyword("nan") {
                        return Ok(Value::Float(f64::NAN));
                    }
                }
                if self.peek() == Some('0') {
                    if let Some(r) = self.peek_at(1) {
                        if matches!(r, 'x' | 'o' | 'b') && tok.is_empty() {
                            return self.radix_integer();
                        }
                    }
                }
                let mut is_float = false;
                while let Some(c) = self.peek() {
                    match c {
                        '0'..='9' | '_' => {
                            tok.push(c);
                            self.pos += 1;
                        }
                        '.' | 'e' | 'E' | '+' | '-' => {
                            is_float = true;
                            tok.push(c);
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                let clean: String = tok.chars().filter(|c| *c != '_').collect();
                if is_float {
                    clean
                        .parse::<f64>()
                        .map(Value::Float)
                        .map_err(|_| self.err(format!("invalid number `{tok}`")))
                } else {
                    clean
                        .parse::<i64>()
                        .map(Value::Integer)
                        .map_err(|_| self.err(format!("invalid number `{tok}`")))
                }
            }
            fn radix_integer(&mut self) -> Result<Value, ParseError> {
                self.bump();
                let prefix = self.bump().unwrap();
                let radix = match prefix {
                    'x' => 16,
                    'o' => 8,
                    'b' => 2,
                    _ => 16,
                };
                let mut tok = String::new();
                while let Some(c) = self.peek() {
                    if c == '_' {
                        self.pos += 1;
                    } else if c.is_digit(radix) {
                        tok.push(c);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if tok.is_empty() {
                    return Err(self.err("expected digits after numeric base prefix"));
                }
                i64::from_str_radix(&tok, radix)
                    .map(Value::Integer)
                    .map_err(|_| self.err(format!("invalid base-{radix} integer `{tok}`")))
            }
        }

        fn is_bare_key_char(c: char) -> bool {
            c.is_ascii_alphanumeric() || c == '_' || c == '-'
        }
    }

    // ── core.encoding.yaml: full std-only YAML 1.2 core → DataTree (c152) ─────────
    // D-ENC-YAML1 = A: block mappings + sequences (indentation-driven), flow `{}`/`[]`,
    // core-schema typed scalars (null/~, bool, int, float, str), single/double-quoted
    // + plain + block scalars (`|` literal, `>` folded with chomping), comments,
    // `---`/`...` document markers, and anchors/aliases (`&a`/`*a`). Explicit/custom
    // tags (`!!str`, `!MyType`) are deferred to c153 (frozen). No external crates (I6).
    pub mod yaml {
        use super::DataTree;
        use std::collections::BTreeMap;

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct ParseError {
            pub line: usize,
            pub message: String,
        }

        pub fn parse_to_tree(raw: &str) -> Result<DataTree, ParseError> {
            let lines: Vec<String> = raw
                .split('\n')
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect();
            let mut p = Parser {
                lines,
                pos: 0,
                anchors: BTreeMap::new(),
            };
            p.skip_ignorable();
            // Leading document marker(s).
            while p.at_doc_marker() {
                p.pos += 1;
                p.skip_ignorable();
            }
            if p.pos >= p.lines.len() || p.at_doc_end() {
                return Ok(DataTree::Null);
            }
            let base = p.indent(p.pos);
            p.parse_node(base)
        }

        struct Parser {
            lines: Vec<String>,
            pos: usize,
            anchors: BTreeMap<String, DataTree>,
        }

        impl Parser {
            fn indent(&self, i: usize) -> usize {
                self.lines[i].chars().take_while(|c| *c == ' ').count()
            }
            // The line's content with leading indent removed and any trailing comment
            // stripped (a ` #` outside quotes, or a leading `#`).
            fn content(&self, i: usize) -> String {
                let after = &self.lines[i][self.indent(i)..];
                strip_comment(after)
            }
            fn is_ignorable(&self, i: usize) -> bool {
                let t = self.lines[i].trim();
                t.is_empty() || t.starts_with('#')
            }
            fn skip_ignorable(&mut self) {
                while self.pos < self.lines.len() && self.is_ignorable(self.pos) {
                    self.pos += 1;
                }
            }
            fn at_doc_marker(&self) -> bool {
                self.pos < self.lines.len() && self.lines[self.pos].trim_start().starts_with("---")
            }
            fn at_doc_end(&self) -> bool {
                self.pos < self.lines.len() && self.lines[self.pos].trim() == "..."
            }

            fn parse_node(&mut self, min_indent: usize) -> Result<DataTree, ParseError> {
                self.skip_ignorable();
                if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                    return Ok(DataTree::Null);
                }
                let ind = self.indent(self.pos);
                if ind < min_indent {
                    return Ok(DataTree::Null);
                }
                let content = self.content(self.pos);
                if content == "-" || content.starts_with("- ") {
                    self.parse_block_seq(ind)
                } else if is_map_entry(&content) {
                    self.parse_block_map(ind)
                } else {
                    // A bare scalar node (possibly anchored/aliased/flow/quoted).
                    self.pos += 1;
                    self.parse_inline_value(&content)
                }
            }

            fn parse_block_seq(&mut self, indent: usize) -> Result<DataTree, ParseError> {
                let mut items = Vec::new();
                loop {
                    self.skip_ignorable();
                    if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                        break;
                    }
                    let ind = self.indent(self.pos);
                    if ind != indent {
                        break;
                    }
                    let content = self.content(self.pos);
                    if content != "-" && !content.starts_with("- ") {
                        break;
                    }
                    // Dash trick: blank out `-` so the item content aligns as a normal
                    // block at indent+1, then parse it uniformly (scalar/map/seq).
                    let line = &mut self.lines[self.pos];
                    let bytes: Vec<char> = line.chars().collect();
                    // The dash sits at char index == indent (indentation is spaces only).
                    let mut rebuilt: String = bytes
                        .iter()
                        .enumerate()
                        .map(|(i, c)| if i == indent { ' ' } else { *c })
                        .collect();
                    // If nothing follows the dash, leave a blank line.
                    if rebuilt.trim().is_empty() {
                        rebuilt = String::new();
                    }
                    *line = rebuilt;
                    let item = self.parse_node(indent + 1)?;
                    items.push(item);
                }
                Ok(DataTree::Array(items))
            }

            fn parse_block_map(&mut self, indent: usize) -> Result<DataTree, ParseError> {
                let mut entries: Vec<(String, DataTree)> = Vec::new();
                loop {
                    self.skip_ignorable();
                    if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                        break;
                    }
                    let ind = self.indent(self.pos);
                    if ind != indent {
                        break;
                    }
                    let content = self.content(self.pos);
                    if content.starts_with("- ") || content == "-" || !is_map_entry(&content) {
                        break;
                    }
                    let line_no = self.pos + 1;
                    let (key, rest) = split_key(&content).ok_or_else(|| ParseError {
                        line: line_no,
                        message: "expected `key: value`".into(),
                    })?;
                    self.pos += 1;
                    let rest = rest.trim();
                    let value = if rest.is_empty() {
                        // Nested block (deeper indent) or empty → Null.
                        self.skip_ignorable();
                        if self.pos < self.lines.len()
                            && self.indent(self.pos) > indent
                            && !self.at_doc_marker()
                            && !self.at_doc_end()
                        {
                            self.parse_node(indent + 1)?
                        } else {
                            DataTree::Null
                        }
                    } else if rest.starts_with('|') || rest.starts_with('>') {
                        self.parse_block_scalar(indent, rest)
                    } else {
                        self.parse_inline_value(rest)?
                    };
                    entries.push((key, value));
                }
                Ok(DataTree::Object(entries))
            }

            // A `|`/`>` block scalar. Following lines more-indented than the key form the
            // body; dedent by the first body line's indent. `>` folds line breaks to spaces.
            fn parse_block_scalar(&mut self, parent_indent: usize, header: &str) -> DataTree {
                let folded = header.starts_with('>');
                let chomp = if header.contains('-') {
                    'S'
                } else if header.contains('+') {
                    'K'
                } else {
                    'C'
                };
                let mut body_lines: Vec<String> = Vec::new();
                let mut block_indent: Option<usize> = None;
                while self.pos < self.lines.len() {
                    let raw = &self.lines[self.pos];
                    if raw.trim().is_empty() {
                        body_lines.push(String::new());
                        self.pos += 1;
                        continue;
                    }
                    let ind = self.indent(self.pos);
                    if ind <= parent_indent {
                        break;
                    }
                    let bi = *block_indent.get_or_insert(ind);
                    let chars: Vec<char> = raw.chars().collect();
                    let start = bi.min(chars.len());
                    let dedented: String = chars[start..].iter().collect();
                    body_lines.push(dedented);
                    self.pos += 1;
                }
                // Drop trailing blank lines for chomping decisions.
                let mut text = if folded {
                    fold_lines(&body_lines)
                } else {
                    body_lines.join("\n")
                };
                let trimmed = text.trim_end_matches('\n').to_string();
                text = match chomp {
                    'S' => trimmed,                                        // strip: no trailing newline
                    'K' => text.trim_end_matches('\n').to_string() + "\n", // keep (simplified to one)
                    _ => trimmed + "\n", // clip: single trailing newline
                };
                DataTree::Text(text)
            }

            fn parse_inline_value(&mut self, s: &str) -> Result<DataTree, ParseError> {
                let s = s.trim();
                // Anchor: `&name <value?>` — register the parsed value under `name`.
                if let Some(rest) = s.strip_prefix('&') {
                    let mut it = rest.splitn(2, char::is_whitespace);
                    let name = it.next().unwrap_or("").to_string();
                    let val_str = it.next().unwrap_or("").trim();
                    let value = if val_str.is_empty() {
                        // The value is a nested block following this line.
                        self.parse_node(0)?
                    } else {
                        self.parse_inline_value(val_str)?
                    };
                    self.anchors.insert(name, value.clone());
                    return Ok(value);
                }
                // Alias: `*name`.
                if let Some(name) = s.strip_prefix('*') {
                    return Ok(self
                        .anchors
                        .get(name.trim())
                        .cloned()
                        .unwrap_or(DataTree::Null));
                }
                if s.starts_with('[') {
                    return Ok(parse_flow(s).0);
                }
                if s.starts_with('{') {
                    return Ok(parse_flow(s).0);
                }
                Ok(scalar_value(s))
            }
        }

        // ── Flow `[...]` / `{...}` (single-line) ─────────────────────────────────
        fn parse_flow(s: &str) -> (DataTree, usize) {
            let chars: Vec<char> = s.chars().collect();
            parse_flow_at(&chars, 0)
        }
        fn parse_flow_at(chars: &[char], mut i: usize) -> (DataTree, usize) {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i >= chars.len() {
                return (DataTree::Null, i);
            }
            match chars[i] {
                '[' => {
                    i += 1;
                    let mut items = Vec::new();
                    loop {
                        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
                            i += 1;
                        }
                        if i >= chars.len() || chars[i] == ']' {
                            i += 1;
                            break;
                        }
                        let (v, ni) = parse_flow_at(chars, i);
                        items.push(v);
                        i = ni;
                        while i < chars.len() && chars[i].is_whitespace() {
                            i += 1;
                        }
                        if i < chars.len() && chars[i] == ',' {
                            i += 1;
                        } else if i < chars.len() && chars[i] == ']' {
                            i += 1;
                            break;
                        }
                    }
                    (DataTree::Array(items), i)
                }
                '{' => {
                    i += 1;
                    let mut entries = Vec::new();
                    loop {
                        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
                            i += 1;
                        }
                        if i >= chars.len() || chars[i] == '}' {
                            i += 1;
                            break;
                        }
                        // key up to ':'
                        let (key, ni) = scan_flow_scalar(chars, i, true);
                        i = ni;
                        while i < chars.len() && chars[i].is_whitespace() {
                            i += 1;
                        }
                        if i < chars.len() && chars[i] == ':' {
                            i += 1;
                        }
                        let (v, nj) = parse_flow_at(chars, i);
                        i = nj;
                        entries.push((key.trim().to_string(), v));
                        while i < chars.len() && chars[i].is_whitespace() {
                            i += 1;
                        }
                        if i < chars.len() && chars[i] == ',' {
                            i += 1;
                        } else if i < chars.len() && chars[i] == '}' {
                            i += 1;
                            break;
                        }
                    }
                    (DataTree::Object(entries), i)
                }
                _ => {
                    let (raw, ni) = scan_flow_scalar(chars, i, false);
                    (scalar_value(raw.trim()), ni)
                }
            }
        }
        // Read a flow scalar (until `,`/`]`/`}`/`:` when key) honoring quotes.
        fn scan_flow_scalar(chars: &[char], mut i: usize, as_key: bool) -> (String, usize) {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                let q = chars[i];
                let mut out = String::new();
                i += 1;
                while i < chars.len() {
                    if chars[i] == q {
                        if q == '\'' && i + 1 < chars.len() && chars[i + 1] == '\'' {
                            out.push('\'');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    if chars[i] == '\\' && q == '"' && i + 1 < chars.len() {
                        out.push(unescape(chars[i + 1]));
                        i += 2;
                        continue;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
                return (out, i);
            }
            let mut out = String::new();
            while i < chars.len() {
                let c = chars[i];
                if c == ',' || c == ']' || c == '}' {
                    break;
                }
                if as_key && c == ':' {
                    break;
                }
                out.push(c);
                i += 1;
            }
            (out, i)
        }

        fn fold_lines(lines: &[String]) -> String {
            // `>` folding: blank lines become newlines; consecutive non-blank lines join
            // with a single space.
            let mut out = String::new();
            let mut prev_blank = true;
            for l in lines {
                if l.trim().is_empty() {
                    out.push('\n');
                    prev_blank = true;
                } else {
                    if !prev_blank {
                        out.push(' ');
                    }
                    out.push_str(l);
                    prev_blank = false;
                }
            }
            out
        }

        fn unescape(c: char) -> char {
            match c {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '0' => '\0',
                '\\' => '\\',
                '"' => '"',
                _ => c,
            }
        }

        // Strip a trailing ` #...` comment that is outside quotes, or a leading `#`.
        fn strip_comment(s: &str) -> String {
            let chars: Vec<char> = s.chars().collect();
            let mut in_s = false;
            let mut in_d = false;
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                match c {
                    '\'' if !in_d => in_s = !in_s,
                    '"' if !in_s => in_d = !in_d,
                    '#' if !in_s
                        && !in_d
                        && (i == 0 || chars[i - 1] == ' ' || chars[i - 1] == '\t') =>
                    {
                        let kept: String = chars[..i].iter().collect();
                        return kept.trim_end().to_string();
                    }
                    _ => {}
                }
                i += 1;
            }
            s.trim_end().to_string()
        }

        // Is this content a `key: value` mapping entry (top-level `:` outside flow/quotes)?
        fn is_map_entry(s: &str) -> bool {
            top_level_colon(s).is_some()
        }
        fn top_level_colon(s: &str) -> Option<usize> {
            let chars: Vec<char> = s.chars().collect();
            let mut in_s = false;
            let mut in_d = false;
            let mut depth = 0i32;
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                match c {
                    '\'' if !in_d => in_s = !in_s,
                    '"' if !in_s => in_d = !in_d,
                    '[' | '{' if !in_s && !in_d => depth += 1,
                    ']' | '}' if !in_s && !in_d => depth -= 1,
                    ':' if !in_s && !in_d && depth == 0 => {
                        // A mapping `:` must be followed by space or end-of-line.
                        if i + 1 >= chars.len() || chars[i + 1] == ' ' {
                            return Some(i);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            None
        }
        fn split_key(s: &str) -> Option<(String, String)> {
            let idx = top_level_colon(s)?;
            let chars: Vec<char> = s.chars().collect();
            let key_raw: String = chars[..idx].iter().collect();
            let rest: String = chars[idx + 1..].iter().collect();
            Some((unquote_key(key_raw.trim()), rest))
        }
        fn unquote_key(k: &str) -> String {
            if (k.starts_with('"') && k.ends_with('"') && k.len() >= 2)
                || (k.starts_with('\'') && k.ends_with('\'') && k.len() >= 2)
            {
                k[1..k.len() - 1].to_string()
            } else {
                k.to_string()
            }
        }

        // Type a plain/quoted scalar by the YAML core schema.
        fn scalar_value(s: &str) -> DataTree {
            let s = s.trim();
            if s.is_empty() {
                return DataTree::Null;
            }
            // Quoted strings are always text.
            if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                let inner = &s[1..s.len() - 1];
                let mut out = String::new();
                let mut chars = inner.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        if let Some(n) = chars.next() {
                            out.push(unescape(n));
                        }
                    } else {
                        out.push(c);
                    }
                }
                return DataTree::Text(out);
            }
            if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
                return DataTree::Text(s[1..s.len() - 1].replace("''", "'"));
            }
            match s {
                "null" | "Null" | "NULL" | "~" => return DataTree::Null,
                "true" | "True" | "TRUE" => return DataTree::Bool(true),
                "false" | "False" | "FALSE" => return DataTree::Bool(false),
                ".inf" | ".Inf" | ".INF" => return DataTree::Float(f64::INFINITY),
                "-.inf" | "-.Inf" => return DataTree::Float(f64::NEG_INFINITY),
                ".nan" | ".NaN" | ".NAN" => return DataTree::Float(f64::NAN),
                _ => {}
            }
            // Integer (decimal, with optional sign).
            if let Ok(n) = s.parse::<i64>() {
                return DataTree::Int(n);
            }
            // Float: must contain a `.`, `e`/`E` and parse cleanly.
            if (s.contains('.') || s.contains('e') || s.contains('E'))
                && s.chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
            {
                if let Ok(f) = s.parse::<f64>() {
                    return DataTree::Float(f);
                }
            }
            DataTree::Text(s.to_string())
        }

        // ── Render: a `DataTree` → block YAML text ───────────────────────────────
        pub fn render(t: &DataTree) -> String {
            let mut out = String::new();
            render_node(t, 0, &mut out);
            let s = out.trim_end().to_string();
            if s.is_empty() {
                "{}".to_string()
            } else {
                s
            }
        }
        fn render_node(t: &DataTree, indent: usize, out: &mut String) {
            let pad = " ".repeat(indent);
            match t {
                DataTree::Object(entries) => {
                    if entries.is_empty() {
                        out.push_str(&format!("{}{{}}\n", pad));
                        return;
                    }
                    for (k, v) in entries {
                        match v {
                            DataTree::Object(e) if !e.is_empty() => {
                                out.push_str(&format!("{}{}:\n", pad, render_key(k)));
                                render_node(v, indent + 2, out);
                            }
                            DataTree::Array(a) if !a.is_empty() => {
                                out.push_str(&format!("{}{}:\n", pad, render_key(k)));
                                render_seq(a, indent, out);
                            }
                            _ => {
                                out.push_str(&format!(
                                    "{}{}: {}\n",
                                    pad,
                                    render_key(k),
                                    render_scalar(v)
                                ));
                            }
                        }
                    }
                }
                DataTree::Array(items) => render_seq(items, indent, out),
                _ => out.push_str(&format!("{}{}\n", pad, render_scalar(t))),
            }
        }
        fn render_seq(items: &[DataTree], indent: usize, out: &mut String) {
            let pad = " ".repeat(indent);
            for item in items {
                match item {
                    DataTree::Object(e) if !e.is_empty() => {
                        out.push_str(&format!("{}-\n", pad));
                        render_node(item, indent + 2, out);
                    }
                    DataTree::Array(a) if !a.is_empty() => {
                        out.push_str(&format!("{}-\n", pad));
                        render_seq(a, indent + 2, out);
                    }
                    _ => out.push_str(&format!("{}- {}\n", pad, render_scalar(item))),
                }
            }
        }
        fn render_key(k: &str) -> String {
            if k.is_empty() || k.contains(':') || k.contains(' ') || k.contains('#') {
                format!("{:?}", k)
            } else {
                k.to_string()
            }
        }
        fn render_scalar(v: &DataTree) -> String {
            match v {
                DataTree::Null => "null".to_string(),
                DataTree::Bool(b) => b.to_string(),
                DataTree::Int(n) => n.to_string(),
                DataTree::Float(f) => format!("{:?}", f),
                DataTree::Text(s) => {
                    if needs_quote(s) {
                        format!("{:?}", s)
                    } else {
                        s.clone()
                    }
                }
                DataTree::Bytes(bs) => {
                    let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                    format!("[{}]", parts.join(", "))
                }
                // An inline collection value renders in flow form.
                DataTree::Array(items) => {
                    let parts: Vec<String> = items.iter().map(render_scalar).collect();
                    format!("[{}]", parts.join(", "))
                }
                DataTree::Object(entries) => {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, val)| format!("{}: {}", render_key(k), render_scalar(val)))
                        .collect();
                    format!("{{{}}}", parts.join(", "))
                }
            }
        }
        fn needs_quote(s: &str) -> bool {
            if s.is_empty() {
                return true;
            }
            matches!(
                s,
                "null"
                    | "Null"
                    | "NULL"
                    | "~"
                    | "true"
                    | "True"
                    | "TRUE"
                    | "false"
                    | "False"
                    | "FALSE"
            ) || s.parse::<i64>().is_ok()
                || s.parse::<f64>().is_ok()
                || s.starts_with(' ')
                || s.ends_with(' ')
                || s.starts_with([
                    '-', '?', ':', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"',
                    '%', '@', '`',
                ])
                || s.contains(": ")
                || s.contains(" #")
                || s.contains('\n')
        }
    }
}

// ── View<T> (D-DYNARRAY1) ────────────────────────────────────────────────────
// `list.view(a..b)` is a zero-copy window: unlike every bridge type below,
// `View<T>` has no owning Rust struct here — it lowers straight to a plain
// borrowed slice `&[T]` (`Context::rust_type`'s `View` arm, crates/jet-codegen/
// src/Codegen/Context.rs), and its constructor/method helpers
// (`jet_view_new`/`jet_view_fold`/`jet_view_map`) live in Core.rs next to
// `jet_slice_vec`/`jet_list_fold` — the same bare (non-`jet_std::`-namespaced)
// family every other list method belongs to, since `.view(...)` dispatches
// through the ordinary list-method machinery, not the handle-type dispatch
// the structs below use. Ownership (the window cannot outlive its list) is
// proved by sema's E2305, not by a Rust lifetime parameter on a wrapper type.

// ── Streaming file handles (E2-M7, D-IO2) ────────────────────────────────────
// FileReader / FileWriter are RAII: Drop closes (and flushes) them
// on every exit path — including `?` early returns and panics.
struct JetFileReader {
    inner: std::io::BufReader<std::fs::File>,
    path: String,
}
struct JetFileWriter {
    inner: std::io::BufWriter<std::fs::File>,
    path: String,
}

// ── core.db connection handle (D-DBDRIVER1) ──────────────────────────────────
// The real SQLite connection lives in the FFI bridge crate's thread-local
// handle map (`rusqlite::Connection` can't cross into this always-compiled
// prelude — I6). `JetDbConnection` is a thin, `Copy` handle wrapper so
// `.query`/`.execute`/`.begin`/`.commit`/`.rollback`/`.close` dispatch by
// receiver TYPE (`DbConnection`), the same mechanism `FileReader`/`FileWriter`
// use, instead of exposing the bare `u64` to Jet code.
#[derive(Clone, Copy, Debug)]
struct JetDbConnection {
    handle: u64,
}

// ── core.plugin sandboxed WASM handle (D-DEP-WASM1=A / D-PLUGIN1=B, c81) ─────
// The real wasmtime `Store`/`Instance` live in the FFI bridge crate's
// thread-local handle map (wasmtime types can't cross into this
// always-compiled prelude — I6). `JetPlugin` is a thin, `Copy` handle wrapper,
// same shape as `JetDbConnection`, so `.call`/`.call_int` dispatch by receiver
// TYPE (`Plugin`) instead of exposing the bare `u64` to Jet code.
#[derive(Clone, Copy, Debug)]
struct JetPlugin {
    handle: u64,
}

// jet:raylib-begin
// -- core.raylib bridge (D-RAYLIB1=A / D-FLAGSHIP-RAYLIB1=A) -----------------
// Display remains explicit: without JET_RAYLIB_DISPLAY=1 the bridge is a
// deterministic headless no-op. With the flag set, Jet dynamically loads the
// native raylib shared library and calls the real C API without adding a
// compile-time link requirement to every CI run.
#[derive(Clone, Debug)]
struct RaylibWindow {
    width: i64,
    height: i64,
    title: String,
    native: bool,
}

#[derive(Clone, Copy, Debug)]
struct RaylibColor {
    r: i64,
    g: i64,
    b: i64,
    a: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct JetRaylibCColor {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

type JetRaylibInitWindow = unsafe extern "C" fn(i32, i32, *const std::os::raw::c_char);
type JetRaylibWindowShouldClose = unsafe extern "C" fn() -> bool;
type JetRaylibBeginDrawing = unsafe extern "C" fn();
type JetRaylibClearBackground = unsafe extern "C" fn(JetRaylibCColor);
type JetRaylibDrawText =
    unsafe extern "C" fn(*const std::os::raw::c_char, i32, i32, i32, JetRaylibCColor);
type JetRaylibEndDrawing = unsafe extern "C" fn();
type JetRaylibCloseWindow = unsafe extern "C" fn();

#[derive(Clone, Copy)]
struct JetRaylibApi {
    init_window: JetRaylibInitWindow,
    window_should_close: JetRaylibWindowShouldClose,
    begin_drawing: JetRaylibBeginDrawing,
    clear_background: JetRaylibClearBackground,
    draw_text: JetRaylibDrawText,
    end_drawing: JetRaylibEndDrawing,
    close_window: JetRaylibCloseWindow,
}

static JET_RAYLIB_WINDOW_OPEN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn jet_raylib_display_enabled() -> bool {
    std::env::var("JET_RAYLIB_DISPLAY").as_deref() == Ok("1")
}

fn jet_raylib_clamp_u8(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}

fn jet_raylib_c_color(color: &RaylibColor) -> JetRaylibCColor {
    JetRaylibCColor {
        r: jet_raylib_clamp_u8(color.r),
        g: jet_raylib_clamp_u8(color.g),
        b: jet_raylib_clamp_u8(color.b),
        a: jet_raylib_clamp_u8(color.a),
    }
}

fn jet_raylib_cstring(s: &String) -> std::ffi::CString {
    let filtered: Vec<u8> = s.as_bytes().iter().copied().filter(|b| *b != 0).collect();
    std::ffi::CString::new(filtered).unwrap_or_else(|_| std::ffi::CString::new("").unwrap())
}

#[cfg(unix)]
mod jet_raylib_dyn {
    use super::*;
    use std::os::raw::{c_char, c_int, c_void};
    use std::sync::OnceLock;

    #[cfg(target_os = "linux")]
    #[link(name = "dl")]
    unsafe extern "C" {}

    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    const RTLD_NOW: c_int = 2;
    static API: OnceLock<Option<JetRaylibApi>> = OnceLock::new();

    pub(super) fn api() -> Option<&'static JetRaylibApi> {
        API.get_or_init(load).as_ref()
    }

    fn load() -> Option<JetRaylibApi> {
        // SAFETY: the loader only reads process-global dynamic-linker state.
        let handle = unsafe {
            #[cfg(target_os = "macos")]
            {
                dlopen(b"libraylib.dylib\0".as_ptr().cast(), RTLD_NOW)
            }
            #[cfg(not(target_os = "macos"))]
            {
                let first = dlopen(b"libraylib.so\0".as_ptr().cast(), RTLD_NOW);
                if first.is_null() {
                    dlopen(b"libraylib.so.5\0".as_ptr().cast(), RTLD_NOW)
                } else {
                    first
                }
            }
        };
        if handle.is_null() {
            return None;
        }
        Some(JetRaylibApi {
            init_window: symbol(handle, b"InitWindow\0")?,
            window_should_close: symbol(handle, b"WindowShouldClose\0")?,
            begin_drawing: symbol(handle, b"BeginDrawing\0")?,
            clear_background: symbol(handle, b"ClearBackground\0")?,
            draw_text: symbol(handle, b"DrawText\0")?,
            end_drawing: symbol(handle, b"EndDrawing\0")?,
            close_window: symbol(handle, b"CloseWindow\0")?,
        })
    }

    fn symbol<T: Copy>(handle: *mut c_void, name: &[u8]) -> Option<T> {
        // SAFETY: names are NUL-terminated raylib symbols and T matches each
        // requested C function signature at the call site above.
        let ptr = unsafe { dlsym(handle, name.as_ptr().cast()) };
        if ptr.is_null() {
            None
        } else {
            // SAFETY: C function pointers and data pointers have the platform ABI
            // representation used by dlsym on supported Unix targets.
            Some(unsafe { std::mem::transmute_copy(&ptr) })
        }
    }
}

#[cfg(unix)]
fn jet_raylib_api() -> Option<&'static JetRaylibApi> {
    jet_raylib_dyn::api()
}

#[cfg(not(unix))]
fn jet_raylib_api() -> Option<&'static JetRaylibApi> {
    None
}

fn jet_raylib_window_open(width: i64, height: i64, title: &String) -> RaylibWindow {
    let mut native = false;
    if jet_raylib_display_enabled() {
        if let Some(api) = jet_raylib_api() {
            let title_c = jet_raylib_cstring(title);
            // SAFETY: raylib is loaded, the title pointer is valid for the call,
            // and all C interaction is confined to this vetted bridge.
            unsafe { (api.init_window)(width as i32, height as i32, title_c.as_ptr()) };
            JET_RAYLIB_WINDOW_OPEN.store(true, std::sync::atomic::Ordering::SeqCst);
            native = true;
        }
    }
    RaylibWindow {
        width,
        height,
        title: title.clone(),
        native,
    }
}

fn jet_raylib_window_should_close(window: &RaylibWindow) -> bool {
    if window.native {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: the function pointer was loaded from raylib and takes no args.
            return unsafe { (api.window_should_close)() };
        }
    }
    true
}

fn jet_raylib_begin_drawing(window: &RaylibWindow) {
    if window.native {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: the raylib window was opened by this bridge.
            unsafe { (api.begin_drawing)() };
        }
    }
}

fn jet_raylib_clear_background(color: &RaylibColor) {
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: color is a repr(C) mirror of raylib Color.
            unsafe { (api.clear_background)(jet_raylib_c_color(color)) };
        }
    }
}

fn jet_raylib_draw_text(text: &String, x: i64, y: i64, size: i64, color: &RaylibColor) {
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            let text_c = jet_raylib_cstring(text);
            // SAFETY: the text pointer is valid for the call, color matches C ABI,
            // and raylib owns the active drawing context.
            unsafe {
                (api.draw_text)(
                    text_c.as_ptr(),
                    x as i32,
                    y as i32,
                    size as i32,
                    jet_raylib_c_color(color),
                )
            };
        }
    }
}

fn jet_raylib_end_drawing() {
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: the raylib window/drawing context is bridge-owned.
            unsafe { (api.end_drawing)() };
        }
    }
}

fn jet_raylib_close_window(window: &RaylibWindow) {
    if window.native {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: the window was opened by this bridge.
            unsafe { (api.close_window)() };
            JET_RAYLIB_WINDOW_OPEN.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

fn jet_raylib_color(r: i64, g: i64, b: i64, a: i64) -> RaylibColor {
    RaylibColor { r, g, b, a }
}
// jet:raylib-end

// -- core.game headless substrate (D-GAME1/2/3, D-WD10, D-GAME-*) -------------
#[derive(Default, Debug)]
struct GameState {
    assets: Vec<(String, String)>,
    bindings: Vec<(String, String)>,
    budgets: Option<GameBudgets>,
    components: Vec<String>,
}

#[derive(Clone)]
struct GameAssets {
    state: std::rc::Rc<std::cell::RefCell<GameState>>,
}

#[derive(Clone)]
struct GameInputMap {
    state: std::rc::Rc<std::cell::RefCell<GameState>>,
}

#[derive(Clone)]
struct GameBudgetsSlot {
    state: std::rc::Rc<std::cell::RefCell<GameState>>,
}

struct GameScene {
    name: String,
    assets: GameAssets,
    user_assets: GameAssets,
    input: GameInputMap,
    user_input: GameInputMap,
    budgets: GameBudgetsSlot,
    user_budgets: GameBudgetsSlot,
    callbacks: std::rc::Rc<std::cell::RefCell<Vec<Box<dyn FnMut(GameFrame)>>>>,
}

#[derive(Clone, Debug)]
struct GameImage {
    path: String,
}

#[derive(Clone, Debug)]
struct GameSound {
    path: String,
}

#[derive(Clone, Debug)]
struct GameReplay {
    path: String,
}

#[derive(Clone, Debug)]
struct GameBackend {
    renderer: String,
    audio: String,
    editor: String,
}

#[derive(Clone, Debug)]
struct GameBudgets {
    frame_ms: i64,
    memory_mb: i64,
    asset_kb: i64,
    draw_calls: i64,
}

#[derive(Clone, Debug)]
struct GameInputSnapshot {
    pressed: Vec<String>,
}

#[derive(Clone, Debug)]
struct GameFrame {
    index: i64,
    user_index: i64,
    input: GameInputSnapshot,
    user_input: GameInputSnapshot,
}

impl JetShow for GameScene {
    fn jet_show(&self) -> String {
        format!("GameScene({})", self.name)
    }
}
impl JetShow for GameAssets {
    fn jet_show(&self) -> String {
        "GameAssets".to_string()
    }
}
impl JetShow for GameInputMap {
    fn jet_show(&self) -> String {
        "GameInputMap".to_string()
    }
}
impl JetShow for GameBudgetsSlot {
    fn jet_show(&self) -> String {
        "GameBudgetsSlot".to_string()
    }
}
impl JetShow for GameImage {
    fn jet_show(&self) -> String {
        format!("GameImage({})", self.path)
    }
}
impl JetDebug for GameImage {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}
impl JetShow for GameSound {
    fn jet_show(&self) -> String {
        format!("GameSound({})", self.path)
    }
}
impl JetDebug for GameSound {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}
impl JetShow for GameReplay {
    fn jet_show(&self) -> String {
        format!("GameReplay({})", self.path)
    }
}
impl JetShow for GameBackend {
    fn jet_show(&self) -> String {
        format!(
            "GameBackend(renderer: {}, audio: {}, editor: {})",
            self.renderer, self.audio, self.editor
        )
    }
}
impl JetShow for GameBudgets {
    fn jet_show(&self) -> String {
        format!(
            "GameBudgets(frame_ms: {}, memory_mb: {}, asset_kb: {}, draw_calls: {})",
            self.frame_ms, self.memory_mb, self.asset_kb, self.draw_calls
        )
    }
}
impl JetShow for GameInputSnapshot {
    fn jet_show(&self) -> String {
        format!("GameInputSnapshot({})", self.pressed.join(","))
    }
}
impl JetShow for GameFrame {
    fn jet_show(&self) -> String {
        format!("GameFrame({})", self.index)
    }
}

fn jet_game_scene_new(name: &String) -> GameScene {
    let state = std::rc::Rc::new(std::cell::RefCell::new(GameState::default()));
    let assets = GameAssets {
        state: state.clone(),
    };
    let input = GameInputMap {
        state: state.clone(),
    };
    let budgets = GameBudgetsSlot { state };
    GameScene {
        name: name.clone(),
        assets: assets.clone(),
        user_assets: assets,
        input: input.clone(),
        user_input: input,
        budgets: budgets.clone(),
        user_budgets: budgets,
        callbacks: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
    }
}

fn jet_game_replay_record(path: &String) -> GameReplay {
    GameReplay { path: path.clone() }
}

fn jet_game_backend_headless() -> GameBackend {
    GameBackend {
        renderer: "headless".to_string(),
        audio: "none".to_string(),
        editor: "none".to_string(),
    }
}

fn jet_game_budgets_new(
    frame_ms: i64,
    memory_mb: i64,
    asset_kb: i64,
    draw_calls: i64,
) -> GameBudgets {
    GameBudgets {
        frame_ms,
        memory_mb,
        asset_kb,
        draw_calls,
    }
}

fn jet_game_scene_on_frame(scene: &mut GameScene, f: Box<dyn FnMut(GameFrame)>) {
    scene.callbacks.borrow_mut().push(f);
}

fn jet_game_scene_component(scene: &mut GameScene, name: &String) {
    let mut state = scene.assets.state.borrow_mut();
    if !state.components.iter().any(|existing| existing == name) {
        state.components.push(name.clone());
    }
}

fn jet_game_scene_query(scene: &GameScene, names: &String) -> Vec<String> {
    let state = scene.assets.state.borrow();
    let wanted: Vec<&str> = names.split(',').filter(|s| !s.is_empty()).collect();
    if wanted
        .iter()
        .all(|name| state.components.iter().any(|c| c == name))
    {
        vec![wanted.join("+")]
    } else {
        Vec::new()
    }
}

fn jet_game_assets_image(assets: &GameAssets, path: &String) -> Result<GameImage, String> {
    if path.contains("missing") {
        return Err(format!("asset not found: {}", path));
    }
    assets
        .state
        .borrow_mut()
        .assets
        .push(("image".to_string(), path.clone()));
    Ok(GameImage { path: path.clone() })
}

fn jet_game_assets_sound(assets: &GameAssets, path: &String) -> Result<GameSound, String> {
    if path.contains("missing") {
        return Err(format!("asset not found: {}", path));
    }
    assets
        .state
        .borrow_mut()
        .assets
        .push(("sound".to_string(), path.clone()));
    Ok(GameSound { path: path.clone() })
}

fn jet_game_input_bind(input: &GameInputMap, action: &String, key: &String) {
    let mut state = input.state.borrow_mut();
    if !state.bindings.iter().any(|(a, k)| a == action && k == key) {
        state.bindings.push((action.clone(), key.clone()));
    }
}

fn jet_game_budgets_set(slot: &GameBudgetsSlot, budgets: &GameBudgets) {
    slot.state.borrow_mut().budgets = Some(budgets.clone());
}

fn jet_game_input_pressed(input: &GameInputSnapshot, action: &String) -> bool {
    input.pressed.iter().any(|a| a == action)
}

fn jet_game_run(
    scene: &mut GameScene,
    replay: Option<&GameReplay>,
    backend: Option<&GameBackend>,
) -> String {
    let backend = backend.cloned().unwrap_or_else(jet_game_backend_headless);
    let replay_path = replay
        .map(|r| r.path.clone())
        .unwrap_or_else(|| "<none>".to_string());
    let mut out = Vec::new();
    out.push(format!("scene:{}", scene.name));
    out.push(format!(
        "backend:{}/{}/{}",
        backend.renderer, backend.audio, backend.editor
    ));
    out.push(format!("replay:{}", replay_path));
    {
        let state = scene.assets.state.borrow();
        let assets = state
            .assets
            .iter()
            .map(|(kind, path)| format!("{}:{}", kind, path))
            .collect::<Vec<_>>()
            .join(",");
        out.push(format!(
            "assets:{}",
            if assets.is_empty() {
                "none".to_string()
            } else {
                assets
            }
        ));
        let bindings = state
            .bindings
            .iter()
            .map(|(action, key)| format!("{}={}", action, key))
            .collect::<Vec<_>>()
            .join(",");
        out.push(format!(
            "input:{}",
            if bindings.is_empty() {
                "none".to_string()
            } else {
                bindings
            }
        ));
        let components = state.components.join(",");
        out.push(format!(
            "components:{}",
            if components.is_empty() {
                "none".to_string()
            } else {
                components
            }
        ));
        if let Some(b) = &state.budgets {
            out.push(format!(
                "budgets:frame={}ms,memory={}mb,assets={}kb,draws={}",
                b.frame_ms, b.memory_mb, b.asset_kb, b.draw_calls
            ));
        } else {
            out.push("budgets:none".to_string());
        }
    }
    for frame_idx in 0..3 {
        let pressed = if frame_idx == 1 {
            scene
                .assets
                .state
                .borrow()
                .bindings
                .iter()
                .map(|(action, _)| action.clone())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let frame = GameFrame {
            index: frame_idx,
            user_index: frame_idx,
            input: GameInputSnapshot {
                pressed: pressed.clone(),
            },
            user_input: GameInputSnapshot {
                pressed: pressed.clone(),
            },
        };
        for cb in scene.callbacks.borrow_mut().iter_mut() {
            cb(frame.clone());
        }
        let input = if pressed.is_empty() {
            "none".to_string()
        } else {
            pressed.join("+")
        };
        out.push(format!("frame:{} input:{}", frame_idx, input));
    }
    out.join("\n")
}

// ── Typed Path API (D-PATHFS1) ────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct JetPath {
    inner: std::path::PathBuf,
}
impl JetShow for JetPath {
    fn jet_show(&self) -> String {
        self.inner.to_string_lossy().to_string()
    }
}
fn jet_path_from(s: &String) -> JetPath {
    JetPath {
        inner: std::path::PathBuf::from(s),
    }
}
fn jet_path_join(p: &JetPath, other: &String) -> JetPath {
    JetPath {
        inner: p.inner.join(other.as_str()),
    }
}
fn jet_path_parent(p: &JetPath) -> Option<JetPath> {
    p.inner.parent().map(|par| JetPath {
        inner: par.to_path_buf(),
    })
}
fn jet_path_extension(p: &JetPath) -> Option<String> {
    p.inner.extension().map(|e| e.to_string_lossy().to_string())
}
fn jet_path_stem(p: &JetPath) -> Option<String> {
    p.inner.file_stem().map(|s| s.to_string_lossy().to_string())
}
fn jet_path_write_atomic(p: &JetPath, content: &Vec<u8>) -> Result<(), jet_std::IoError> {
    let path_s = p.inner.to_string_lossy();
    let dir = p.inner.parent().ok_or_else(|| {
        jet_std::io_error(
            &path_s,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path has no parent directory",
            ),
        )
    })?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".jet_tmp_{}", nanos));
    std::fs::write(&tmp, content)
        .map_err(|e| jet_std::io_error(tmp.to_string_lossy().as_ref(), e))?;
    std::fs::rename(&tmp, &p.inner).map_err(|e| jet_std::io_error(&path_s, e))
}
fn jet_path_walk(p: &JetPath) -> Vec<JetPath> {
    let mut result = Vec::new();
    let mut stack = vec![p.inner.clone()];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        let canonical = match std::fs::canonicalize(&dir) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !visited.insert(canonical) {
            continue; // symlink loop — skip
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            result.push(JetPath {
                inner: path.clone(),
            });
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    result
}
// ─────────────────────────────────────────────────────────────────────────────

fn jet_std_files_open(path: &String) -> Result<JetFileReader, jet_std::IoError> {
    let f = std::fs::File::open(path).map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileReader {
        inner: std::io::BufReader::new(f),
        path: path.clone(),
    })
}
fn jet_std_files_create(path: &String) -> Result<JetFileWriter, jet_std::IoError> {
    let f = std::fs::File::create(path).map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileWriter {
        inner: std::io::BufWriter::new(f),
        path: path.clone(),
    })
}
fn jet_std_files_append(path: &String) -> Result<JetFileWriter, jet_std::IoError> {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileWriter {
        inner: std::io::BufWriter::new(f),
        path: path.clone(),
    })
}
fn jet_std_file_reader_read_line(
    r: &mut JetFileReader,
) -> Result<Option<String>, jet_std::IoError> {
    use std::io::BufRead;
    let mut line = String::new();
    match r.inner.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Some(line))
        }
        Err(e) => Err(jet_std::io_error(&r.path, e)),
    }
}
fn jet_std_file_writer_write_line(
    w: &mut JetFileWriter,
    line: &String,
) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    w.inner
        .write_all(line.as_bytes())
        .and_then(|_| w.inner.write_all(b"\n"))
        .map_err(|e| jet_std::io_error(&w.path, e))
}
fn jet_std_file_writer_flush(w: &mut JetFileWriter) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    w.inner.flush().map_err(|e| jet_std::io_error(&w.path, e))
}

// ── std.path helpers (D-IO1) ──────────────────────────────────────────────────
fn jet_std_path_join(base: &String, part: &String) -> String {
    let b = std::path::Path::new(base.as_str());
    b.join(part.as_str()).to_string_lossy().to_string()
}
fn jet_std_path_parent(path: &String) -> String {
    std::path::Path::new(path.as_str())
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}
fn jet_std_path_extension(path: &String) -> String {
    std::path::Path::new(path.as_str())
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default()
}
fn jet_std_path_normalize(path: &String) -> String {
    // Resolve `.` and `..` components without hitting the filesystem.
    let mut parts: Vec<&str> = Vec::new();
    let s = path.as_str();
    let absolute = s.starts_with('/');
    for seg in s.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{}", joined)
    } else {
        joined
    }
}

// ── core.text.unicode helpers (D-TEXTUNICODE1) ───────────────────────────────
fn jet_text_unicode_scalar_count(s: &String) -> i64 {
    s.chars().count() as i64
}
fn jet_text_unicode_byte_count(s: &String) -> i64 {
    s.len() as i64
}
fn jet_text_unicode_is_ascii(s: &String) -> bool {
    s.is_ascii()
}
fn jet_text_unicode_lower(s: &String) -> String {
    s.to_lowercase()
}
fn jet_text_unicode_upper(s: &String) -> String {
    s.to_uppercase()
}
fn jet_text_unicode_scalars(s: &String) -> Vec<String> {
    s.chars().map(|c| c.to_string()).collect()
}
fn jet_text_decompose_char(c: char, compat: bool, out: &mut String) {
    match c {
        'é' => out.push_str("e\u{0301}"), 'É' => out.push_str("E\u{0301}"),
        'è' => out.push_str("e\u{0300}"), 'á' => out.push_str("a\u{0301}"),
        'ó' => out.push_str("o\u{0301}"), 'í' => out.push_str("i\u{0301}"),
        'ú' => out.push_str("u\u{0301}"), 'ñ' => out.push_str("n\u{0303}"),
        'ö' => out.push_str("o\u{0308}"), 'Ö' => out.push_str("O\u{0308}"),
        'ü' => out.push_str("u\u{0308}"), 'Ü' => out.push_str("U\u{0308}"),
        'ä' => out.push_str("a\u{0308}"), 'Ä' => out.push_str("A\u{0308}"),
        'ç' => out.push_str("c\u{0327}"), 'Ç' => out.push_str("C\u{0327}"),
        'Å' => out.push_str("A\u{030A}"), 'å' => out.push_str("a\u{030A}"),
        'ﬃ' if compat => out.push_str("ffi"), 'ﬁ' if compat => out.push_str("fi"),
        '①' if compat => out.push('1'), '②' if compat => out.push('2'),
        _ => out.push(c),
    }
}
fn jet_text_compose_pair(a: char, b: char) -> Option<char> {
    match (a, b) {
        ('e', '\u{0301}') => Some('é'), ('E', '\u{0301}') => Some('É'),
        ('e', '\u{0300}') => Some('è'), ('a', '\u{0301}') => Some('á'),
        ('o', '\u{0301}') => Some('ó'), ('i', '\u{0301}') => Some('í'),
        ('u', '\u{0301}') => Some('ú'), ('n', '\u{0303}') => Some('ñ'),
        ('o', '\u{0308}') => Some('ö'), ('O', '\u{0308}') => Some('Ö'),
        ('u', '\u{0308}') => Some('ü'), ('U', '\u{0308}') => Some('Ü'),
        ('a', '\u{0308}') => Some('ä'), ('A', '\u{0308}') => Some('Ä'),
        ('c', '\u{0327}') => Some('ç'), ('C', '\u{0327}') => Some('Ç'),
        ('A', '\u{030A}') => Some('Å'), ('a', '\u{030A}') => Some('å'),
        _ => None,
    }
}
fn jet_text_nfd_inner(s: &String, compat: bool) -> String {
    let mut out = String::new();
    for c in s.chars() { jet_text_decompose_char(c, compat, &mut out); }
    out
}
fn jet_text_nfd(s: &String) -> String { jet_text_nfd_inner(s, false) }
fn jet_text_nfkd(s: &String) -> String { jet_text_nfd_inner(s, true) }
fn jet_text_compose(s: String) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if let Some(&next) = it.peek() {
            if let Some(composed) = jet_text_compose_pair(c, next) {
                out.push(composed);
                it.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}
fn jet_text_nfc(s: &String) -> String { jet_text_compose(jet_text_nfd(s)) }
fn jet_text_nfkc(s: &String) -> String { jet_text_compose(jet_text_nfkd(s)) }
fn jet_text_casefold(s: &String) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            'ß' | 'ẞ' => out.push_str("ss"),
            'ς' => out.push('σ'),
            _ => out.push_str(&c.to_lowercase().to_string()),
        }
    }
    out
}
fn jet_text_caseless_eq(a: &String, b: &String) -> bool {
    jet_text_casefold(&jet_text_nfkc(a)) == jet_text_casefold(&jet_text_nfkc(b))
}
fn jet_text_is_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}
fn jet_text_graphemes(s: &String) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in s.chars() {
        if jet_text_is_mark(c) || c == '\u{200D}' {
            if let Some(last) = out.last_mut() { last.push(c); } else { out.push(c.to_string()); }
        } else {
            out.push(c.to_string());
        }
    }
    out
}
fn jet_text_words(s: &String) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() || c == '\'' { cur.push(c); }
        else if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}
fn jet_text_sentences(s: &String) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?') {
            let t = cur.trim();
            if !t.is_empty() { out.push(t.to_string()); }
            cur.clear();
        }
    }
    let t = cur.trim();
    if !t.is_empty() { out.push(t.to_string()); }
    out
}
fn jet_text_char_width(c: char) -> i64 {
    let u = c as u32;
    if c == '\0' || c.is_control() || jet_text_is_mark(c) { 0 }
    else if matches!(u, 0x1100..=0x115F | 0x2329..=0x232A | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE10..=0xFE19 | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1FAFF) { 2 }
    else { 1 }
}
fn jet_text_width(s: &String) -> i64 { s.chars().map(jet_text_char_width).sum() }
fn jet_text_is_alphabetic(s: &String) -> bool { !s.is_empty() && s.chars().all(|c| c.is_alphabetic()) }
fn jet_text_is_numeric(s: &String) -> bool { !s.is_empty() && s.chars().all(|c| c.is_numeric()) }
fn jet_text_is_whitespace(s: &String) -> bool { !s.is_empty() && s.chars().all(|c| c.is_whitespace()) }
fn jet_text_splitn(s: &String, pat: &String, n: i64) -> Vec<String> {
    s.splitn(n.max(0) as usize, pat).map(|x| x.to_string()).collect()
}
fn jet_text_rsplitn(s: &String, pat: &String, n: i64) -> Vec<String> {
    s.rsplitn(n.max(0) as usize, pat).map(|x| x.to_string()).collect()
}
fn jet_text_pad_start(s: &String, width: i64, fill: &String) -> String {
    let mut out = String::new();
    let f = fill.chars().next().unwrap_or(' ');
    for _ in 0..(width - jet_text_width(s)).max(0) { out.push(f); }
    out.push_str(s);
    out
}
fn jet_text_pad_end(s: &String, width: i64, fill: &String) -> String {
    let mut out = s.clone();
    let f = fill.chars().next().unwrap_or(' ');
    for _ in 0..(width - jet_text_width(s)).max(0) { out.push(f); }
    out
}
fn jet_text_center(s: &String, width: i64, fill: &String) -> String {
    let gap = (width - jet_text_width(s)).max(0);
    let left = gap / 2;
    let right = gap - left;
    let f = fill.chars().next().unwrap_or(' ');
    format!("{}{}{}", f.to_string().repeat(left as usize), s, f.to_string().repeat(right as usize))
}
fn jet_text_starts_any(s: &String, prefixes: &Vec<String>) -> bool {
    prefixes.iter().any(|p| s.starts_with(p))
}
fn jet_text_ends_any(s: &String, suffixes: &Vec<String>) -> bool {
    suffixes.iter().any(|p| s.ends_with(p))
}
fn jet_text_char_indices(s: &String) -> Vec<String> {
    s.char_indices().map(|(i, c)| format!("{}:{}", i, c)).collect()
}

fn jet_std_fs_read(path: &String) -> Result<String, jet_std::IoError> {
    std::fs::read_to_string(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_read_bytes(path: &String) -> Result<Vec<u8>, jet_std::IoError> {
    std::fs::read(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_write(path: &String, text: &String) -> Result<(), jet_std::IoError> {
    std::fs::write(path, text).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_append(path: &String, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    f.write_all(text.as_bytes())
        .map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_exists(path: &String) -> bool {
    std::path::Path::new(path).exists()
}
fn jet_std_fs_remove(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::remove_file(path).map_err(|e| jet_std::io_error(path, e))
}
// D-LSDIR1=A: returns DirEntry values with name, full path, and is_dir flag.
fn jet_std_fs_list_dir(path: &String) -> Result<Vec<jet_std::DirEntry>, jet_std::IoError> {
    let mut out = Vec::new();
    let rd = std::fs::read_dir(path).map_err(|e| jet_std::io_error(path, e))?;
    for entry in rd {
        let entry = entry.map_err(|e| jet_std::io_error(path, e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = std::path::Path::new(path.as_str())
            .join(&name)
            .to_string_lossy()
            .to_string();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        out.push(jet_std::DirEntry {
            name,
            path: full_path,
            is_dir,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
fn jet_std_fs_create_dir(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::create_dir_all(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_create_dir_all(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::create_dir_all(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_is_dir(path: &String) -> bool {
    std::path::Path::new(path).is_dir()
}
fn jet_std_fs_remove_dir(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::remove_dir(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_remove_all(path: &String) -> Result<(), jet_std::IoError> {
    let p = std::path::Path::new(path);
    if p.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| jet_std::io_error(path, e))
    } else {
        std::fs::remove_file(path).map_err(|e| jet_std::io_error(path, e))
    }
}
fn jet_std_fs_copy(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| jet_std::io_error(from, e))
}
fn jet_std_fs_symlink(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(from, to).map_err(|e| jet_std::io_error(to, e))
    }
    #[cfg(windows)]
    {
        let meta = std::fs::metadata(from).map_err(|e| jet_std::io_error(from, e))?;
        if meta.is_dir() {
            std::os::windows::fs::symlink_dir(from, to).map_err(|e| jet_std::io_error(to, e))
        } else {
            std::os::windows::fs::symlink_file(from, to).map_err(|e| jet_std::io_error(to, e))
        }
    }
}
fn jet_std_fs_read_link(path: &String) -> Result<String, jet_std::IoError> {
    std::fs::read_link(path)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_hard_link(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::hard_link(from, to).map_err(|e| jet_std::io_error(to, e))
}
fn jet_std_fs_rename(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::rename(from, to).map_err(|e| jet_std::io_error(from, e))
}
fn jet_std_fs_stat(path: &String) -> Result<jet_std::Stat, jet_std::IoError> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| jet_std::io_error(path, e))?;
    let ft = meta.file_type();
    let modified_ms = meta.modified().ok().and_then(system_time_ms).unwrap_or(0);
    let created_ms = meta.created().ok().and_then(system_time_ms).unwrap_or(0);
    let kind = if ft.is_symlink() {
        "symlink"
    } else if ft.is_dir() {
        "dir"
    } else if ft.is_file() {
        "file"
    } else {
        "other"
    };
    Ok(jet_std::Stat {
        size: meta.len() as i64,
        modified_ms,
        created_ms,
        readonly: meta.permissions().readonly(),
        is_file: ft.is_file(),
        is_dir: ft.is_dir(),
        is_symlink: ft.is_symlink(),
        kind: kind.to_string(),
    })
}
fn system_time_ms(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}
fn jet_std_fs_canonicalize(path: &String) -> Result<String, jet_std::IoError> {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_absolute(path: &String) -> Result<String, jet_std::IoError> {
    let p = std::path::Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| jet_std::IoError::Other {
                message: e.to_string(),
            })?
            .join(p)
    };
    Ok(abs.to_string_lossy().to_string())
}
fn jet_std_fs_copy_dir(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    fn copy_tree(
        src: &std::path::Path,
        dst: &std::path::Path,
        shown: &str,
    ) -> Result<(), jet_std::IoError> {
        std::fs::create_dir_all(dst).map_err(|e| jet_std::io_error(shown, e))?;
        for entry in std::fs::read_dir(src).map_err(|e| jet_std::io_error(shown, e))? {
            let entry = entry.map_err(|e| jet_std::io_error(shown, e))?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            let ft = entry.file_type().map_err(|e| jet_std::io_error(shown, e))?;
            if ft.is_dir() {
                copy_tree(&src_path, &dst_path, shown)?;
            } else if ft.is_file() {
                std::fs::copy(&src_path, &dst_path).map_err(|e| jet_std::io_error(shown, e))?;
            }
        }
        Ok(())
    }
    copy_tree(std::path::Path::new(from), std::path::Path::new(to), from)
}
fn jet_std_fs_walk(path: &String) -> Result<Vec<jet_std::WalkEntry>, jet_std::IoError> {
    let root = std::path::PathBuf::from(path);
    let mut out = Vec::new();
    fn walk_dir(
        root: &std::path::Path,
        dir: &std::path::Path,
        depth: i64,
        out: &mut Vec<jet_std::WalkEntry>,
        shown: &str,
    ) -> Result<(), jet_std::IoError> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(|e| jet_std::io_error(shown, e))? {
            entries.push(entry.map_err(|e| jet_std::io_error(shown, e))?);
        }
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let p = entry.path();
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let relative = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .to_string();
            out.push(jet_std::WalkEntry {
                path: p.to_string_lossy().to_string(),
                relative,
                is_dir,
                depth,
            });
            if is_dir {
                walk_dir(root, &p, depth + 1, out, shown)?;
            }
        }
        Ok(())
    }
    walk_dir(&root, &root, 0, &mut out, path)?;
    Ok(out)
}
fn jet_std_fs_glob(pattern: &String) -> Result<Vec<String>, jet_std::IoError> {
    let split = pattern.find(['*', '?']).unwrap_or(pattern.len());
    let base = pattern[..split]
        .rsplit_once(std::path::MAIN_SEPARATOR)
        .map(|(dir, _)| if dir.is_empty() { "." } else { dir })
        .unwrap_or(".");
    let mut out = Vec::new();
    let base_s = base.to_string();
    for entry in jet_std_fs_walk(&base_s)? {
        if glob_match(pattern.as_str(), entry.path.as_str()) {
            out.push(entry.path);
        }
    }
    out.sort();
    Ok(out)
}
fn glob_match(pattern: &str, text: &str) -> bool {
    fn inner(p: &[u8], t: &[u8]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        match p[0] {
            b'*' => inner(&p[1..], t) || (!t.is_empty() && inner(p, &t[1..])),
            b'?' => !t.is_empty() && inner(&p[1..], &t[1..]),
            c => !t.is_empty() && c == t[0] && inner(&p[1..], &t[1..]),
        }
    }
    inner(pattern.as_bytes(), text.as_bytes())
}
fn jet_std_fs_read_at(path: &String, offset: i64, len: i64) -> Result<Vec<u8>, jet_std::IoError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| jet_std::io_error(path, e))?;
    f.seek(SeekFrom::Start(offset.max(0) as u64))
        .map_err(|e| jet_std::io_error(path, e))?;
    let mut buf = vec![0u8; len.max(0) as usize];
    let n = f.read(&mut buf).map_err(|e| jet_std::io_error(path, e))?;
    buf.truncate(n);
    Ok(buf)
}
fn jet_std_fs_write_at(
    path: &String,
    offset: i64,
    bytes: &Vec<u8>,
) -> Result<(), jet_std::IoError> {
    use std::io::{Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    f.seek(SeekFrom::Start(offset.max(0) as u64))
        .map_err(|e| jet_std::io_error(path, e))?;
    f.write_all(bytes).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_fsync(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_write_atomic(path: &String, bytes: &Vec<u8>) -> Result<(), jet_std::IoError> {
    jet_path_write_atomic(&jet_path_from(path), bytes)
}
fn jet_std_fs_temp_dir(prefix: &String) -> Result<jet_std::TempDir, jet_std::IoError> {
    let path = jet_temp_path(prefix);
    std::fs::create_dir(&path).map_err(|e| jet_std::io_error(&path, e))?;
    Ok(jet_std::TempDir {
        path,
        cleanup: std::rc::Rc::new(()),
    })
}
fn jet_std_fs_temp_file(prefix: &String) -> Result<jet_std::TempFile, jet_std::IoError> {
    let path = jet_temp_path(prefix);
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|e| jet_std::io_error(&path, e))?;
    Ok(jet_std::TempFile {
        path,
        cleanup: std::rc::Rc::new(()),
    })
}
fn jet_std_fs_lock(path: &String) -> Result<jet_std::FileLock, jet_std::IoError> {
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    Ok(jet_std::FileLock {
        path: path.clone(),
        cleanup: std::rc::Rc::new(()),
    })
}
fn jet_temp_path(prefix: &String) -> String {
    let clean: String = prefix
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(format!("{}_{}_{}", clean, std::process::id(), nanos))
        .to_string_lossy()
        .to_string()
}
fn jet_watcher_files(path: &String) -> Result<jet_std::WatchHandle, jet_std::IoError> {
    jet_std::WatchHandle::files(path.clone())
}
fn jet_watcher_process_pid(pid: i64) -> jet_std::WatchHandle {
    jet_std::WatchHandle::process_pid(pid)
}
fn jet_watcher_port(host: &String, port: i64) -> jet_std::WatchHandle {
    jet_std::WatchHandle::port(host.clone(), port)
}
fn jet_watcher_set() -> jet_std::WatchSet {
    jet_std::WatchSet::new()
}

fn jet_std_io_args() -> Vec<String> {
    std::env::args().collect()
}
fn jet_std_io_input(prompt: Option<&String>) -> Result<String, jet_std::IoError> {
    use std::io::Write;
    if let Some(p) = prompt {
        print!("{}", p);
        std::io::stdout()
            .flush()
            .map_err(|e| jet_std::IoError::Other {
                message: e.to_string(),
            })?;
    }
    let mut s = String::new();
    std::io::stdin()
        .read_line(&mut s)
        .map_err(|e| jet_std::IoError::Other {
            message: e.to_string(),
        })?;
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    Ok(s)
}
fn jet_std_io_read_all_input() -> Result<String, jet_std::IoError> {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .map_err(|e| jet_std::IoError::Other {
            message: e.to_string(),
        })?;
    Ok(s)
}

// D-STDIN1=A: streaming line-by-line stdin.
struct JetStdinReader {
    inner: std::io::BufReader<std::io::Stdin>,
}
fn jet_std_io_stdin() -> JetStdinReader {
    JetStdinReader {
        inner: std::io::BufReader::new(std::io::stdin()),
    }
}
fn jet_std_io_stdin_read_line(r: &mut JetStdinReader) -> Result<Option<String>, jet_std::IoError> {
    use std::io::BufRead;
    let mut line = String::new();
    match r.inner.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Some(line))
        }
        Err(e) => Err(jet_std::IoError::Other {
            message: e.to_string(),
        }),
    }
}

// D-COREIO1=A: stdout/stderr stream handles and TTY-aware terminal helpers.
struct JetStdout;
struct JetStderr;

fn jet_stdio_error(e: std::io::Error) -> jet_std::IoError {
    jet_std::IoError::Other {
        message: e.to_string(),
    }
}

fn jet_std_io_stdout() -> JetStdout {
    JetStdout
}
fn jet_std_io_stderr() -> JetStderr {
    JetStderr
}
fn jet_std_io_stdout_write(_s: &mut JetStdout, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stdout()
        .write_all(text.as_bytes())
        .map_err(jet_stdio_error)
}
fn jet_std_io_stdout_write_line(_s: &mut JetStdout, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    let mut out = std::io::stdout();
    out.write_all(text.as_bytes()).map_err(jet_stdio_error)?;
    out.write_all(b"\n").map_err(jet_stdio_error)
}
fn jet_std_io_stdout_write_bytes(_s: &mut JetStdout, bytes: &Vec<u8>) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stdout().write_all(bytes).map_err(jet_stdio_error)
}
fn jet_std_io_stdout_flush(_s: &mut JetStdout) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stdout().flush().map_err(jet_stdio_error)
}
fn jet_std_io_stdout_is_tty(_s: &JetStdout) -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}
fn jet_std_io_stderr_write(_s: &mut JetStderr, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stderr()
        .write_all(text.as_bytes())
        .map_err(jet_stdio_error)
}
fn jet_std_io_stderr_write_line(_s: &mut JetStderr, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    let mut out = std::io::stderr();
    out.write_all(text.as_bytes()).map_err(jet_stdio_error)?;
    out.write_all(b"\n").map_err(jet_stdio_error)
}
fn jet_std_io_stderr_write_bytes(_s: &mut JetStderr, bytes: &Vec<u8>) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stderr().write_all(bytes).map_err(jet_stdio_error)
}
fn jet_std_io_stderr_flush(_s: &mut JetStderr) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stderr().flush().map_err(jet_stdio_error)
}
fn jet_std_io_stderr_is_tty(_s: &JetStderr) -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

fn jet_env_int(name: &str) -> Option<i64> {
    std::env::var(name).ok()?.parse::<i64>().ok().filter(|n| *n > 0)
}
fn jet_terminal_size_from_stty() -> Option<(i64, i64)> {
    let out = std::process::Command::new("stty").arg("size").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let mut parts = text.split_whitespace();
    let rows = parts.next()?.parse::<i64>().ok()?;
    let cols = parts.next()?.parse::<i64>().ok()?;
    (rows > 0 && cols > 0).then_some((cols, rows))
}
fn jet_std_io_terminal_width() -> i64 {
    jet_env_int("COLUMNS")
        .or_else(|| jet_terminal_size_from_stty().map(|(w, _)| w))
        .unwrap_or(80)
}
fn jet_std_io_terminal_height() -> i64 {
    jet_env_int("LINES")
        .or_else(|| jet_terminal_size_from_stty().map(|(_, h)| h))
        .unwrap_or(24)
}
fn jet_style_code(name: &str) -> Option<&'static str> {
    match name {
        "black" => Some("30"),
        "red" => Some("31"),
        "green" => Some("32"),
        "yellow" => Some("33"),
        "blue" => Some("34"),
        "magenta" => Some("35"),
        "cyan" => Some("36"),
        "white" => Some("37"),
        "bold" => Some("1"),
        "dim" => Some("2"),
        _ => None,
    }
}
fn jet_style_enabled() -> bool {
    use std::io::IsTerminal;
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
        && std::io::stdout().is_terminal()
}
fn jet_std_io_style(style: &String, text: &String) -> String {
    if jet_style_enabled() {
        jet_std_io_style_force(style, text)
    } else {
        text.clone()
    }
}
fn jet_std_io_style_force(style: &String, text: &String) -> String {
    match jet_style_code(style.as_str()) {
        Some(code) => format!("\x1b[{code}m{text}\x1b[0m"),
        None => text.clone(),
    }
}
fn jet_std_io_progress(text: &String) -> Result<(), jet_std::IoError> {
    use std::io::{IsTerminal, Write};
    let mut out = std::io::stdout();
    if out.is_terminal() {
        out.write_all(b"\r").map_err(jet_stdio_error)?;
        out.write_all(text.as_bytes()).map_err(jet_stdio_error)?;
        out.flush().map_err(jet_stdio_error)
    } else {
        out.write_all(text.as_bytes()).map_err(jet_stdio_error)?;
        out.write_all(b"\n").map_err(jet_stdio_error)
    }
}

fn jet_std_env_get(name: &String) -> Option<String> {
    std::env::var(name).ok()
}
fn jet_std_env_set(name: &String, value: &String) {
    std::env::set_var(name, value);
}
fn jet_std_env_current_dir() -> Result<String, jet_std::IoError> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::IoError::Other {
            message: e.to_string(),
        })
}
fn jet_std_env_home_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
}

fn jet_std_os_name() -> String {
    std::env::consts::OS.to_string()
}
fn jet_std_os_family() -> String {
    std::env::consts::FAMILY.to_string()
}
fn jet_std_os_arch() -> String {
    std::env::consts::ARCH.to_string()
}
fn jet_std_os_cpu_count() -> i64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i64)
        .unwrap_or(1)
}
fn jet_std_os_temp_dir() -> String {
    std::env::temp_dir().to_string_lossy().to_string()
}
fn jet_std_os_executable() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}
fn jet_std_os_pid() -> i64 {
    std::process::id() as i64
}
fn jet_std_os_hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .unwrap_or_else(|| "localhost".to_string())
}
fn jet_std_os_username() -> String {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
        .unwrap_or_default()
}
fn jet_std_os_set_current_dir(path: &String) -> Result<(), jet_std::IoError> {
    std::env::set_current_dir(path).map_err(|e| jet_std::IoError::Other {
        message: e.to_string(),
    })
}

#[cfg(unix)]
mod jet_os_unix {
    static HIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    extern "C" fn mark(_: i32) {
        HIT.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }

    pub fn on_interrupt<F>(handler: F)
    where
        F: Fn() + Send + 'static,
    {
        const SIGINT: i32 = 2;
        unsafe {
            signal(SIGINT, mark);
        }
        std::thread::spawn(move || loop {
            if HIT.swap(false, std::sync::atomic::Ordering::SeqCst) {
                handler();
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        });
    }
}

#[cfg(unix)]
fn jet_std_os_on_interrupt<F>(handler: F)
where
    F: Fn() + Send + 'static,
{
    jet_os_unix::on_interrupt(handler);
}

#[cfg(not(unix))]
fn jet_std_os_on_interrupt<F>(handler: F)
where
    F: Fn() + Send + 'static,
{
    let _ = handler;
}

fn jet_testing_snap(name: &String, actual: &String) -> bool {
    let path = std::path::Path::new("__snapshots__").join(format!("{}.snap", sanitize_test_name(name)));
    let update = std::env::var("JET_UPDATE_SNAPSHOTS").ok().as_deref() == Some("1");
    if update || !path.is_file() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        return std::fs::write(&path, actual).is_ok();
    }
    std::fs::read_to_string(path).map(|s| s == *actual).unwrap_or(false)
}

fn jet_testing_golden(path: &String, actual: &String) -> bool {
    std::fs::read_to_string(path).map(|s| s == *actual).unwrap_or(false)
}

fn jet_testing_fixture(path: &String) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn jet_testing_temp_dir(prefix: &String) -> String {
    let safe = sanitize_test_name(prefix);
    let path = std::env::temp_dir().join(format!("jet_test_{}_{}", safe, std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    let _ = std::fs::create_dir_all(&path);
    path.to_string_lossy().into_owned()
}

fn jet_testing_corpus(path: &String) -> Vec<String> {
    let mut entries = Vec::new();
    if let Ok(read) = std::fs::read_dir(path) {
        let mut paths = read.filter_map(|e| e.ok().map(|e| e.path())).collect::<Vec<_>>();
        paths.sort();
        for p in paths {
            if p.is_file() {
                if let Ok(text) = std::fs::read_to_string(p) {
                    entries.push(text);
                }
            }
        }
    }
    entries
}

fn jet_testing_bench_budget(_name: &String, max_ns: i64) -> bool {
    max_ns >= 0
}

fn sanitize_test_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "case".to_string()
    } else {
        out
    }
}

fn jet_std_process_exit(code: i64) -> ! {
    std::process::exit(code as i32)
}
fn io_other(e: impl ToString) -> jet_std::IoError {
    jet_std::IoError::Other {
        message: e.to_string(),
    }
}
fn jet_std_process_cmd(cmd: &Vec<String>) -> jet_std::ProcessSpec {
    jet_std::ProcessSpec {
        cmd: cmd.clone(),
        cwd: None,
        env_clear: false,
        env_set: Vec::new(),
        env_remove: Vec::new(),
        stdin_text: None,
        stdout: jet_std::ProcessStreamMode::Capture,
        stderr: jet_std::ProcessStreamMode::Capture,
        timeout_ms: None,
        output_limit: None,
        detached: false,
    }
}
fn jet_std_process_run(cmd: &Vec<String>) -> Result<jet_std::ProcessResult, jet_std::IoError> {
    jet_process_spec_run_inner(&jet_std_process_cmd(cmd))
}
fn jet_std_process_pipeline(
    commands: &Vec<Vec<String>>,
) -> Result<jet_std::ProcessResult, jet_std::IoError> {
    if commands.is_empty() {
        return Err(jet_std::IoError::Other {
            message: "process.pipeline needs at least one command".to_string(),
        });
    }
    let mut children: Vec<std::process::Child> = Vec::new();
    let mut prev_stdout: Option<std::process::ChildStdout> = None;
    for cmd in commands {
        if cmd.is_empty() {
            return Err(jet_std::IoError::Other {
                message: "process.pipeline command is empty".to_string(),
            });
        }
        let mut command = std::process::Command::new(&cmd[0]);
        command.args(&cmd[1..]);
        if let Some(stdout) = prev_stdout.take() {
            command.stdin(std::process::Stdio::from(stdout));
        }
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        let mut child = command.spawn().map_err(io_other)?;
        prev_stdout = child.stdout.take();
        children.push(child);
    }
    let mut output = String::new();
    if let Some(mut stdout) = prev_stdout.take() {
        std::io::Read::read_to_string(&mut stdout, &mut output).map_err(io_other)?;
    }
    let mut errors = String::new();
    let mut code = 0;
    for mut child in children {
        if let Some(mut stderr) = child.stderr.take() {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut stderr, &mut text).map_err(io_other)?;
            errors.push_str(&text);
        }
        let status = child.wait().map_err(io_other)?;
        code = status.code().unwrap_or(-1) as i64;
        if !status.success() {
            break;
        }
    }
    Ok(jet_std::ProcessResult {
        code,
        success: code == 0,
        signal: None,
        timed_out: false,
        output,
        errors,
    })
}
fn jet_process_stream_mode(mode: &String) -> jet_std::ProcessStreamMode {
    match mode.as_str() {
        "inherit" => jet_std::ProcessStreamMode::Inherit,
        "discard" => jet_std::ProcessStreamMode::Discard,
        "capture" | "stream" => jet_std::ProcessStreamMode::Capture,
        _ => jet_std::ProcessStreamMode::Capture,
    }
}
fn jet_process_spec_with_mode(
    mut spec: jet_std::ProcessSpec,
    stdout: bool,
    mode: jet_std::ProcessStreamMode,
) -> jet_std::ProcessSpec {
    if stdout {
        spec.stdout = mode;
    } else {
        spec.stderr = mode;
    }
    spec
}
fn jet_process_spec_cwd(mut spec: jet_std::ProcessSpec, cwd: &String) -> jet_std::ProcessSpec {
    spec.cwd = Some(cwd.clone());
    spec
}
fn jet_process_spec_env(
    mut spec: jet_std::ProcessSpec,
    name: &String,
    value: &String,
) -> jet_std::ProcessSpec {
    spec.env_set.push((name.clone(), value.clone()));
    spec
}
fn jet_process_spec_env_remove(
    mut spec: jet_std::ProcessSpec,
    name: &String,
) -> jet_std::ProcessSpec {
    spec.env_remove.push(name.clone());
    spec
}
fn jet_process_spec_env_clear(mut spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    spec.env_clear = true;
    spec
}
fn jet_process_spec_stdin_text(
    mut spec: jet_std::ProcessSpec,
    text: &String,
) -> jet_std::ProcessSpec {
    spec.stdin_text = Some(text.clone());
    spec
}
fn jet_process_spec_stdout(spec: jet_std::ProcessSpec, mode: &String) -> jet_std::ProcessSpec {
    jet_process_spec_with_mode(spec, true, jet_process_stream_mode(mode))
}
fn jet_process_spec_stderr(spec: jet_std::ProcessSpec, mode: &String) -> jet_std::ProcessSpec {
    jet_process_spec_with_mode(spec, false, jet_process_stream_mode(mode))
}
fn jet_process_spec_stdout_capture(spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    jet_process_spec_with_mode(spec, true, jet_std::ProcessStreamMode::Capture)
}
fn jet_process_spec_stdout_inherit(spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    jet_process_spec_with_mode(spec, true, jet_std::ProcessStreamMode::Inherit)
}
fn jet_process_spec_stdout_discard(spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    jet_process_spec_with_mode(spec, true, jet_std::ProcessStreamMode::Discard)
}
fn jet_process_spec_stderr_capture(spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    jet_process_spec_with_mode(spec, false, jet_std::ProcessStreamMode::Capture)
}
fn jet_process_spec_stderr_inherit(spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    jet_process_spec_with_mode(spec, false, jet_std::ProcessStreamMode::Inherit)
}
fn jet_process_spec_stderr_discard(spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    jet_process_spec_with_mode(spec, false, jet_std::ProcessStreamMode::Discard)
}
fn jet_process_spec_timeout_ms(
    mut spec: jet_std::ProcessSpec,
    timeout_ms: i64,
) -> jet_std::ProcessSpec {
    spec.timeout_ms = Some(timeout_ms.max(0));
    spec
}
fn jet_process_spec_output_limit(
    mut spec: jet_std::ProcessSpec,
    output_limit: i64,
) -> jet_std::ProcessSpec {
    spec.output_limit = Some(output_limit.max(0));
    spec
}
fn jet_process_spec_detached(mut spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    spec.detached = true;
    spec
}
fn jet_process_stdio(mode: &jet_std::ProcessStreamMode) -> std::process::Stdio {
    match mode {
        jet_std::ProcessStreamMode::Capture => std::process::Stdio::piped(),
        jet_std::ProcessStreamMode::Inherit => std::process::Stdio::inherit(),
        jet_std::ProcessStreamMode::Discard => std::process::Stdio::null(),
    }
}
fn jet_process_command(
    spec: &jet_std::ProcessSpec,
) -> Result<std::process::Command, jet_std::IoError> {
    if spec.cmd.is_empty() {
        return Err(jet_std::IoError::Other {
            message: "process command needs at least one word".to_string(),
        });
    }
    let mut command = std::process::Command::new(&spec.cmd[0]);
    command.args(&spec.cmd[1..]);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    if spec.env_clear {
        command.env_clear();
    }
    for name in &spec.env_remove {
        command.env_remove(name);
    }
    for (name, value) in &spec.env_set {
        command.env(name, value);
    }
    command.stdin(if spec.stdin_text.is_some() {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });
    command.stdout(jet_process_stdio(&spec.stdout));
    command.stderr(jet_process_stdio(&spec.stderr));
    Ok(command)
}
fn jet_process_spec_spawn(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessChild, jet_std::IoError> {
    let mut command = jet_process_command(spec)?;
    if spec.detached {
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
    }
    let mut child = command.spawn().map_err(io_other)?;
    if let Some(text) = &spec.stdin_text {
        if let Some(stdin) = child.stdin.as_mut() {
            std::io::Write::write_all(stdin, text.as_bytes()).map_err(io_other)?;
        }
        child.stdin.take();
    }
    Ok(jet_std::ProcessChild {
        stdin: std::rc::Rc::new(std::cell::RefCell::new(child.stdin.take())),
        stdout: std::rc::Rc::new(std::cell::RefCell::new(
            child.stdout.take().map(std::io::BufReader::new),
        )),
        stderr: std::rc::Rc::new(std::cell::RefCell::new(
            child.stderr.take().map(std::io::BufReader::new),
        )),
        inner: std::rc::Rc::new(std::cell::RefCell::new(Some(child))),
        timeout_ms: spec.timeout_ms,
        started: std::time::Instant::now(),
    })
}
fn jet_process_collect_output(
    child: &jet_std::ProcessChild,
) -> Result<(String, String), jet_std::IoError> {
    let mut output = String::new();
    let mut errors = String::new();
    if let Some(mut stdout) = child.stdout.borrow_mut().take() {
        std::io::Read::read_to_string(&mut stdout, &mut output).map_err(io_other)?;
    }
    if let Some(mut stderr) = child.stderr.borrow_mut().take() {
        std::io::Read::read_to_string(&mut stderr, &mut errors).map_err(io_other)?;
    }
    Ok((output, errors))
}
fn jet_process_spec_run_inner(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessResult, jet_std::IoError> {
    let child = jet_process_spec_spawn(spec)?;
    let result = jet_process_child_wait(&child)?;
    if let Some(limit) = spec.output_limit {
        if (result.output.len() + result.errors.len()) as i64 > limit {
            return Err(jet_std::IoError::Other {
                message: "process output exceeded output_limit".to_string(),
            });
        }
    }
    Ok(result)
}
fn jet_process_spec_run(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessResult, jet_std::IoError> {
    jet_process_spec_run_inner(spec)
}
fn jet_process_child_id(child: &jet_std::ProcessChild) -> i64 {
    child
        .inner
        .borrow()
        .as_ref()
        .map(|c| c.id() as i64)
        .unwrap_or(0)
}
fn jet_process_child_wait(
    child: &jet_std::ProcessChild,
) -> Result<jet_std::ProcessResult, jet_std::IoError> {
    let mut timed_out = false;
    let status = loop {
        let mut slot = child.inner.borrow_mut();
        let Some(inner) = slot.as_mut() else {
            let (output, errors) = jet_process_collect_output(child)?;
            return Ok(jet_std::ProcessResult {
                code: 0,
                success: true,
                signal: None,
                timed_out: false,
                output,
                errors,
            });
        };
        if let Some(status) = inner.try_wait().map_err(io_other)? {
            break status;
        }
        if let Some(timeout) = child.timeout_ms {
            if child.started.elapsed() >= std::time::Duration::from_millis(timeout as u64) {
                inner.kill().map_err(io_other)?;
                timed_out = true;
                break inner.wait().map_err(io_other)?;
            }
        }
        drop(slot);
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    child.inner.borrow_mut().take();
    let (output, errors) = jet_process_collect_output(child)?;
    let code = status.code().unwrap_or(-1) as i64;
    Ok(jet_std::ProcessResult {
        code,
        success: status.success(),
        signal: None,
        timed_out,
        output,
        errors,
    })
}
fn jet_process_child_kill(child: &jet_std::ProcessChild) -> Result<(), jet_std::IoError> {
    if let Some(inner) = child.inner.borrow_mut().as_mut() {
        inner.kill().map_err(io_other)?;
    }
    Ok(())
}
fn jet_process_child_terminate(child: &jet_std::ProcessChild) -> Result<(), jet_std::IoError> {
    jet_process_child_kill(child)
}
fn jet_process_child_interrupt(child: &jet_std::ProcessChild) -> Result<(), jet_std::IoError> {
    jet_process_child_kill(child)
}
fn jet_process_child_write_stdin(
    child: &jet_std::ProcessChild,
    text: &String,
) -> Result<(), jet_std::IoError> {
    if let Some(stdin) = child.stdin.borrow_mut().as_mut() {
        std::io::Write::write_all(stdin, text.as_bytes()).map_err(io_other)?;
    }
    Ok(())
}
fn jet_process_child_read_line<R: std::io::Read>(
    reader: &mut Option<std::io::BufReader<R>>,
) -> Result<Option<String>, jet_std::IoError> {
    let Some(reader) = reader.as_mut() else {
        return Ok(None);
    };
    let mut line = String::new();
    let n = std::io::BufRead::read_line(reader, &mut line).map_err(io_other)?;
    if n == 0 {
        Ok(None)
    } else {
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        Ok(Some(line))
    }
}
fn jet_process_child_read_stdout_line(
    child: &jet_std::ProcessChild,
) -> Result<Option<String>, jet_std::IoError> {
    jet_process_child_read_line(&mut child.stdout.borrow_mut())
}
fn jet_process_child_read_stderr_line(
    child: &jet_std::ProcessChild,
) -> Result<Option<String>, jet_std::IoError> {
    jet_process_child_read_line(&mut child.stderr.borrow_mut())
}

fn jet_std_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}
fn jet_std_math_pow(a: f64, b: f64) -> f64 {
    a.powf(b)
}
fn jet_std_math_floor(x: f64) -> f64 {
    x.floor()
}
fn jet_std_math_ceil(x: f64) -> f64 {
    x.ceil()
}
fn jet_std_math_round(x: f64) -> i64 {
    x.round() as i64
}
fn jet_std_math_sign(x: f64) -> i64 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}
fn jet_std_math_checked_pow(base: i64, exp: i64) -> Option<i64> {
    if exp < 0 {
        return None;
    }
    base.checked_pow(exp as u32)
}
fn jet_std_math_int_pow(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0;
    }
    base.saturating_pow(exp as u32)
}
fn jet_std_math_gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
fn jet_std_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        0
    } else {
        (a / jet_std_math_gcd(a, b)).saturating_mul(b).abs()
    }
}
// D-FLOATW1 (ratified 2026-06-22): F32 variants — sqrt(F32)->F32, pow(F32,F32)->F32 etc.
// F32 is a real precision choice, not just storage; no silent widening to f64 (I3).
fn jet_std_math_sqrt_f32(x: f32) -> f32 {
    x.sqrt()
}
fn jet_std_math_pow_f32(a: f32, b: f32) -> f32 {
    a.powf(b)
}
fn jet_std_math_floor_f32(x: f32) -> f32 {
    x.floor()
}
fn jet_std_math_ceil_f32(x: f32) -> f32 {
    x.ceil()
}

thread_local! { static JET_RNG: std::cell::Cell<u64> = std::cell::Cell::new(0x4d595df4d0f33173); }
fn jet_rng_next() -> u64 {
    JET_RNG.with(|cell| {
        let mut x = cell.get();
        x ^= x << 7;
        x ^= x >> 9;
        x = x.wrapping_mul(0x9e3779b97f4a7c15);
        cell.set(x);
        x
    })
}
fn jet_std_random_seed(n: i64) {
    JET_RNG.with(|cell| cell.set(n as u64));
}
fn jet_std_random_int(low: i64, high: i64) -> i64 {
    if high <= low {
        return low;
    }
    low + (jet_rng_next() % ((high - low + 1) as u64)) as i64
}
fn jet_std_random_float() -> f64 {
    (jet_rng_next() as f64) / (u64::MAX as f64)
}
fn jet_std_random_float_open() -> f64 {
    let x = jet_std_random_float();
    if x <= 0.0 { f64::MIN_POSITIVE } else { x }
}
fn jet_std_random_float_range(low: f64, high: f64) -> f64 {
    if !(high > low) {
        return low;
    }
    low + (high - low) * jet_std_random_float()
}
fn jet_std_random_bool(p: f64) -> bool {
    if p <= 0.0 || p.is_nan() {
        false
    } else if p >= 1.0 {
        true
    } else {
        jet_std_random_float() < p
    }
}
fn jet_std_random_normal(mean: f64, stddev: f64) -> f64 {
    let u1 = jet_std_random_float_open();
    let u2 = jet_std_random_float();
    let z0 = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    mean + z0 * stddev.max(0.0)
}
fn jet_std_random_exponential(lambda: f64) -> f64 {
    if lambda <= 0.0 || lambda.is_nan() {
        return 0.0;
    }
    -jet_std_random_float_open().ln() / lambda
}
fn jet_std_random_pick<T: Clone>(xs: &Vec<T>) -> Option<T> {
    if xs.is_empty() {
        None
    } else {
        Some(xs[jet_std_random_int(0, xs.len() as i64 - 1) as usize].clone())
    }
}
fn jet_std_random_weighted_pick<T: Clone>(xs: &Vec<T>, weights: &Vec<f64>) -> Option<T> {
    if xs.is_empty() || xs.len() != weights.len() {
        return None;
    }
    let mut total = 0.0;
    for &w in weights {
        if w.is_finite() && w > 0.0 {
            total += w;
        }
    }
    if total <= 0.0 {
        return None;
    }
    let mut needle = jet_std_random_float_range(0.0, total);
    for (item, &weight) in xs.iter().zip(weights.iter()) {
        let w = if weight.is_finite() && weight > 0.0 { weight } else { 0.0 };
        if needle < w {
            return Some(item.clone());
        }
        needle -= w;
    }
    xs.last().cloned()
}
fn jet_std_random_sample<T: Clone>(xs: &Vec<T>, k: i64) -> Vec<T> {
    let want = (k.max(0) as usize).min(xs.len());
    let mut pool = xs.clone();
    for i in 0..want {
        let j = jet_std_random_int(i as i64, pool.len() as i64 - 1) as usize;
        pool.swap(i, j);
    }
    pool.truncate(want);
    pool
}
fn jet_std_random_shuffle<T>(xs: &mut Vec<T>) {
    let len = xs.len();
    for i in (1..len).rev() {
        let j = jet_std_random_int(0, i as i64) as usize;
        xs.swap(i, j);
    }
}
// D-RANDSPLIT1=A: PRNG bytes via the ambient SplitMix64 state — fast, seedable,
// NOT cryptographically secure. Use for simulation, testing, or shuffles only.
fn jet_std_random_bytes(n: i64) -> Vec<u8> {
    let n = n.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(jet_rng_next() as u8);
    }
    out
}
fn jet_std_random_split(seed: i64) -> jet_std::Rng {
    let mixed = (seed as u64) ^ jet_rng_next().rotate_left(17);
    jet_std::Rng { state: mixed }
}
// D-RANDSPLIT1=A: CSPRNG bytes via /dev/urandom (POSIX) with SplitMix64 fallback.
// Cryptographically secure — use for tokens, keys, nonces, and secrets.
fn jet_std_crypto_random_bytes(n: i64) -> Vec<u8> {
    let n = n.max(0) as usize;
    let mut out = vec![0u8; n];
    jet_uuid_fill_random(&mut out);
    out
}

fn jet_std_time_now() -> i64 {
    if let Ok(s) = std::env::var("LEX_TEST_EPOCH") {
        if let Ok(n) = s.parse::<i64>() {
            return n;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

thread_local! {
    static JET_CTX_DEADLINE_MS: std::cell::Cell<Option<i64>> = std::cell::Cell::new(None);
}

struct JetDeadlineGuard {
    saved: Option<i64>,
}

impl Drop for JetDeadlineGuard {
    fn drop(&mut self) {
        JET_CTX_DEADLINE_MS.with(|c| c.set(self.saved));
    }
}

fn jet_ctx_deadline_ms() -> Option<i64> {
    JET_CTX_DEADLINE_MS.with(|c| c.get())
}

fn jet_ctx_push_deadline(deadline_ms: i64) -> JetDeadlineGuard {
    let saved = JET_CTX_DEADLINE_MS.with(|c| c.get());
    JET_CTX_DEADLINE_MS.with(|c| c.set(Some(deadline_ms)));
    JetDeadlineGuard { saved }
}

fn jet_deadline_remaining_ms() -> Option<i64> {
    let deadline = jet_ctx_deadline_ms()?;
    Some(deadline.saturating_sub(jet_std_time_now()))
}

fn jet_deadline_exceeded(wait_kind: &str) -> ! {
    eprintln!("Error [E3003]: deadline exceeded while waiting in {wait_kind}");
    eprintln!(
        "Why: this wait point observed the task context deadline from `#Context(deadline: …)`"
    );
    eprintln!("Fix: raise the deadline budget or shorten the work before this wait point");
    std::process::exit(70);
}

fn jet_deadline_check(wait_kind: &str) {
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        jet_deadline_exceeded(wait_kind);
    }
}

fn jet_std_time_sleep(millis: i64) {
    let want = millis.max(0);
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded("time sleep");
        }
        if want > remaining {
            jet_scheduler_sleep_ms(remaining as u64);
            jet_deadline_exceeded("time sleep");
        }
    }
    jet_scheduler_sleep_ms(want as u64);
    jet_deadline_check("time sleep");
}
fn jet_std_time_start() -> jet_std::Stopwatch {
    jet_std::Stopwatch {
        start: std::time::Instant::now(),
    }
}

// ── D-DET1: deterministic injected Clock / Rng capabilities ───────────────────
// Built from a caller-supplied seed (a pure value), so a `@Pure fn` may read
// time/randomness THROUGH the handle and stay reproducible. No wall-clock or
// OS-RNG read; std-only (no external crate, I6).
fn jet_std_clock_new(seed: i64) -> jet_std::Clock {
    jet_std::Clock { now: seed }
}
fn jet_clock_now(c: &jet_std::Clock) -> i64 {
    c.now
}
fn jet_clock_tick(c: &mut jet_std::Clock, ms: i64) -> i64 {
    c.now = c.now.wrapping_add(ms);
    c.now
}
// D-DET-CAPAPI: `clock.advance(to_ms)` sets the clock to an ABSOLUTE instant;
// `clock.wait(d)` advances by a `Duration` (relative). Both return the new value.
fn jet_clock_advance(c: &mut jet_std::Clock, to_ms: i64) -> i64 {
    c.now = to_ms;
    c.now
}
fn jet_clock_wait(c: &mut jet_std::Clock, d: &jet_std::Duration) -> i64 {
    c.now = c.now.wrapping_add(d.ms);
    c.now
}
fn jet_std_rng_new(seed: i64) -> jet_std::Rng {
    jet_std::Rng { state: seed as u64 }
}
// SplitMix64 step — a small, well-distributed deterministic PRNG (public domain).
fn jet_det_rng_next(r: &mut jet_std::Rng) -> u64 {
    r.state = r.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = r.state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
fn jet_rng_int(r: &mut jet_std::Rng, lo: i64, hi: i64) -> i64 {
    if hi <= lo {
        return lo;
    }
    let span = (hi - lo + 1) as u64;
    lo + (jet_det_rng_next(r) % span) as i64
}
fn jet_rng_float(r: &mut jet_std::Rng) -> f64 {
    // 53-bit mantissa → [0, 1).
    (jet_det_rng_next(r) >> 11) as f64 / (1u64 << 53) as f64
}
fn jet_rng_float_open(r: &mut jet_std::Rng) -> f64 {
    let x = jet_rng_float(r);
    if x <= 0.0 { f64::MIN_POSITIVE } else { x }
}
fn jet_rng_float_range(r: &mut jet_std::Rng, low: f64, high: f64) -> f64 {
    if !(high > low) {
        return low;
    }
    low + (high - low) * jet_rng_float(r)
}
// D-DET-CAPAPI: the widened deterministic draws — coin, uniform choice, in-place
// Fisher–Yates shuffle. Each advances the SplitMix64 stream, so they are
// reproducible from the seed and mirror the ambient `random.*` set.
fn jet_rng_bool(r: &mut jet_std::Rng) -> bool {
    (jet_det_rng_next(r) & 1) == 1
}
fn jet_rng_bool_p(r: &mut jet_std::Rng, p: f64) -> bool {
    if p <= 0.0 || p.is_nan() {
        false
    } else if p >= 1.0 {
        true
    } else {
        jet_rng_float(r) < p
    }
}
fn jet_rng_normal(r: &mut jet_std::Rng, mean: f64, stddev: f64) -> f64 {
    let u1 = jet_rng_float_open(r);
    let u2 = jet_rng_float(r);
    let z0 = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    mean + z0 * stddev.max(0.0)
}
fn jet_rng_exponential(r: &mut jet_std::Rng, lambda: f64) -> f64 {
    if lambda <= 0.0 || lambda.is_nan() {
        return 0.0;
    }
    -jet_rng_float_open(r).ln() / lambda
}
fn jet_rng_bytes(r: &mut jet_std::Rng, n: i64) -> Vec<u8> {
    let n = n.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(jet_det_rng_next(r) as u8);
    }
    out
}
fn jet_rng_split(r: &mut jet_std::Rng) -> jet_std::Rng {
    jet_std::Rng { state: jet_det_rng_next(r) }
}
fn jet_rng_pick<T: Clone>(r: &mut jet_std::Rng, xs: &Vec<T>) -> Option<T> {
    if xs.is_empty() {
        None
    } else {
        Some(xs[jet_rng_int(r, 0, xs.len() as i64 - 1) as usize].clone())
    }
}
fn jet_rng_weighted_pick<T: Clone>(
    r: &mut jet_std::Rng,
    xs: &Vec<T>,
    weights: &Vec<f64>,
) -> Option<T> {
    if xs.is_empty() || xs.len() != weights.len() {
        return None;
    }
    let mut total = 0.0;
    for &w in weights {
        if w.is_finite() && w > 0.0 {
            total += w;
        }
    }
    if total <= 0.0 {
        return None;
    }
    let mut needle = jet_rng_float_range(r, 0.0, total);
    for (item, &weight) in xs.iter().zip(weights.iter()) {
        let w = if weight.is_finite() && weight > 0.0 { weight } else { 0.0 };
        if needle < w {
            return Some(item.clone());
        }
        needle -= w;
    }
    xs.last().cloned()
}
fn jet_rng_sample<T: Clone>(r: &mut jet_std::Rng, xs: &Vec<T>, k: i64) -> Vec<T> {
    let want = (k.max(0) as usize).min(xs.len());
    let mut pool = xs.clone();
    for i in 0..want {
        let j = jet_rng_int(r, i as i64, pool.len() as i64 - 1) as usize;
        pool.swap(i, j);
    }
    pool.truncate(want);
    pool
}
fn jet_rng_shuffle<T>(r: &mut jet_std::Rng, xs: &mut Vec<T>) {
    let len = xs.len();
    for i in (1..len).rev() {
        let j = jet_rng_int(r, 0, i as i64) as usize;
        xs.swap(i, j);
    }
}
// D-SOLVER-LIB1=A: explicit finite solver state. Constraints are ordinary Bool
// values recorded in insertion order; no unification or hidden backtracking.
fn jet_solver_new(seed: i64) -> jet_std::Solver {
    jet_std::Solver {
        seed,
        checked: 0,
        failures: 0,
    }
}
fn jet_solver_require(s: &mut jet_std::Solver, ok: bool) {
    s.checked += 1;
    if !ok {
        s.failures += 1;
    }
}
fn jet_solver_failure_count(s: &jet_std::Solver) -> i64 {
    s.failures
}
fn jet_solver_status(s: &jet_std::Solver) -> String {
    if s.failures == 0 {
        "ok".to_string()
    } else {
        "failed".to_string()
    }
}
// D-DET-CAPAPI: `Duration` constructors + read. Pure value ops, ms-based.
fn jet_std_duration_ms(n: i64) -> jet_std::Duration {
    jet_std::Duration { ms: n }
}
fn jet_std_duration_secs(n: i64) -> jet_std::Duration {
    jet_std::Duration {
        ms: n.wrapping_mul(1000),
    }
}
fn jet_std_duration_minutes(n: i64) -> jet_std::Duration {
    jet_std::Duration {
        ms: n.wrapping_mul(60_000),
    }
}
fn jet_std_duration_hours(n: i64) -> jet_std::Duration {
    jet_std::Duration {
        ms: n.wrapping_mul(3_600_000),
    }
}
fn jet_duration_millis(d: &jet_std::Duration) -> i64 {
    d.ms
}
fn jet_duration_seconds(d: &jet_std::Duration) -> i64 {
    d.ms.div_euclid(1000)
}

fn jet_time_instant_now() -> JetInstant {
    JetInstant::now()
}
fn jet_instant_elapsed_millis(i: &JetInstant) -> i64 {
    i.elapsed_millis()
}
fn jet_time_now_utc() -> JetDateTime {
    JetDateTime::now()
}
fn jet_time_today() -> JetDate {
    JetDate::today_utc()
}
fn jet_time_parse_rfc3339(s: &String) -> Result<JetDateTime, String> {
    JetDateTime::parse_rfc3339(s)
}
fn jet_time_period(years: i64, months: i64, days: i64) -> JetPeriod {
    JetPeriod::new(years, months, days)
}
fn jet_time_period_days(days: i64) -> JetPeriod {
    JetPeriod::days(days)
}
fn jet_time_period_months(months: i64) -> JetPeriod {
    JetPeriod::months(months)
}
fn jet_time_period_years(years: i64) -> JetPeriod {
    JetPeriod::years(years)
}
fn jet_time_zone_named(name: &String) -> Result<JetZone, String> {
    JetZone::named(name)
}
fn jet_time_zone_utc() -> JetZone {
    JetZone::utc()
}
fn jet_time_zoned(dt: &JetDateTime, zone: &JetZone) -> JetZonedDateTime {
    dt.in_zone(zone)
}
fn jet_time_zoned_local(date: &JetDate, time: &JetLocalTime, zone: &JetZone) -> JetZonedDateTime {
    JetZonedDateTime::from_local(date, time, zone)
}
fn jet_datetime_plus_duration(dt: &JetDateTime, d: &crate::jet_std::Duration) -> JetDateTime {
    dt.plus_duration_ms(d.ms)
}
fn jet_zoned_add_duration(z: &JetZonedDateTime, d: &crate::jet_std::Duration) -> JetZonedDateTime {
    z.add_duration_ms(d.ms)
}

fn jet_url_parse(s: &String) -> Result<crate::jet_std::JetUrl, String> {
    crate::jet_std::JetUrl::parse(s)
}
fn jet_url_from_parts(
    scheme: &String,
    host: &String,
    path: &String,
    query: &Vec<Vec<String>>,
    fragment: &String,
) -> Result<crate::jet_std::JetUrl, String> {
    crate::jet_std::JetUrl::from_parts(scheme, host, path, query, fragment)
}
fn jet_url_file(path: &String) -> crate::jet_std::JetUrl {
    crate::jet_std::JetUrl::file(path)
}
fn jet_url_data(mime: &crate::jet_std::JetMime, text: &String) -> crate::jet_std::JetUrl {
    crate::jet_std::JetUrl::data(mime, text)
}
fn jet_url_query(pairs: &Vec<Vec<String>>) -> String {
    let rows: Vec<(String, String)> = pairs
        .iter()
        .filter(|r| !r.is_empty())
        .map(|r| {
            (
                r.get(0).cloned().unwrap_or_default(),
                r.get(1).cloned().unwrap_or_default(),
            )
        })
        .collect();
    rows.iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                crate::jet_std::jet_url_percent_encode(k, false),
                crate::jet_std::jet_url_percent_encode(v, false)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}
fn jet_url_percent_encode_component(s: &String) -> String {
    crate::jet_std::jet_url_percent_encode(s, false)
}
fn jet_url_percent_decode_component(s: &String) -> Result<String, String> {
    crate::jet_std::jet_url_percent_decode_str(s)
}
fn jet_mime_parse(s: &String) -> Result<crate::jet_std::JetMime, String> {
    crate::jet_std::JetMime::parse(s)
}
fn jet_mime_from_extension(ext: &String) -> Option<String> {
    crate::jet_std::jet_mime_from_extension(ext).map(|s| s.to_string())
}
fn jet_mime_extension(mime: &String) -> Option<String> {
    crate::jet_std::jet_extension_from_mime(mime).map(|s| s.to_string())
}

// D-BIGINT1 / D-DECIMAL1: precise numeric constructors and methods.
fn jet_bigint_from_int(n: i64) -> jet_std::JetBigInt {
    jet_std::JetBigInt::from_int(n)
}
fn jet_bigint_from_str(s: &String) -> jet_std::JetBigInt {
    jet_std::JetBigInt::from_str(s).expect("invalid BigInt string")
}
fn jet_bigint_add(a: &jet_std::JetBigInt, b: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.add(b)
}
fn jet_bigint_sub(a: &jet_std::JetBigInt, b: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.sub(b)
}
fn jet_bigint_mul(a: &jet_std::JetBigInt, b: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.mul(b)
}
fn jet_bigint_neg(a: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.neg()
}
fn jet_bigint_to_string(a: &jet_std::JetBigInt) -> String {
    a.to_string_rep()
}
fn jet_decimal_from_str(s: &String) -> jet_std::JetDecimal {
    jet_std::JetDecimal::from_str(s).expect("invalid Decimal string")
}
fn jet_decimal_add(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.add(b)
}
fn jet_decimal_sub(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.sub(b)
}
fn jet_decimal_mul(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.mul(b)
}
fn jet_decimal_to_string(a: &jet_std::JetDecimal) -> String {
    a.to_string_rep()
}

// D-ENC-DYN1=A+: the dynamic `parse` returns the one rich `Data` value (the
// user-facing face of `DataTree`). JSON text parses through the internal `Json`
// enum, then collapses onto `DataTree` (integral numbers become `Int`, fractional
// `Float`). Object keys arrive in sorted order (the internal `Json` enum is
// `BTreeMap`-keyed), matching the pre-`Data` dynamic JSON behavior.
fn jet_std_json_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JsonError> {
    jet_std::parse_json(text).map(|j| jet_std::datatree_from_json(&j))
}
fn jet_std_json_render(d: &jet_std::DataTree) -> String {
    jet_std::render_datatree_json(d, false, 0)
}
fn jet_std_json_render_pretty(d: &jet_std::DataTree) -> String {
    jet_std::render_datatree_json(d, true, 0)
}
fn jet_quote_json_local(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn jet_std_json_render_canonical(d: &jet_std::DataTree) -> String {
    fn render(t: &jet_std::DataTree) -> String {
        match t {
            jet_std::DataTree::Null => "null".to_string(),
            jet_std::DataTree::Bool(b) => b.to_string(),
            jet_std::DataTree::Int(n) => n.to_string(),
            jet_std::DataTree::Float(f) => format!("{:?}", f),
            jet_std::DataTree::Text(s) => jet_quote_json_local(s),
            jet_std::DataTree::Bytes(bs) => {
                format!("[{}]", bs.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","))
            }
            jet_std::DataTree::Array(xs) => {
                format!("[{}]", xs.iter().map(render).collect::<Vec<_>>().join(","))
            }
            jet_std::DataTree::Object(entries) => {
                let mut sorted = entries.clone();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                let parts: Vec<String> = sorted
                    .iter()
                    .map(|(k, v)| format!("{}:{}", jet_quote_json_local(k), render(v)))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
        }
    }
    render(d)
}
fn jet_std_json_events(d: &jet_std::DataTree) -> String {
    fn walk(path: String, t: &jet_std::DataTree, out: &mut Vec<String>) {
        let here = if path.is_empty() { "$".to_string() } else { path };
        match t {
            jet_std::DataTree::Object(entries) => {
                out.push(format!("object_start {here}"));
                for (k, v) in entries {
                    walk(format!("{}.{}", here, k), v, out);
                }
                out.push(format!("object_end {here}"));
            }
            jet_std::DataTree::Array(items) => {
                out.push(format!("array_start {here}"));
                for (i, v) in items.iter().enumerate() {
                    walk(format!("{}[{}]", here, i), v, out);
                }
                out.push(format!("array_end {here}"));
            }
            _ => out.push(format!("value {here} {}", jet_std_json_render_canonical(t))),
        }
    }
    let mut out = Vec::new();
    walk(String::new(), d, &mut out);
    out.join("\n")
}
fn jet_std_jsonl_parse(text: &String) -> Result<Vec<jet_std::DataTree>, jet_std::JsonError> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match jet_std_json_parse(&trimmed.to_string()) {
            Ok(v) => out.push(v),
            Err(e) => {
                return Err(jet_std::JsonError {
                    line: idx as i64 + e.line,
                    message: e.message,
                })
            }
        }
    }
    Ok(out)
}
fn jet_std_jsonl_render(rows: &Vec<jet_std::DataTree>) -> String {
    let mut out = rows
        .iter()
        .map(jet_std_json_render_canonical)
        .collect::<Vec<_>>()
        .join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

// D-JSON1-decode + D-JSON3: lenient JSON decode with coercion surfacing.
// Parses `text`, then walks the result. Any JSON string that looks like a
// number or boolean is coerced to that type; one log line is emitted per
// coercion naming the field and the from→to types. The coerced value collapses
// onto `Data` (D-ENC-DYN1=A+).
fn jet_std_json_decode_lenient(text: &String) -> Result<jet_std::DataTree, jet_std::JsonError> {
    let parsed = jet_std::parse_json(text)?;
    Ok(jet_std::datatree_from_json(&jet_std_json_coerce_walk(
        &parsed, "",
    )))
}

fn jet_std_json_coerce_walk(value: &jet_std::Json, path: &str) -> jet_std::Json {
    match value {
        jet_std::Json::Text(s) => {
            // try bool first (exact match only)
            if s == "true" {
                jet_std_json_emit_coerce(path, "string", "boolean");
                return jet_std::Json::Boolean(true);
            }
            if s == "false" {
                jet_std_json_emit_coerce(path, "string", "boolean");
                return jet_std::Json::Boolean(false);
            }
            // try number (must parse as valid f64 and round-trip cleanly)
            if let Ok(n) = s.parse::<f64>() {
                if n.is_finite() {
                    jet_std_json_emit_coerce(path, "string", "number");
                    return jet_std::Json::Number(n);
                }
            }
            value.clone()
        }
        jet_std::Json::Object(entries) => {
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in entries {
                let child_path = if path.is_empty() {
                    format!("{}", k)
                } else {
                    format!("{}.{}", path, k)
                };
                out.insert(k.clone(), jet_std_json_coerce_walk(v, &child_path));
            }
            jet_std::Json::Object(out)
        }
        jet_std::Json::Array(items) => {
            let coerced: Vec<jet_std::Json> = items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let child_path = if path.is_empty() {
                        format!("[{}]", i)
                    } else {
                        format!("{}[{}]", path, i)
                    };
                    jet_std_json_coerce_walk(v, &child_path)
                })
                .collect();
            jet_std::Json::Array(coerced)
        }
        // Null, Boolean, Number — already the right type, no coercion.
        other => other.clone(),
    }
}

fn jet_std_json_emit_coerce(path: &str, from: &str, to: &str) {
    let field_label = if path.is_empty() { "<root>" } else { path };
    let msg = format!(
        "json coerce: field \"{}\" {} \u{2192} {}",
        field_label, from, to
    );
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    eprintln!("{{\"level\":\"info\",\"body\":\"{}\",\"ts\":{}}}", msg, ts);
}

fn jet_string_bytes(s: &String) -> Vec<u8> {
    s.as_bytes().to_vec()
}
fn jet_string_from_bytes(bs: &Vec<u8>) -> Result<String, jet_std::Utf8Error> {
    String::from_utf8(bs.clone()).map_err(|e| jet_std::Utf8Error {
        message: e.to_string(),
    })
}
fn jet_int_to_u8(n: i64) -> Result<u8, String> {
    if (0..=255).contains(&n) {
        Ok(n as u8)
    } else {
        Err("a U8 holds 0..255".to_string())
    }
}
fn jet_stopwatch_elapsed_millis(sw: &jet_std::Stopwatch) -> i64 {
    sw.start.elapsed().as_millis() as i64
}

// ── D-SIMD2 / D-LINALG1: math value-type free functions ───────────────────────
// Constructors (`_new`), statics (`splat`/`from_array`), instance methods, lane
// reads, and reductions. Codegen names these `jet_math_<Type>_<fn>` and always
// passes the receiver as `&recv` (value types — every op returns a fresh value).
// Plain std math; no intrinsics, no `un`+`safe`.

fn jet_math_F32x4_new(a: f32, b: f32, c: f32, d: f32) -> jet_std::F32x4 {
    jet_std::F32x4([a, b, c, d])
}
fn jet_math_F64x2_new(a: f64, b: f64) -> jet_std::F64x2 {
    jet_std::F64x2([a, b])
}
fn jet_math_F32x4_splat(x: f32) -> jet_std::F32x4 {
    jet_std::F32x4([x; 4])
}
fn jet_math_F64x2_splat(x: f64) -> jet_std::F64x2 {
    jet_std::F64x2([x; 2])
}
fn jet_math_F32x4_from_array(a: [f32; 4]) -> jet_std::F32x4 {
    jet_std::F32x4(a)
}
fn jet_math_F64x2_from_array(a: [f64; 2]) -> jet_std::F64x2 {
    jet_std::F64x2(a)
}
fn jet_math_F32x4_to_array(v: &jet_std::F32x4) -> [f32; 4] {
    v.0
}
fn jet_math_F64x2_to_array(v: &jet_std::F64x2) -> [f64; 2] {
    v.0
}

fn jet_math_F32x4_lane(v: &jet_std::F32x4, i: i64, file: &str, line: u32) -> f32 {
    if i < 0 || i as usize >= 4 {
        jet_panic(
            file,
            line,
            &format!("lane index {} out of range for F32x4 (4 lanes)", i),
        );
    }
    v.0[i as usize]
}
fn jet_math_F64x2_lane(v: &jet_std::F64x2, i: i64, file: &str, line: u32) -> f64 {
    if i < 0 || i as usize >= 2 {
        jet_panic(
            file,
            line,
            &format!("lane index {} out of range for F64x2 (2 lanes)", i),
        );
    }
    v.0[i as usize]
}

fn jet_math_F32x4_sum(v: &jet_std::F32x4) -> f32 {
    v.0.iter().sum()
}
fn jet_math_F32x4_product(v: &jet_std::F32x4) -> f32 {
    v.0.iter().product()
}
fn jet_math_F32x4_min(v: &jet_std::F32x4) -> f32 {
    v.0.iter().copied().fold(f32::INFINITY, f32::min)
}
fn jet_math_F32x4_max(v: &jet_std::F32x4) -> f32 {
    v.0.iter().copied().fold(f32::NEG_INFINITY, f32::max)
}
fn jet_math_F32x4_reduce_add(v: &jet_std::F32x4) -> f32 {
    jet_math_F32x4_sum(v)
}
fn jet_math_F32x4_reduce_mul(v: &jet_std::F32x4) -> f32 {
    jet_math_F32x4_product(v)
}
fn jet_math_F32x4_reduce_min(v: &jet_std::F32x4) -> f32 {
    jet_math_F32x4_min(v)
}
fn jet_math_F32x4_reduce_max(v: &jet_std::F32x4) -> f32 {
    jet_math_F32x4_max(v)
}

fn jet_math_F64x2_sum(v: &jet_std::F64x2) -> f64 {
    v.0.iter().sum()
}
fn jet_math_F64x2_product(v: &jet_std::F64x2) -> f64 {
    v.0.iter().product()
}
fn jet_math_F64x2_min(v: &jet_std::F64x2) -> f64 {
    v.0.iter().copied().fold(f64::INFINITY, f64::min)
}
fn jet_math_F64x2_max(v: &jet_std::F64x2) -> f64 {
    v.0.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}
fn jet_math_F64x2_reduce_add(v: &jet_std::F64x2) -> f64 {
    jet_math_F64x2_sum(v)
}
fn jet_math_F64x2_reduce_mul(v: &jet_std::F64x2) -> f64 {
    jet_math_F64x2_product(v)
}
fn jet_math_F64x2_reduce_min(v: &jet_std::F64x2) -> f64 {
    jet_math_F64x2_min(v)
}
fn jet_math_F64x2_reduce_max(v: &jet_std::F64x2) -> f64 {
    jet_math_F64x2_max(v)
}

// Vectors.
fn jet_math_Vec2_new(x: f64, y: f64) -> jet_std::Vec2 {
    jet_std::Vec2([x, y])
}
fn jet_math_Vec3_new(x: f64, y: f64, z: f64) -> jet_std::Vec3 {
    jet_std::Vec3([x, y, z])
}
fn jet_math_Vec4_new(x: f64, y: f64, z: f64, w: f64) -> jet_std::Vec4 {
    jet_std::Vec4([x, y, z, w])
}
fn jet_math_Vec2_splat(x: f64) -> jet_std::Vec2 {
    jet_std::Vec2([x; 2])
}
fn jet_math_Vec3_splat(x: f64) -> jet_std::Vec3 {
    jet_std::Vec3([x; 3])
}
fn jet_math_Vec4_splat(x: f64) -> jet_std::Vec4 {
    jet_std::Vec4([x; 4])
}
fn jet_math_Vec2_from_array(a: [f64; 2]) -> jet_std::Vec2 {
    jet_std::Vec2(a)
}
fn jet_math_Vec3_from_array(a: [f64; 3]) -> jet_std::Vec3 {
    jet_std::Vec3(a)
}
fn jet_math_Vec4_from_array(a: [f64; 4]) -> jet_std::Vec4 {
    jet_std::Vec4(a)
}
fn jet_math_Vec2_to_array(v: &jet_std::Vec2) -> [f64; 2] {
    v.0
}
fn jet_math_Vec3_to_array(v: &jet_std::Vec3) -> [f64; 3] {
    v.0
}
fn jet_math_Vec4_to_array(v: &jet_std::Vec4) -> [f64; 4] {
    v.0
}

fn jet_math_Vec2_dot(v: &jet_std::Vec2, o: jet_std::Vec2) -> f64 {
    v.0[0] * o.0[0] + v.0[1] * o.0[1]
}
fn jet_math_Vec3_dot(v: &jet_std::Vec3, o: jet_std::Vec3) -> f64 {
    v.0[0] * o.0[0] + v.0[1] * o.0[1] + v.0[2] * o.0[2]
}
fn jet_math_Vec4_dot(v: &jet_std::Vec4, o: jet_std::Vec4) -> f64 {
    (0..4).map(|i| v.0[i] * o.0[i]).sum()
}
fn jet_math_Vec3_cross(v: &jet_std::Vec3, o: jet_std::Vec3) -> jet_std::Vec3 {
    jet_std::Vec3([
        v.0[1] * o.0[2] - v.0[2] * o.0[1],
        v.0[2] * o.0[0] - v.0[0] * o.0[2],
        v.0[0] * o.0[1] - v.0[1] * o.0[0],
    ])
}
fn jet_math_Vec2_length(v: &jet_std::Vec2) -> f64 {
    jet_math_Vec2_dot(v, *v).sqrt()
}
fn jet_math_Vec3_length(v: &jet_std::Vec3) -> f64 {
    jet_math_Vec3_dot(v, *v).sqrt()
}
fn jet_math_Vec4_length(v: &jet_std::Vec4) -> f64 {
    jet_math_Vec4_dot(v, *v).sqrt()
}
fn jet_math_Vec2_normalize(v: &jet_std::Vec2) -> jet_std::Vec2 {
    let l = jet_math_Vec2_length(v);
    if l == 0.0 {
        *v
    } else {
        jet_std::Vec2([v.0[0] / l, v.0[1] / l])
    }
}
fn jet_math_Vec3_normalize(v: &jet_std::Vec3) -> jet_std::Vec3 {
    let l = jet_math_Vec3_length(v);
    if l == 0.0 {
        *v
    } else {
        jet_std::Vec3([v.0[0] / l, v.0[1] / l, v.0[2] / l])
    }
}
fn jet_math_Vec4_normalize(v: &jet_std::Vec4) -> jet_std::Vec4 {
    let l = jet_math_Vec4_length(v);
    if l == 0.0 {
        *v
    } else {
        let mut r = v.0;
        for i in 0..4 {
            r[i] /= l;
        }
        jet_std::Vec4(r)
    }
}

// Matrices (column-major). Constructors take N*N components in column-major order.
fn jet_math_Mat3_new(
    m0: f64,
    m1: f64,
    m2: f64,
    m3: f64,
    m4: f64,
    m5: f64,
    m6: f64,
    m7: f64,
    m8: f64,
) -> jet_std::Mat3 {
    jet_std::Mat3([m0, m1, m2, m3, m4, m5, m6, m7, m8])
}
fn jet_math_Mat4_new(
    m0: f64,
    m1: f64,
    m2: f64,
    m3: f64,
    m4: f64,
    m5: f64,
    m6: f64,
    m7: f64,
    m8: f64,
    m9: f64,
    m10: f64,
    m11: f64,
    m12: f64,
    m13: f64,
    m14: f64,
    m15: f64,
) -> jet_std::Mat4 {
    jet_std::Mat4([
        m0, m1, m2, m3, m4, m5, m6, m7, m8, m9, m10, m11, m12, m13, m14, m15,
    ])
}
fn jet_math_Mat3_from_array(a: [f64; 9]) -> jet_std::Mat3 {
    jet_std::Mat3(a)
}
fn jet_math_Mat4_from_array(a: [f64; 16]) -> jet_std::Mat4 {
    jet_std::Mat4(a)
}
fn jet_math_Mat3_to_array(m: &jet_std::Mat3) -> [f64; 9] {
    m.0
}
fn jet_math_Mat4_to_array(m: &jet_std::Mat4) -> [f64; 16] {
    m.0
}
fn jet_math_Mat3_matmul(m: &jet_std::Mat3, o: jet_std::Mat3) -> jet_std::Mat3 {
    *m * o
}
fn jet_math_Mat4_matmul(m: &jet_std::Mat4, o: jet_std::Mat4) -> jet_std::Mat4 {
    *m * o
}
fn jet_math_Mat3_transform(m: &jet_std::Mat3, v: jet_std::Vec3) -> jet_std::Vec3 {
    *m * v
}
fn jet_math_Mat4_transform(m: &jet_std::Mat4, v: jet_std::Vec4) -> jet_std::Vec4 {
    *m * v
}
fn jet_math_Mat3_transpose(m: &jet_std::Mat3) -> jet_std::Mat3 {
    let mut r = [0.0f64; 9];
    for c in 0..3 {
        for row in 0..3 {
            r[c * 3 + row] = m.0[row * 3 + c];
        }
    }
    jet_std::Mat3(r)
}
fn jet_math_Mat4_transpose(m: &jet_std::Mat4) -> jet_std::Mat4 {
    let mut r = [0.0f64; 16];
    for c in 0..4 {
        for row in 0..4 {
            r[c * 4 + row] = m.0[row * 4 + c];
        }
    }
    jet_std::Mat4(r)
}

// ── core.encoding: Encode / Decode traits + blanket impls (D-SERDE1/2/4) ──────
// The built-in `@[Codable]`/`@[Encode]`/`@[Decode]` derive (D-ENC1) lowers to
// these traits. `jet_encode`/`jet_decode` are codegen-internal method names the
// user never types (they write the verbs `encode`/`decode` only in a hand-impl,
// D-SERDE2 — a later increment). Pure safe std Rust, no proc-macros (I1/I6).
#[allow(non_camel_case_types)]
pub trait user_Encode {
    fn jet_encode(&self) -> jet_std::DataTree;
}
#[allow(non_camel_case_types)]
pub trait user_Decode: Sized {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError>;
    /// D-MIGRATE4: decode this value, reporting whether it arrived as an older
    /// `@PublishedSchema` shape and was walked forward through the migration
    /// chain. The default is the zero-cost identity: no migrations declared, so
    /// decode the current shape and report `fresh`. Codegen overrides this only
    /// for a `@PublishedSchema` type that has `migration { }` blocks and a
    /// runtime decode path — every other type keeps this default, so no
    /// per-type code is emitted and the decode path is byte-for-byte unchanged.
    fn jet_decode_traced(
        tree: &jet_std::DataTree,
    ) -> Result<(Self, jet_std::MigrationStatus), jet_std::DecodeError> {
        Ok((Self::jet_decode(tree)?, jet_std::MigrationStatus::fresh()))
    }
}

impl user_Encode for i64 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Int(*self)
    }
}
impl user_Encode for f64 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Float(*self)
    }
}
impl user_Encode for bool {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Bool(*self)
    }
}
impl user_Encode for String {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Text(self.clone())
    }
}
impl user_Encode for char {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Text(self.to_string())
    }
}
impl<T: user_Encode> user_Encode for Vec<T> {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Array(self.iter().map(|x| x.jet_encode()).collect())
    }
}
impl<T: user_Encode> user_Encode for Option<T> {
    fn jet_encode(&self) -> jet_std::DataTree {
        match self {
            Some(x) => x.jet_encode(),
            None => jet_std::DataTree::Null,
        }
    }
}
impl<V: user_Encode> user_Encode for std::collections::BTreeMap<String, V> {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Object(
            self.iter()
                .map(|(k, v)| (k.clone(), v.jet_encode()))
                .collect(),
        )
    }
}

impl user_Decode for i64 {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Int(n) => Ok(*n),
            jet_std::DataTree::Float(f) if f.fract() == 0.0 => Ok(*f as i64),
            jet_std::DataTree::Text(s) => s.trim().parse::<i64>().map_err(|_| {
                jet_std::DecodeError::new(format!("expected Int, found text {:?}", s))
            }),
            other => Err(jet_std::DecodeError::new(format!(
                "expected Int, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for f64 {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Float(f) => Ok(*f),
            jet_std::DataTree::Int(n) => Ok(*n as f64),
            jet_std::DataTree::Text(s) => s.trim().parse::<f64>().map_err(|_| {
                jet_std::DecodeError::new(format!("expected Float, found text {:?}", s))
            }),
            other => Err(jet_std::DecodeError::new(format!(
                "expected Float, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for bool {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Bool(b) => Ok(*b),
            jet_std::DataTree::Text(s) => match s.trim() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(jet_std::DecodeError::new(format!(
                    "expected Bool, found text {:?}",
                    s
                ))),
            },
            other => Err(jet_std::DecodeError::new(format!(
                "expected Bool, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for String {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Text(s) => Ok(s.clone()),
            jet_std::DataTree::Int(n) => Ok(n.to_string()),
            jet_std::DataTree::Float(f) => Ok(format!("{:?}", f)),
            jet_std::DataTree::Bool(b) => Ok(b.to_string()),
            other => Err(jet_std::DecodeError::new(format!(
                "expected Text, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for char {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        let s = String::jet_decode(t)?;
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (Some(c), None) => Ok(c),
            _ => Err(jet_std::DecodeError::new(format!(
                "expected a single Char, found {:?}",
                s
            ))),
        }
    }
}
impl<T: user_Decode> user_Decode for Vec<T> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    out.push(
                        T::jet_decode(item)
                            .map_err(|e| jet_std::DecodeError::under(&format!("[{}]", i), e))?,
                    );
                }
                Ok(out)
            }
            other => Err(jet_std::DecodeError::new(format!(
                "expected a list, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl<T: user_Decode> user_Decode for Option<T> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Null => Ok(None),
            other => Ok(Some(T::jet_decode(other)?)),
        }
    }
}
impl<V: user_Decode> user_Decode for std::collections::BTreeMap<String, V> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Object(entries) => {
                let mut out = std::collections::BTreeMap::new();
                for (k, v) in entries {
                    out.insert(
                        k.clone(),
                        V::jet_decode(v).map_err(|e| jet_std::DecodeError::under(k, e))?,
                    );
                }
                Ok(out)
            }
            other => Err(jet_std::DecodeError::new(format!(
                "expected an object, found {}",
                jet_std::datatree_kind(other)
            ))),
        }
    }
}

// ── core.encoding: typed format verbs over Encode/Decode (D-ENC1, D-SERDE6) ────
// `to_string`/`to_string_pretty` (D-JSONVERB1) and the typed `decode<T>` route
// every format through the one DataTree model.
fn jet_enc_json_to_string<T: user_Encode>(v: &T) -> String {
    jet_std::render_datatree_json(&v.jet_encode(), false, 0)
}
fn jet_enc_json_to_string_pretty<T: user_Encode>(v: &T) -> String {
    jet_std::render_datatree_json(&v.jet_encode(), true, 0)
}
fn jet_enc_json_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let j = jet_std::parse_json(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid JSON (line {}): {}", e.line, e.message))
    })?;
    // D-MIGRATE4: plain decode walks the same migration chain, silently — the
    // status is dropped. Types without migrations hit the trait default, which
    // is exactly `jet_decode` (zero cost).
    Ok(T::jet_decode_traced(&jet_std::datatree_from_json(&j))?.0)
}

// D-MIGRATE3=A: `decode_traced<T>` — same decode, wrapped in `DecodeResult` so the
// caller can ask whether/how it migrated, without `decode` itself paying for it.
fn jet_enc_json_decode_traced<T: user_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<T>, jet_std::DecodeError> {
    let j = jet_std::parse_json(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid JSON (line {}): {}", e.line, e.message))
    })?;
    let (value, migration) = T::jet_decode_traced(&jet_std::datatree_from_json(&j))?;
    Ok(jet_std::DecodeResult { value, migration })
}

// CSV typed decode: header row maps columns to fields by name; each data row
// becomes a DataTree::Object of Text cells, then decodes to `T`. A short row or a
// per-row decode failure is a typed `DecodeError` naming the 1-based row.
fn jet_enc_csv_decode<T: user_Decode>(text: &String) -> Result<Vec<T>, jet_std::DecodeError> {
    let rows = jet_ring_csv_parse(text).map_err(jet_std::DecodeError::new)?;
    let mut it = rows.into_iter();
    let Some(header) = it.next() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (i, row) in it.enumerate() {
        let obj: Vec<(String, jet_std::DataTree)> = header
            .iter()
            .enumerate()
            .map(|(c, name)| {
                let cell = row.get(c).cloned().unwrap_or_default();
                (name.clone(), jet_std::DataTree::Text(cell))
            })
            .collect();
        let tree = jet_std::DataTree::Object(obj);
        // D-MIGRATE4: plain decode walks the migration chain silently (see json's).
        out.push(
            T::jet_decode_traced(&tree)
                .map(|(v, _)| v)
                .map_err(|e| jet_std::DecodeError::under(&format!("row {}", i + 1), e))?,
        );
    }
    Ok(out)
}

fn jet_data_count<T>(rows: &Vec<T>) -> i64 {
    rows.len() as i64
}

fn jet_data_filter<T, F>(rows: &Vec<T>, pred: F) -> Vec<T>
where
    T: Clone,
    F: Fn(T) -> bool,
{
    rows.iter().cloned().filter(|row| pred(row.clone())).collect()
}

fn jet_data_sort_by<T, F>(rows: &Vec<T>, key: F) -> Vec<T>
where
    T: Clone,
    F: Fn(T) -> String,
{
    let mut out = rows.clone();
    out.sort_by_key(|row| key(row.clone()));
    out
}

fn jet_data_sum(values: &Vec<f64>) -> f64 {
    values.iter().copied().sum()
}

fn jet_data_mean(values: &Vec<f64>) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        jet_data_sum(values) / values.len() as f64
    }
}

fn jet_data_min(values: &Vec<f64>) -> f64 {
    values.iter().copied().reduce(f64::min).unwrap_or(0.0)
}

fn jet_data_max(values: &Vec<f64>) -> f64 {
    values.iter().copied().reduce(f64::max).unwrap_or(0.0)
}

fn jet_data_median(values: &Vec<f64>) -> f64 {
    jet_data_quantile(values, 0.5)
}

fn jet_data_quantile(values: &Vec<f64>, q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = q.clamp(0.0, 1.0);
    let pos = q * (sorted.len().saturating_sub(1)) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let t = pos - lo as f64;
        sorted[lo] * (1.0 - t) + sorted[hi] * t
    }
}

fn jet_data_variance(values: &Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = jet_data_mean(values);
    values
        .iter()
        .map(|v| {
            let d = *v - mean;
            d * d
        })
        .sum::<f64>()
        / values.len() as f64
}

fn jet_data_stddev(values: &Vec<f64>) -> f64 {
    jet_data_variance(values).sqrt()
}

fn jet_data_describe(values: &Vec<f64>) -> jet_std::DataSummary {
    jet_std::DataSummary {
        count: values.len() as i64,
        sum: jet_data_sum(values),
        mean: jet_data_mean(values),
        min: jet_data_min(values),
        max: jet_data_max(values),
        median: jet_data_median(values),
        variance: jet_data_variance(values),
        stddev: jet_data_stddev(values),
    }
}

fn jet_data_group_count<T, F>(rows: &Vec<T>, key: F) -> Vec<jet_std::DataGroup>
where
    T: Clone,
    F: Fn(T) -> String,
{
    let mut groups: std::collections::BTreeMap<String, (i64, f64)> =
        std::collections::BTreeMap::new();
    for row in rows.iter().cloned() {
        let k = key(row);
        let entry = groups.entry(k).or_insert((0, 0.0));
        entry.0 += 1;
    }
    groups
        .into_iter()
        .map(|(key, (count, sum))| jet_std::DataGroup {
            key,
            count,
            sum,
            mean: 0.0,
        })
        .collect()
}

fn jet_data_group_sum<T, FK, FV>(rows: &Vec<T>, key: FK, value: FV) -> Vec<jet_std::DataGroup>
where
    T: Clone,
    FK: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    let mut groups: std::collections::BTreeMap<String, (i64, f64)> =
        std::collections::BTreeMap::new();
    for row in rows.iter().cloned() {
        let k = key(row.clone());
        let v = value(row);
        let entry = groups.entry(k).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += v;
    }
    groups
        .into_iter()
        .map(|(key, (count, sum))| jet_std::DataGroup {
            key,
            count,
            sum,
            mean: if count == 0 { 0.0 } else { sum / count as f64 },
        })
        .collect()
}

fn jet_data_group_mean<T, FK, FV>(rows: &Vec<T>, key: FK, value: FV) -> Vec<jet_std::DataGroup>
where
    T: Clone,
    FK: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    jet_data_group_sum(rows, key, value)
}

fn jet_data_status() -> Vec<jet_std::DataStatus> {
    vec![
        jet_std::DataStatus {
            step: "core.data.csv".to_string(),
            path: "native".to_string(),
            replacement: "native".to_string(),
        },
        jet_std::DataStatus {
            step: "core.data.stats".to_string(),
            path: "native".to_string(),
            replacement: "native".to_string(),
        },
        jet_std::DataStatus {
            step: "py.* / r.* / gpu.*".to_string(),
            path: "bridge-ready".to_string(),
            replacement: "report via data.status() and jet dossier data".to_string(),
        },
    ]
}

fn jet_data_bar_text(groups: &Vec<jet_std::DataGroup>) -> String {
    let mut lines = Vec::new();
    for g in groups {
        let n = if g.count < 0 { 0 } else { g.count.min(40) } as usize;
        lines.push(format!("{} | {} {}", g.key, "#".repeat(n), g.count));
    }
    lines.join("\n")
}

fn jet_data_bar_svg(groups: &Vec<jet_std::DataGroup>) -> String {
    let width = 320.0f64;
    let row_h = 24.0f64;
    let height = 24.0 + row_h * groups.len() as f64;
    let max = groups.iter().map(|g| g.count).max().unwrap_or(1).max(1) as f64;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"{}\" viewBox=\"0 0 320 {}\">",
        height as i64,
        height as i64
    );
    out.push_str("<rect width=\"320\" height=\"100%\" fill=\"white\"/>");
    for (i, g) in groups.iter().enumerate() {
        let y = 18.0 + i as f64 * row_h;
        let bar_w = ((g.count as f64 / max) * (width - 120.0)).round();
        out.push_str(&format!(
            "<text x=\"8\" y=\"{}\" font-family=\"monospace\" font-size=\"12\">{}</text>",
            y as i64,
            jet_data_svg_escape(&g.key)
        ));
        out.push_str(&format!(
            "<rect x=\"96\" y=\"{}\" width=\"{}\" height=\"14\" fill=\"#2f6f73\"/>",
            (y - 12.0) as i64,
            bar_w as i64
        ));
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"12\">{}</text>",
            (104.0 + bar_w) as i64,
            y as i64,
            g.count
        ));
    }
    out.push_str("</svg>");
    out
}

fn jet_fmt_number(value: i64) -> String {
    comma_int(value)
}

fn jet_fmt_decimal(value: f64, precision: i64) -> String {
    let precision = precision.clamp(0, 9) as usize;
    comma_decimal(format!("{:.*}", precision, value))
}

fn jet_fmt_percent(value: f64, precision: i64) -> String {
    format!("{}%", jet_fmt_decimal(value * 100.0, precision))
}

fn jet_fmt_bytes(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let mut size = (value as f64).abs();
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut unit = 0usize;
    while size >= 1000.0 && unit + 1 < units.len() {
        size /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}{} {}", sign, size as i64, units[unit])
    } else if size >= 10.0 {
        format!("{}{} {}", sign, size.round() as i64, units[unit])
    } else {
        let shown = format!("{:.1}", size);
        format!("{}{} {}", sign, shown.trim_end_matches(".0"), units[unit])
    }
}

fn jet_fmt_duration(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "" };
    let mut rest = ms.abs();
    if rest < 1000 {
        return format!("{}{}ms", sign, rest);
    }
    let days = rest / 86_400_000;
    rest %= 86_400_000;
    let hours = rest / 3_600_000;
    rest %= 3_600_000;
    let minutes = rest / 60_000;
    rest %= 60_000;
    let seconds = rest / 1000;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }
    format!("{}{}", sign, parts.into_iter().take(3).collect::<Vec<_>>().join(" "))
}

fn jet_fmt_ordinal(value: i64) -> String {
    let n = value.abs();
    let suffix = if (11..=13).contains(&(n % 100)) {
        "th"
    } else {
        match n % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{}{}", comma_int(value), suffix)
}

fn jet_fmt_plural(count: i64, singular: &String, plural: &String) -> String {
    let word = if count.abs() == 1 { singular } else { plural };
    format!("{} {}", comma_int(count), word)
}

fn jet_fmt_pad_left(text: &String, width: i64, fill: &String) -> String {
    let need = pad_need(text, width);
    format!("{}{}", pad_fill(fill, need), text)
}

fn jet_fmt_pad_right(text: &String, width: i64, fill: &String) -> String {
    let need = pad_need(text, width);
    format!("{}{}", text, pad_fill(fill, need))
}

fn jet_fmt_pad_center(text: &String, width: i64, fill: &String) -> String {
    let need = pad_need(text, width);
    let left = need / 2;
    let right = need - left;
    format!("{}{}{}", pad_fill(fill, left), text, pad_fill(fill, right))
}

fn pad_need(text: &str, width: i64) -> usize {
    let width = width.max(0) as usize;
    width.saturating_sub(text.chars().count())
}

fn pad_fill(fill: &str, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    let fill = if fill.is_empty() { " " } else { fill };
    let mut out = String::new();
    while out.chars().count() < len {
        out.push_str(fill);
    }
    out.chars().take(len).collect()
}

fn comma_int(value: i64) -> String {
    let raw = value.abs().to_string();
    let mut out = String::new();
    for (i, ch) in raw.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let mut text: String = out.chars().rev().collect();
    if value < 0 {
        text.insert(0, '-');
    }
    text
}

fn comma_decimal(raw: String) -> String {
    let (sign, rest) = raw.strip_prefix('-').map_or(("", raw.as_str()), |s| ("-", s));
    let mut split = rest.splitn(2, '.');
    let whole = split.next().unwrap_or("0");
    let frac = split.next();
    let whole_value = whole.parse::<i64>().unwrap_or(0);
    let whole_text = comma_int(whole_value);
    match frac {
        Some(frac) => format!("{}{}.{}", sign, whole_text, frac),
        None => format!("{}{}", sign, whole_text),
    }
}

fn jet_data_svg_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// D-MIGRATE3=A: traced sibling of `jet_enc_csv_decode` — see json's for the shape.
fn jet_enc_csv_decode_traced<T: user_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<Vec<T>>, jet_std::DecodeError> {
    let rows = jet_ring_csv_parse(text).map_err(jet_std::DecodeError::new)?;
    let mut it = rows.into_iter();
    let Some(header) = it.next() else {
        return Ok(jet_std::DecodeResult {
            value: Vec::new(),
            migration: jet_std::MigrationStatus::fresh(),
        });
    };
    let mut value = Vec::new();
    // Each row decodes independently; the record-level status is the first row
    // that actually migrated (a CSV file is one shape per column layout, so a
    // migrated file migrates uniformly — the first hit describes the batch).
    let mut migration = jet_std::MigrationStatus::fresh();
    for (i, row) in it.enumerate() {
        let obj: Vec<(String, jet_std::DataTree)> = header
            .iter()
            .enumerate()
            .map(|(c, name)| {
                let cell = row.get(c).cloned().unwrap_or_default();
                (name.clone(), jet_std::DataTree::Text(cell))
            })
            .collect();
        let tree = jet_std::DataTree::Object(obj);
        let (v, m) = T::jet_decode_traced(&tree)
            .map_err(|e| jet_std::DecodeError::under(&format!("row {}", i + 1), e))?;
        if m.migrated && !migration.migrated {
            migration = m;
        }
        value.push(v);
    }
    Ok(jet_std::DecodeResult { value, migration })
}

// CSV typed encode: `[T]` → header row (field names from the first row's Object)
// + one record per element. Requires every element to encode to a flat Object.
fn jet_enc_csv_to_string<T: user_Encode>(values: &Vec<T>) -> String {
    let trees: Vec<jet_std::DataTree> = values.iter().map(|v| v.jet_encode()).collect();
    let mut header: Vec<String> = Vec::new();
    if let Some(jet_std::DataTree::Object(entries)) = trees.first() {
        header = entries.iter().map(|(k, _)| k.clone()).collect();
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    rows.push(header.clone());
    for tree in &trees {
        let mut record = Vec::with_capacity(header.len());
        for key in &header {
            let cell = match jet_std::datatree_get(tree, key) {
                Some(jet_std::DataTree::Text(s)) => s.clone(),
                Some(jet_std::DataTree::Int(n)) => n.to_string(),
                Some(jet_std::DataTree::Float(f)) => format!("{:?}", f),
                Some(jet_std::DataTree::Bool(b)) => b.to_string(),
                Some(jet_std::DataTree::Null) | None => String::new(),
                Some(other) => jet_std::render_datatree_json(other, false, 0),
            };
            record.push(cell);
        }
        rows.push(record);
    }
    jet_ring_csv_render(&rows)
}

// D-ENC-DYN1=A+ (c152): TOML is a full serde-equivalent adapter over the one rich
// `DataTree` — nested `[table]`s, arrays-of-tables, dotted keys, and typed scalars.
// The dynamic `parse` returns the `Data` value; `decode<T>` walks the rich tree;
// `to_string` renders a `DataTree` back to a nested document.
fn jet_std_toml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JsonError> {
    jet_std::toml::parse_to_tree(text).map_err(|e| jet_std::JsonError {
        line: e.line as i64,
        message: e.message,
    })
}
fn jet_std_toml_render(d: &jet_std::DataTree) -> String {
    jet_std::toml::render(d)
}

fn jet_enc_toml_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let tree = jet_std::toml::parse_to_tree(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid TOML (line {}): {}", e.line, e.message))
    })?;
    // D-MIGRATE4: plain decode walks the migration chain silently (see json's).
    Ok(T::jet_decode_traced(&tree)?.0)
}

// D-MIGRATE3=A: traced sibling of `jet_enc_toml_decode` — see json's for the shape.
fn jet_enc_toml_decode_traced<T: user_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<T>, jet_std::DecodeError> {
    let tree = jet_std::toml::parse_to_tree(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid TOML (line {}): {}", e.line, e.message))
    })?;
    let (value, migration) = T::jet_decode_traced(&tree)?;
    Ok(jet_std::DecodeResult { value, migration })
}

// YAML typed decode: parse flat scalars into a DataTree::Object of Text, then decode.
// D-ENC-DYN1=A+ / D-ENC-YAML1 (c152): YAML is a full serde adapter over the one
// rich `DataTree` — block + flow maps/sequences, typed core scalars, block scalars,
// comments, documents, anchors/aliases. parse → `Data`; decode<T> → typed tree.
fn jet_std_yaml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JsonError> {
    jet_std::yaml::parse_to_tree(text).map_err(|e| jet_std::JsonError {
        line: e.line as i64,
        message: e.message,
    })
}
fn jet_std_yaml_render(d: &jet_std::DataTree) -> String {
    jet_std::yaml::render(d)
}

fn jet_enc_yaml_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let tree = jet_std::yaml::parse_to_tree(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid YAML (line {}): {}", e.line, e.message))
    })?;
    // D-MIGRATE4: plain decode walks the migration chain silently (see json's).
    Ok(T::jet_decode_traced(&tree)?.0)
}

// D-MIGRATE3=A: traced sibling of `jet_enc_yaml_decode` — see json's for the shape.
fn jet_enc_yaml_decode_traced<T: user_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<T>, jet_std::DecodeError> {
    let tree = jet_std::yaml::parse_to_tree(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid YAML (line {}): {}", e.line, e.message))
    })?;
    let (value, migration) = T::jet_decode_traced(&tree)?;
    Ok(jet_std::DecodeResult { value, migration })
}
fn jet_enc_toml_to_string<T: user_Encode>(v: &T) -> String {
    jet_std::toml::render(&v.jet_encode())
}
fn jet_enc_yaml_to_string<T: user_Encode>(v: &T) -> String {
    jet_std::yaml::render(&v.jet_encode())
}

// ── E2-M9: First-party ring libraries ────────────────────────────────────────
// Pure-Rust, zero external crates (I6). CSV, TOML, YAML, log, time, crypto.

// ── jet.csv ───────────────────────────────────────────────────────────────────
fn jet_ring_csv_parse(text: &String) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        match csv_parse_row(line) {
            Ok(row) => rows.push(row),
            Err(msg) => {
                return Err(format!("E2701: CSV row {} — {}", line_no + 1, msg));
            }
        }
    }
    Ok(rows)
}

fn csv_parse_row(line: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        let field = if chars.peek() == Some(&'"') {
            chars.next(); // consume opening quote
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('"') => {
                        if chars.peek() == Some(&'"') {
                            chars.next(); // escaped quote
                            s.push('"');
                        } else {
                            break;
                        }
                    }
                    Some(c) => s.push(c),
                    None => break,
                }
            }
            s
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                s.push(c);
                chars.next();
            }
            s
        };
        fields.push(field);
        match chars.next() {
            Some(',') => {}
            None => break,
            Some(c) => return Err(format!("unexpected character {:?} after field", c)),
        }
    }
    Ok(fields)
}

fn jet_ring_csv_render(rows: &Vec<Vec<String>>) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|field| {
                    if field.contains(',') || field.contains('"') || field.contains('\n') {
                        format!("\"{}\"", field.replace('"', "\"\""))
                    } else {
                        field.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── jet.log ───────────────────────────────────────────────────────────────────
// E2-M12 D-OBS3: structured JSON logs (OTel-aligned field names).
// Each log record is a JSON object on stderr:
//   {"level":"info","body":"...","ts":<unix-ms>}
// When a trace_id is set (log.set_trace_id), it appears as "trace_id":"...".
// Log level: 0=debug, 1=info, 2=warn, 3=error. Default is info (1).
// D-LOGFMT1=A: format 0=auto (TTY→text, else JSON), 1=json, 2=text.
thread_local! {
    static JET_LOG_LEVEL: std::cell::Cell<u8> = std::cell::Cell::new(1);
    static JET_LOG_TRACE_ID: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    static JET_LOG_FORMAT: std::cell::Cell<u8> = std::cell::Cell::new(0);
    static JET_LOG_SINK_PATH: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    static JET_LOG_SPANS: std::cell::RefCell<Vec<jet_std::LogSpan>> = std::cell::RefCell::new(Vec::new());
    static JET_LOG_SAMPLE_EVERY: std::cell::Cell<i64> = std::cell::Cell::new(1);
    static JET_LOG_SAMPLE_COUNT: std::cell::Cell<i64> = std::cell::Cell::new(0);
    static JET_LOG_NEXT_SPAN: std::cell::Cell<i64> = std::cell::Cell::new(1);
}

fn jet_ring_log_set_level(level: &String) {
    let n: u8 = match level.as_str() {
        "debug" => 0,
        "info" => 1,
        "warn" => 2,
        "error" => 3,
        _ => 1,
    };
    JET_LOG_LEVEL.with(|l| l.set(n));
}

fn jet_ring_log_set_trace_id(id: &String) {
    JET_LOG_TRACE_ID.with(|t| *t.borrow_mut() = id.clone());
}

// D-LOGFMT1=A: explicit format override.
fn jet_ring_log_setup(format: &String) {
    let n: u8 = match format.as_str() {
        "json" => 1,
        "text" => 2,
        _ => 0,
    };
    JET_LOG_FORMAT.with(|f| f.set(n));
}

fn jet_ring_log_set_sink(kind: &String, path: &String) {
    let n: u8 = match kind.as_str() {
        "jsonl" | "json" => 1,
        "text" => 2,
        _ => 1,
    };
    JET_LOG_FORMAT.with(|f| f.set(n));
    JET_LOG_SINK_PATH.with(|p| *p.borrow_mut() = path.clone());
}

fn jet_ring_log_otlp_file(path: &String) {
    jet_ring_log_set_sink(&"jsonl".to_string(), path);
}

fn jet_ring_log_sample_every(n: i64) {
    JET_LOG_SAMPLE_EVERY.with(|s| s.set(n.max(1)));
    JET_LOG_SAMPLE_COUNT.with(|c| c.set(0));
}

fn jet_ring_log_field(key: &String, value: &String) -> jet_std::LogField {
    jet_std::LogField {
        key: key.clone(),
        value: value.clone(),
        kind: "string".to_string(),
        redacted: false,
    }
}

fn jet_ring_log_int(key: &String, value: i64) -> jet_std::LogField {
    jet_std::LogField {
        key: key.clone(),
        value: value.to_string(),
        kind: "int".to_string(),
        redacted: false,
    }
}

fn jet_ring_log_float(key: &String, value: f64) -> jet_std::LogField {
    jet_std::LogField {
        key: key.clone(),
        value: value.to_string(),
        kind: "float".to_string(),
        redacted: false,
    }
}

fn jet_ring_log_bool(key: &String, value: bool) -> jet_std::LogField {
    jet_std::LogField {
        key: key.clone(),
        value: value.to_string(),
        kind: "bool".to_string(),
        redacted: false,
    }
}

fn jet_ring_log_redact(key: &String) -> jet_std::LogField {
    jet_std::LogField {
        key: key.clone(),
        value: "[redacted]".to_string(),
        kind: "redacted".to_string(),
        redacted: true,
    }
}

fn jet_ring_log_counter(name: &String, value: i64) -> jet_std::LogField {
    jet_std::LogField {
        key: format!("metric.counter.{}", name),
        value: value.to_string(),
        kind: "counter".to_string(),
        redacted: false,
    }
}

fn jet_ring_log_span(name: &String) -> jet_std::LogSpan {
    let id = JET_LOG_NEXT_SPAN.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    jet_std::LogSpan {
        id,
        name: name.clone(),
    }
}

fn jet_ring_log_enter(span: &jet_std::LogSpan) {
    JET_LOG_SPANS.with(|s| s.borrow_mut().push(span.clone()));
}

fn jet_ring_log_close(span: &jet_std::LogSpan) {
    JET_LOG_SPANS.with(|s| {
        let mut spans = s.borrow_mut();
        if let Some(pos) = spans.iter().rposition(|x| x.id == span.id) {
            spans.remove(pos);
        }
    });
}

fn jet_log_format_active() -> u8 {
    let explicit = JET_LOG_FORMAT.with(|f| f.get());
    if explicit != 0 {
        return explicit;
    }
    // Auto-detect: text if stderr is a terminal, JSON otherwise.
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() {
        2
    } else {
        1
    }
}

fn jet_log_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn jet_log_fields_json(fields: &[jet_std::LogField]) -> String {
    let mut out = String::new();
    for field in fields {
        out.push_str(",\"");
        out.push_str(&jet_log_json_escape(&field.key));
        out.push_str("\":");
        if field.kind == "int" || field.kind == "float" || field.kind == "bool" || field.kind == "counter" {
            out.push_str(&field.value);
        } else {
            out.push('"');
            out.push_str(&jet_log_json_escape(&field.value));
            out.push('"');
        }
    }
    out
}

fn jet_log_spans_json() -> String {
    JET_LOG_SPANS.with(|s| {
        let spans = s.borrow();
        if spans.is_empty() {
            return String::new();
        }
        let names = spans
            .iter()
            .map(|span| format!("\"{}\"", jet_log_json_escape(&span.name)))
            .collect::<Vec<_>>()
            .join(",");
        format!(",\"spans\":[{}]", names)
    })
}

fn jet_log_write(line: &str) {
    let path = JET_LOG_SINK_PATH.with(|p| p.borrow().clone());
    if path.is_empty() {
        eprintln!("{}", line);
    } else if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(f, "{}", line);
    }
}

fn jet_log_emit_json(level: &str, msg: &str, ts: i64, fields: &[jet_std::LogField]) {
    let trace = JET_LOG_TRACE_ID.with(|t| t.borrow().clone());
    let fields_json = jet_log_fields_json(fields);
    let spans_json = jet_log_spans_json();
    let line = if trace.is_empty() {
        format!(
            "{{\"level\":\"{}\",\"body\":\"{}\",\"ts\":{}{}{} }}",
            level, jet_log_json_escape(msg), ts, fields_json, spans_json
        )
    } else {
        format!(
            "{{\"level\":\"{}\",\"body\":\"{}\",\"trace_id\":\"{}\",\"ts\":{}{}{} }}",
            level, jet_log_json_escape(msg), jet_log_json_escape(&trace), ts, fields_json, spans_json
        )
    };
    jet_log_write(&line.replace(" }", "}"));
}

fn jet_log_emit_text(level: &str, msg: &str, ts: i64, fields: &[jet_std::LogField]) {
    let secs = ts / 1000;
    let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
    let level_tag = match level {
        "debug" => "DEBUG",
        "info" => "INFO",
        "warn" => "WARN",
        "error" => "ERROR",
        _ => level,
    };
    let trace = JET_LOG_TRACE_ID.with(|t| t.borrow().clone());
    let field_text = if fields.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            fields
                .iter()
                .map(|f| format!("{}={}", f.key, f.value))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    let line = if trace.is_empty() {
        format!("[{}] {:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z | {}{}", level_tag, y, mo, d, h, mi, s, msg, field_text)
    } else {
        format!("[{}] {:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z trace={} | {}{}", level_tag, y, mo, d, h, mi, s, trace, msg, field_text)
    };
    jet_log_write(&line);
}

fn jet_log_emit(level: &str, msg: &str, fields: &[jet_std::LogField]) {
    let keep = JET_LOG_SAMPLE_EVERY.with(|every| {
        JET_LOG_SAMPLE_COUNT.with(|count| {
            let next = count.get() + 1;
            count.set(next);
            every.get() <= 1 || (next - 1) % every.get() == 0
        })
    });
    if !keep {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    if jet_log_format_active() == 2 {
        jet_log_emit_text(level, msg, ts, fields);
    } else {
        jet_log_emit_json(level, msg, ts, fields);
    }
}

fn jet_ring_log_debug(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 0 {
        jet_log_emit("debug", msg, &[]);
    }
}
fn jet_ring_log_info(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 1 {
        jet_log_emit("info", msg, &[]);
    }
}
fn jet_ring_log_warn(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 2 {
        jet_log_emit("warn", msg, &[]);
    }
}
fn jet_ring_log_error(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 3 {
        jet_log_emit("error", msg, &[]);
    }
}

fn jet_ring_log_debug_fields(msg: &String, fields: &Vec<jet_std::LogField>) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 0 {
        jet_log_emit("debug", msg, fields);
    }
}
fn jet_ring_log_info_fields(msg: &String, fields: &Vec<jet_std::LogField>) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 1 {
        jet_log_emit("info", msg, fields);
    }
}
fn jet_ring_log_warn_fields(msg: &String, fields: &Vec<jet_std::LogField>) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 2 {
        jet_log_emit("warn", msg, fields);
    }
}
fn jet_ring_log_error_fields(msg: &String, fields: &Vec<jet_std::LogField>) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 3 {
        jet_log_emit("error", msg, fields);
    }
}

// ── jet.time ──────────────────────────────────────────────────────────────────
// Format a Unix millisecond timestamp using a strftime-like pattern.
// Supported tokens: %Y year, %m month, %d day, %H hour, %M minute, %S second.
fn jet_ring_time_format(millis: i64, fmt: &String) -> String {
    let secs = (millis / 1000) as i64;
    let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
    let mut out = fmt.clone();
    out = out.replace("%Y", &format!("{:04}", y));
    out = out.replace("%m", &format!("{:02}", mo));
    out = out.replace("%d", &format!("{:02}", d));
    out = out.replace("%H", &format!("{:02}", h));
    out = out.replace("%M", &format!("{:02}", mi));
    out = out.replace("%S", &format!("{:02}", s));
    out
}

fn unix_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Days since epoch, treating every year as having the right leap-year logic.
    let mut days = secs / 86400;
    let time_of_day = (secs % 86400).unsigned_abs();
    let h = (time_of_day / 3600) as u32;
    let mi = ((time_of_day % 3600) / 60) as u32;
    let s = (time_of_day % 60) as u32;
    // Walk from 1970.
    let mut year: i32 = 1970;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let month_days: [i64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month: u32 = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, (days + 1) as u32, h, mi, s)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

// ── jet.crypto ────────────────────────────────────────────────────────────────
// SHA-256 of a UTF-8 string, returned as a lowercase hex string.
fn jet_ring_crypto_sha256(s: &String) -> String {
    let hash = jet_sha256_raw(s.as_bytes());
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

// SHA-256 of a byte list (Vec<u8>), returned as lowercase hex.
fn jet_ring_crypto_sha256_bytes(bs: &Vec<u8>) -> String {
    let hash = jet_sha256_raw(bs.as_slice());
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

// D-TTLVAL1=A: Expiring<T> / Rotting<T> (pure std, injectable Clock).
fn jet_expiring_new<T: Clone>(value: T, ttl_ms: i64, clock_now: i64) -> JetExpiring<T> {
    JetExpiring::new(value, clock_now.saturating_add(ttl_ms))
}
fn jet_rotting_new<T: Clone + 'static>(value: T, ttl_ms: i64, clock_now: i64) -> JetRotting<T> {
    JetRotting::new(value, clock_now.saturating_add(ttl_ms))
}
fn jet_expiring_get<T: Clone>(exp: &JetExpiring<T>, now_ms: i64) -> Result<T, JetExpired> {
    exp.get(now_ms)
}
fn jet_rotting_get<T: Clone + 'static>(
    rot: &mut JetRotting<T>,
    now_ms: i64,
) -> Result<T, JetExpired> {
    rot.get(now_ms)
}

// Minimal SHA-256 (same algorithm as src/sha256.rs — duplicated here so the
// prelude doesn't need to reach into the compiler crate; I6 forbids extern deps).
fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut state = H0;
    let bit_len = (data.len() as u64) * 8;
    let mut msg: Vec<u8> = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = [
            state[0], state[1], state[2], state[3], state[4], state[5], state[6], state[7],
        ];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for (i, &s) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&s.to_be_bytes());
    }
    out
}

// ── D-UUIDENC1=A: core.encoding.hex / core.encoding.base64 / core.uuid ───────
// Pure std implementations; zero external crates (I6); memory-safe (I1).

fn jet_std_hex_encode(bytes: &Vec<u8>) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn jet_std_hex_decode(text: &String) -> Result<Vec<u8>, String> {
    let s = text.trim();
    if s.len() % 2 != 0 {
        return Err(format!("hex string has odd length ({})", s.len()));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        match u8::from_str_radix(&s[i..i + 2], 16) {
            Ok(b) => out.push(b),
            Err(_) => return Err(format!("invalid hex at offset {}: {:?}", i, &s[i..i + 2])),
        }
    }
    Ok(out)
}

const JET_B64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn jet_std_b64_encode(bytes: &Vec<u8>) -> String {
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(JET_B64_CHARS[(n >> 18) as usize] as char);
        out.push(JET_B64_CHARS[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            JET_B64_CHARS[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            JET_B64_CHARS[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn jet_b64_val(b: u8) -> Result<u8, String> {
    match b {
        b'A'..=b'Z' => Ok(b - b'A'),
        b'a'..=b'z' => Ok(b - b'a' + 26),
        b'0'..=b'9' => Ok(b - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(format!("invalid base64 character: {:?}", b as char)),
    }
}

fn jet_std_b64_decode(text: &String) -> Result<Vec<u8>, String> {
    let input: Vec<u8> = text.bytes().filter(|&b| !b.is_ascii_whitespace()).collect();
    if input.len() % 4 != 0 {
        return Err(format!(
            "base64 length must be a multiple of 4 (got {})",
            input.len()
        ));
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.chunks(4) {
        let a = jet_b64_val(chunk[0])?;
        let b = jet_b64_val(chunk[1])?;
        out.push(((a << 2) | (b >> 4)) as u8);
        if chunk[2] != b'=' {
            let c = jet_b64_val(chunk[2])?;
            out.push(((b << 4) | (c >> 2)) as u8);
            if chunk[3] != b'=' {
                let d = jet_b64_val(chunk[3])?;
                out.push(((c << 6) | d) as u8);
            }
        }
    }
    Ok(out)
}

fn jet_std_b64url_encode(bytes: &Vec<u8>) -> String {
    jet_std_b64_encode(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}
fn jet_std_b64url_decode(text: &String) -> Result<Vec<u8>, String> {
    let mut s = text.trim().replace('-', "+").replace('_', "/");
    while s.len() % 4 != 0 {
        s.push('=');
    }
    jet_std_b64_decode(&s)
}

const JET_BASE32_CHARS: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
fn jet_std_base32_encode(bytes: &Vec<u8>) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            let idx = ((buffer >> (bits - 5)) & 31) as usize;
            out.push(JET_BASE32_CHARS[idx] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let idx = ((buffer << (5 - bits)) & 31) as usize;
        out.push(JET_BASE32_CHARS[idx] as char);
    }
    while out.len() % 8 != 0 {
        out.push('=');
    }
    out
}
fn jet_base32_val(b: u8) -> Result<u8, String> {
    match b {
        b'A'..=b'Z' => Ok(b - b'A'),
        b'a'..=b'z' => Ok(b - b'a'),
        b'2'..=b'7' => Ok(b - b'2' + 26),
        _ => Err(format!("invalid base32 character: {:?}", b as char)),
    }
}
fn jet_std_base32_decode(text: &String) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for b in text.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=') {
        buffer = (buffer << 5) | jet_base32_val(b)? as u32;
        bits += 5;
        if bits >= 8 {
            out.push(((buffer >> (bits - 8)) & 0xff) as u8);
            bits -= 8;
        }
    }
    Ok(out)
}

fn jet_xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn jet_xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
fn jet_xml_obj(name: String, attrs: Vec<(String, String)>, children: Vec<jet_std::DataTree>, text: String) -> jet_std::DataTree {
    jet_std::DataTree::Object(vec![
        ("name".to_string(), jet_std::DataTree::Text(name)),
        (
            "attrs".to_string(),
            jet_std::DataTree::Object(
                attrs
                    .into_iter()
                    .map(|(k, v)| (k, jet_std::DataTree::Text(v)))
                    .collect(),
            ),
        ),
        ("children".to_string(), jet_std::DataTree::Array(children)),
        ("text".to_string(), jet_std::DataTree::Text(text)),
    ])
}
fn jet_std_xml_parse(text: &String) -> Result<jet_std::DataTree, String> {
    #[derive(Clone)]
    struct Node { name: String, attrs: Vec<(String, String)>, children: Vec<jet_std::DataTree>, text: String }
    fn finish(n: Node) -> jet_std::DataTree { jet_xml_obj(n.name, n.attrs, n.children, n.text.trim().to_string()) }
    fn parse_tag(src: &str) -> Result<(String, Vec<(String, String)>, bool), String> {
        let mut s = src.trim().to_string();
        let self_close = s.ends_with('/');
        if self_close { s.pop(); }
        let mut parts = s.split_whitespace();
        let name = parts.next().ok_or_else(|| "empty XML tag".to_string())?.to_string();
        let mut attrs = Vec::new();
        let rest = &s[name.len()..];
        let mut i = 0usize;
        let bytes = rest.as_bytes();
        while i < bytes.len() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() { i += 1; }
            if i >= bytes.len() { break; }
            let start = i;
            while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() { i += 1; }
            let key = rest[start..i].trim().to_string();
            while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'=') { i += 1; }
            if i >= bytes.len() || bytes[i] != b'"' { return Err(format!("XML attribute `{key}` needs quoted value")); }
            i += 1;
            let val_start = i;
            while i < bytes.len() && bytes[i] != b'"' { i += 1; }
            if i >= bytes.len() { return Err(format!("XML attribute `{key}` is unterminated")); }
            attrs.push((key, jet_xml_unescape(&rest[val_start..i])));
            i += 1;
        }
        Ok((name, attrs, self_close))
    }
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<jet_std::DataTree> = None;
    let mut i = 0usize;
    while let Some(rel) = text[i..].find('<') {
        let start = i + rel;
        if start > i {
            if let Some(top) = stack.last_mut() { top.text.push_str(&jet_xml_unescape(&text[i..start])); }
        }
        let end = text[start..].find('>').ok_or_else(|| "unterminated XML tag".to_string())? + start;
        let tag = text[start + 1..end].trim();
        if tag.starts_with("!--") || tag.starts_with('?') {
            i = end + 1;
            continue;
        }
        if let Some(close) = tag.strip_prefix('/') {
            let node = stack.pop().ok_or_else(|| format!("closing tag </{}> without opener", close.trim()))?;
            if node.name != close.trim() { return Err(format!("closing tag </{}> does not match <{}>", close.trim(), node.name)); }
            let tree = finish(node);
            if let Some(parent) = stack.last_mut() { parent.children.push(tree); } else { root = Some(tree); }
        } else {
            let (name, attrs, self_close) = parse_tag(tag)?;
            let node = Node { name, attrs, children: Vec::new(), text: String::new() };
            if self_close {
                let tree = finish(node);
                if let Some(parent) = stack.last_mut() { parent.children.push(tree); } else { root = Some(tree); }
            } else {
                stack.push(node);
            }
        }
        i = end + 1;
    }
    if i < text.len() {
        if let Some(top) = stack.last_mut() { top.text.push_str(&jet_xml_unescape(&text[i..])); }
    }
    if !stack.is_empty() { return Err(format!("unclosed XML tag <{}>", stack.last().unwrap().name)); }
    root.ok_or_else(|| "empty XML document".to_string())
}
fn jet_std_xml_render(d: &jet_std::DataTree) -> String {
    fn field<'a>(d: &'a jet_std::DataTree, name: &str) -> Option<&'a jet_std::DataTree> {
        if let jet_std::DataTree::Object(entries) = d {
            entries.iter().find(|(k, _)| k == name).map(|(_, v)| v)
        } else { None }
    }
    fn render_node(d: &jet_std::DataTree) -> String {
        let name = match field(d, "name") { Some(jet_std::DataTree::Text(s)) => s.clone(), _ => "node".to_string() };
        let attrs = match field(d, "attrs") {
            Some(jet_std::DataTree::Object(es)) => es.iter().filter_map(|(k, v)| match v {
                jet_std::DataTree::Text(s) => Some(format!(" {}=\"{}\"", k, jet_xml_escape(s))),
                _ => None,
            }).collect::<String>(),
            _ => String::new(),
        };
        let text = match field(d, "text") { Some(jet_std::DataTree::Text(s)) => jet_xml_escape(s), _ => String::new() };
        let children = match field(d, "children") {
            Some(jet_std::DataTree::Array(xs)) => xs.iter().map(render_node).collect::<String>(),
            _ => String::new(),
        };
        if text.is_empty() && children.is_empty() {
            format!("<{name}{attrs}/>")
        } else {
            format!("<{name}{attrs}>{text}{children}</{name}>")
        }
    }
    render_node(d)
}

fn jet_cbor_push_len(out: &mut Vec<u8>, major: u8, n: u64) {
    if n < 24 { out.push((major << 5) | n as u8); }
    else if n <= u8::MAX as u64 { out.extend_from_slice(&[(major << 5) | 24, n as u8]); }
    else if n <= u16::MAX as u64 { out.push((major << 5) | 25); out.extend_from_slice(&(n as u16).to_be_bytes()); }
    else if n <= u32::MAX as u64 { out.push((major << 5) | 26); out.extend_from_slice(&(n as u32).to_be_bytes()); }
    else { out.push((major << 5) | 27); out.extend_from_slice(&n.to_be_bytes()); }
}
fn jet_cbor_encode_val(v: &jet_std::DataTree, out: &mut Vec<u8>) {
    match v {
        jet_std::DataTree::Null => out.push(0xf6),
        jet_std::DataTree::Bool(false) => out.push(0xf4),
        jet_std::DataTree::Bool(true) => out.push(0xf5),
        jet_std::DataTree::Int(n) if *n >= 0 => jet_cbor_push_len(out, 0, *n as u64),
        jet_std::DataTree::Int(n) => jet_cbor_push_len(out, 1, (-1 - *n) as u64),
        jet_std::DataTree::Float(f) => { out.push(0xfb); out.extend_from_slice(&f.to_be_bytes()); }
        jet_std::DataTree::Text(s) => { jet_cbor_push_len(out, 3, s.len() as u64); out.extend_from_slice(s.as_bytes()); }
        jet_std::DataTree::Bytes(bs) => { jet_cbor_push_len(out, 2, bs.len() as u64); out.extend_from_slice(bs); }
        jet_std::DataTree::Array(xs) => { jet_cbor_push_len(out, 4, xs.len() as u64); for x in xs { jet_cbor_encode_val(x, out); } }
        jet_std::DataTree::Object(es) => {
            let mut sorted = es.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            jet_cbor_push_len(out, 5, sorted.len() as u64);
            for (k, v) in sorted { jet_cbor_encode_val(&jet_std::DataTree::Text(k), out); jet_cbor_encode_val(&v, out); }
        }
    }
}
fn jet_std_cbor_encode(d: &jet_std::DataTree) -> Vec<u8> {
    let mut out = Vec::new();
    jet_cbor_encode_val(d, &mut out);
    out
}
fn jet_cbor_read_len(input: &[u8], i: &mut usize, add: u8) -> Result<u64, String> {
    let need = match add { n @ 0..=23 => return Ok(n as u64), 24 => 1, 25 => 2, 26 => 4, 27 => 8, _ => return Err("unsupported CBOR indefinite/simple length".to_string()) };
    if *i + need > input.len() { return Err("truncated CBOR length".to_string()); }
    let mut n = 0u64;
    for _ in 0..need { n = (n << 8) | input[*i] as u64; *i += 1; }
    Ok(n)
}
fn jet_cbor_decode_val(input: &[u8], i: &mut usize) -> Result<jet_std::DataTree, String> {
    if *i >= input.len() { return Err("truncated CBOR value".to_string()); }
    let b = input[*i]; *i += 1;
    let major = b >> 5; let add = b & 31;
    match major {
        0 => Ok(jet_std::DataTree::Int(jet_cbor_read_len(input, i, add)? as i64)),
        1 => Ok(jet_std::DataTree::Int(-1 - jet_cbor_read_len(input, i, add)? as i64)),
        2 | 3 => {
            let n = jet_cbor_read_len(input, i, add)? as usize;
            if *i + n > input.len() { return Err("truncated CBOR bytes/text".to_string()); }
            let bytes = input[*i..*i + n].to_vec(); *i += n;
            if major == 2 { Ok(jet_std::DataTree::Bytes(bytes)) } else { String::from_utf8(bytes).map(jet_std::DataTree::Text).map_err(|_| "CBOR text is not UTF-8".to_string()) }
        }
        4 => {
            let n = jet_cbor_read_len(input, i, add)? as usize;
            let mut xs = Vec::with_capacity(n);
            for _ in 0..n { xs.push(jet_cbor_decode_val(input, i)?); }
            Ok(jet_std::DataTree::Array(xs))
        }
        5 => {
            let n = jet_cbor_read_len(input, i, add)? as usize;
            let mut es = Vec::with_capacity(n);
            for _ in 0..n {
                let k = match jet_cbor_decode_val(input, i)? { jet_std::DataTree::Text(s) => s, _ => return Err("CBOR map key must be text".to_string()) };
                es.push((k, jet_cbor_decode_val(input, i)?));
            }
            Ok(jet_std::DataTree::Object(es))
        }
        7 => match add {
            20 => Ok(jet_std::DataTree::Bool(false)),
            21 => Ok(jet_std::DataTree::Bool(true)),
            22 => Ok(jet_std::DataTree::Null),
            27 => { if *i + 8 > input.len() { return Err("truncated CBOR float".to_string()); } let mut buf = [0u8; 8]; buf.copy_from_slice(&input[*i..*i+8]); *i += 8; Ok(jet_std::DataTree::Float(f64::from_be_bytes(buf))) }
            _ => Err("unsupported CBOR simple value".to_string()),
        },
        _ => Err("unsupported CBOR major type".to_string()),
    }
}
fn jet_std_cbor_decode(bytes: &Vec<u8>) -> Result<jet_std::DataTree, String> {
    let mut i = 0usize;
    let v = jet_cbor_decode_val(bytes, &mut i)?;
    if i != bytes.len() { return Err("trailing CBOR bytes".to_string()); }
    Ok(v)
}

// UUID helpers — pure std, zero deps. CSPRNG via /dev/urandom (POSIX); the
// fallback SplitMix64 engages only when /dev/urandom is unavailable.
fn jet_uuid_fill_random(out: &mut [u8]) {
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(out).is_ok() {
            return;
        }
    }
    // Fallback: SplitMix64 seeded from wall-clock nanoseconds.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    for b in out.iter_mut() {
        state = state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = (state ^ (state >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        *b = (z ^ (z >> 31)) as u8;
    }
}

fn jet_uuid_format(b: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
         {:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

fn jet_std_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    jet_uuid_fill_random(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx
    jet_uuid_format(&bytes)
}

fn jet_std_uuid_v7(clock: &jet_std::Clock) -> String {
    let ts_ms = clock.now as u64;
    let mut bytes = [0u8; 16];
    // 48-bit timestamp in the high bytes
    bytes[0] = (ts_ms >> 40) as u8;
    bytes[1] = (ts_ms >> 32) as u8;
    bytes[2] = (ts_ms >> 24) as u8;
    bytes[3] = (ts_ms >> 16) as u8;
    bytes[4] = (ts_ms >> 8) as u8;
    bytes[5] = ts_ms as u8;
    jet_uuid_fill_random(&mut bytes[6..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70; // version 7
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx
    jet_uuid_format(&bytes)
}

// ── E2-M10: networking (core.net + jet.http) ─────────────────────────────────
// All networking uses std::net only — zero external crates in the prelude (I6).
// TLS (D-NET1) is delivered as the `jet.tls` FFI package and is not included here.

pub struct JetTcpListener {
    inner: std::net::TcpListener,
}

pub struct JetTcpStream {
    inner: std::net::TcpStream,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetIpAddr {
    inner: std::net::IpAddr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetSocketAddr {
    inner: std::net::SocketAddr,
}

pub struct JetUdpSocket {
    inner: std::net::UdpSocket,
}

#[derive(Clone, Debug)]
pub struct JetUdpPacket {
    data: String,
    addr: JetSocketAddr,
}

#[derive(Clone, Debug)]
pub struct JetDnsSrv {
    priority: i64,
    weight: i64,
    port: i64,
    target: String,
}

#[cfg(unix)]
pub struct JetUnixListener {
    inner: std::os::unix::net::UnixListener,
}

#[cfg(unix)]
pub struct JetUnixStream {
    inner: std::os::unix::net::UnixStream,
}

#[cfg(not(unix))]
pub struct JetUnixListener;

#[cfg(not(unix))]
pub struct JetUnixStream;

pub struct JetTlsStream {
    id: i64,
}

#[derive(Clone)]
pub struct JetHttpRequest {
    pub method: String,
    pub path: String,
    pub body: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub params: std::collections::BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct JetHttpResponse {
    pub status: String,
    pub body: String,
    pub headers: std::collections::BTreeMap<String, String>,
}

// D-ROUTE1=A: HTTP router — registration + :param dispatch.
#[derive(Clone)]
enum RouteSegment {
    Static(String),
    Param(String),
}

type JetHttpHandler = Box<dyn Fn(JetHttpRequest) -> JetHttpResponse + Send + Sync>;

struct JetHttpRoute {
    method: String,
    segments: Vec<RouteSegment>,
    handler: JetHttpHandler,
}

pub struct JetHttpRouter {
    routes: Vec<JetHttpRoute>,
}

impl JetShow for JetTcpListener {
    fn jet_show(&self) -> String {
        format!(
            "TcpListener({})",
            self.inner
                .local_addr()
                .map(|a| a.to_string())
                .unwrap_or_default()
        )
    }
}
impl JetShow for JetTcpStream {
    fn jet_show(&self) -> String {
        format!(
            "TcpStream({})",
            self.inner
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_default()
        )
    }
}
impl JetShow for JetIpAddr {
    fn jet_show(&self) -> String {
        self.inner.to_string()
    }
}
impl JetShow for JetSocketAddr {
    fn jet_show(&self) -> String {
        self.inner.to_string()
    }
}
impl JetShow for JetUdpSocket {
    fn jet_show(&self) -> String {
        format!(
            "UdpSocket({})",
            self.inner
                .local_addr()
                .map(|a| a.to_string())
                .unwrap_or_default()
        )
    }
}
impl JetShow for JetUdpPacket {
    fn jet_show(&self) -> String {
        format!("UdpPacket({} bytes from {})", self.data.len(), self.addr.inner)
    }
}
impl JetShow for JetDnsSrv {
    fn jet_show(&self) -> String {
        format!(
            "DnsSrv(priority={}, weight={}, port={}, target={})",
            self.priority, self.weight, self.port, self.target
        )
    }
}
impl JetShow for JetUnixListener {
    fn jet_show(&self) -> String {
        "UnixListener".to_string()
    }
}
impl JetShow for JetUnixStream {
    fn jet_show(&self) -> String {
        "UnixStream".to_string()
    }
}
impl JetShow for JetTlsStream {
    fn jet_show(&self) -> String {
        format!("TlsStream({})", self.id)
    }
}

fn jet_net_timeout(ms: i64) -> Result<std::time::Duration, String> {
    if ms < 0 {
        return Err("network timeout must be non-negative".to_string());
    }
    Ok(std::time::Duration::from_millis(ms as u64))
}

fn jet_net_apply_tcp_deadlines(stream: &std::net::TcpStream, op: &str) {
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded(op);
        }
        let dur = Some(std::time::Duration::from_millis(remaining as u64));
        let _ = stream.set_read_timeout(dur);
        let _ = stream.set_write_timeout(dur);
    }
}

fn jet_net_apply_udp_deadline(socket: &std::net::UdpSocket, op: &str) {
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded(op);
        }
        let dur = Some(std::time::Duration::from_millis(remaining as u64));
        let _ = socket.set_read_timeout(dur);
        let _ = socket.set_write_timeout(dur);
    }
}

fn jet_net_ip_addr(text: &String) -> Result<JetIpAddr, String> {
    text.parse::<std::net::IpAddr>()
        .map(|inner| JetIpAddr { inner })
        .map_err(|e| format!("invalid IP address `{}`: {}", text, e))
}

fn jet_net_ip_to_string(ip: &JetIpAddr) -> String {
    ip.inner.to_string()
}

fn jet_net_ip_is_ipv4(ip: &JetIpAddr) -> bool {
    ip.inner.is_ipv4()
}

fn jet_net_socket_addr(host: &String, port: i64) -> Result<JetSocketAddr, String> {
    if port < 0 || port > u16::MAX as i64 {
        return Err(format!("invalid port `{}`: expected 0..65535", port));
    }
    let text = format!("{}:{}", host, port);
    text.parse::<std::net::SocketAddr>()
        .or_else(|_| {
            use std::net::ToSocketAddrs;
            text.to_socket_addrs()?
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no address"))
        })
        .map(|inner| JetSocketAddr { inner })
        .map_err(|e| format!("resolve `{}` failed: {}", text, e))
}

fn jet_net_socket_addr_parse(text: &String) -> Result<JetSocketAddr, String> {
    text.parse::<std::net::SocketAddr>()
        .map(|inner| JetSocketAddr { inner })
        .map_err(|e| format!("invalid socket address `{}`: {}", text, e))
}

fn jet_net_socket_host(addr: &JetSocketAddr) -> String {
    addr.inner.ip().to_string()
}

fn jet_net_socket_port(addr: &JetSocketAddr) -> i64 {
    addr.inner.port() as i64
}

fn jet_net_socket_to_string(addr: &JetSocketAddr) -> String {
    addr.inner.to_string()
}

fn jet_net_tcp_listen_addr(addr: &JetSocketAddr) -> Result<JetTcpListener, String> {
    std::net::TcpListener::bind(addr.inner)
        .map(|l| JetTcpListener { inner: l })
        .map_err(|e| format!("bind on `{}` failed: {}", addr.inner, e))
}

fn jet_net_tcp_connect_addr(addr: &JetSocketAddr) -> Result<JetTcpStream, String> {
    std::net::TcpStream::connect(addr.inner)
        .map(|s| JetTcpStream { inner: s })
        .map_err(|e| format!("connect to `{}` failed: {}", addr.inner, e))
}

fn jet_net_tcp_connect_timeout(addr: &JetSocketAddr, ms: i64) -> Result<JetTcpStream, String> {
    let timeout = jet_net_timeout(ms)?;
    std::net::TcpStream::connect_timeout(&addr.inner, timeout)
        .map(|s| JetTcpStream { inner: s })
        .map_err(|e| format!("connect to `{}` failed: {}", addr.inner, e))
}

fn jet_net_tcp_connect_happy(host: &String, port: i64, ms: i64) -> Result<JetTcpStream, String> {
    if port < 0 || port > u16::MAX as i64 {
        return Err(format!("invalid port `{}`: expected 0..65535", port));
    }
    let timeout = jet_net_timeout(ms)?;
    let deadline = std::time::Instant::now() + timeout;
    let mut addrs: Vec<std::net::SocketAddr> = {
        use std::net::ToSocketAddrs;
        (host.as_str(), port as u16)
            .to_socket_addrs()
            .map_err(|e| format!("resolve `{}` failed: {}", host, e))?
            .collect()
    };
    addrs.sort_by_key(|a| if a.is_ipv6() { 0 } else { 1 });
    let mut last = "no address".to_string();
    for addr in addrs {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        match std::net::TcpStream::connect_timeout(&addr, deadline.saturating_duration_since(now)) {
            Ok(s) => return Ok(JetTcpStream { inner: s }),
            Err(e) => last = format!("{}: {}", addr, e),
        }
    }
    Err(format!("connect to `{}` failed: {}", host, last))
}

fn jet_net_listener_local_socket_addr(listener: &JetTcpListener) -> JetSocketAddr {
    JetSocketAddr {
        inner: listener
            .inner
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
    }
}

fn jet_net_tcp_local_socket_addr(stream: &JetTcpStream) -> JetSocketAddr {
    JetSocketAddr {
        inner: stream
            .inner
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
    }
}

fn jet_net_tcp_peer_socket_addr(stream: &JetTcpStream) -> JetSocketAddr {
    JetSocketAddr {
        inner: stream
            .inner
            .peer_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
    }
}
impl JetShow for JetHttpRequest {
    fn jet_show(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}
impl JetShow for JetHttpResponse {
    fn jet_show(&self) -> String {
        format!("HTTP {}", self.status)
    }
}
impl JetShow for JetHttpRouter {
    fn jet_show(&self) -> String {
        format!("HttpRouter({} routes)", self.routes.len())
    }
}

fn jet_net_tcp_listen(addr: &String) -> Result<JetTcpListener, String> {
    std::net::TcpListener::bind(addr.as_str())
        .map(|l| JetTcpListener { inner: l })
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))
}

fn jet_net_tcp_accept(listener: &JetTcpListener) -> Result<JetTcpStream, String> {
    listener
        .inner
        .accept()
        .map(|(s, _)| JetTcpStream { inner: s })
        .map_err(|e| format!("accept failed: {}", e))
}

fn jet_net_tcp_connect(addr: &String) -> Result<JetTcpStream, String> {
    std::net::TcpStream::connect(addr.as_str())
        .map(|s| JetTcpStream { inner: s })
        .map_err(|e| format!("connect to `{}` failed: {}", addr, e))
}

fn jet_net_tcp_read(stream: &mut JetTcpStream) -> Result<String, String> {
    use std::io::Read;
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded("tcp read");
        }
        let _ = stream
            .inner
            .set_read_timeout(Some(std::time::Duration::from_millis(remaining as u64)));
    }
    let mut buf = [0u8; 8192];
    loop {
        match stream.inner.read(&mut buf) {
            Ok(0) => return Ok(String::new()),
            Ok(n) => {
                jet_deadline_check("tcp read");
                return String::from_utf8(buf[..n].to_vec())
                    .map_err(|e| format!("tcp read: invalid UTF-8: {}", e));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                jet_scheduler_io_wait(&stream.inner, true, false, "tcp read");
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    jet_deadline_exceeded("tcp read");
                }
                return Err(format!("tcp read failed: {}", e));
            }
        }
    }
}

fn jet_net_tcp_write(stream: &mut JetTcpStream, data: &String) -> Result<(), String> {
    use std::io::Write;
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded("tcp write");
        }
        let _ = stream
            .inner
            .set_write_timeout(Some(std::time::Duration::from_millis(remaining as u64)));
    }
    let bytes = data.as_bytes();
    let mut off = 0usize;
    while off < bytes.len() {
        match stream.inner.write(&bytes[off..]) {
            Ok(0) => return Err("tcp write failed: zero bytes written".to_string()),
            Ok(n) => off += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                jet_scheduler_io_wait(&stream.inner, false, true, "tcp write");
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    jet_deadline_exceeded("tcp write");
                }
                return Err(format!("tcp write failed: {}", e));
            }
        }
    }
    jet_deadline_check("tcp write");
    Ok(())
}

fn jet_net_tcp_local_addr(stream: &JetTcpStream) -> String {
    stream
        .inner
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_default()
}

fn jet_net_tcp_peer_addr(stream: &JetTcpStream) -> String {
    stream
        .inner
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_default()
}

fn jet_net_listener_local_addr(listener: &JetTcpListener) -> String {
    listener
        .inner
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_default()
}

fn jet_net_set_timeout(stream: &mut JetTcpStream, ms: i64) {
    if let Ok(dur) = jet_net_timeout(ms) {
        let _ = stream.inner.set_read_timeout(Some(dur));
        let _ = stream.inner.set_write_timeout(Some(dur));
    }
}

fn jet_net_set_read_timeout(stream: &mut JetTcpStream, ms: i64) -> Result<(), String> {
    stream
        .inner
        .set_read_timeout(Some(jet_net_timeout(ms)?))
        .map_err(|e| format!("set tcp read timeout failed: {}", e))
}

fn jet_net_set_write_timeout(stream: &mut JetTcpStream, ms: i64) -> Result<(), String> {
    stream
        .inner
        .set_write_timeout(Some(jet_net_timeout(ms)?))
        .map_err(|e| format!("set tcp write timeout failed: {}", e))
}

fn jet_net_udp_bind(addr: &String) -> Result<JetUdpSocket, String> {
    std::net::UdpSocket::bind(addr.as_str())
        .map(|inner| JetUdpSocket { inner })
        .map_err(|e| format!("udp bind on `{}` failed: {}", addr, e))
}

fn jet_net_udp_bind_addr(addr: &JetSocketAddr) -> Result<JetUdpSocket, String> {
    std::net::UdpSocket::bind(addr.inner)
        .map(|inner| JetUdpSocket { inner })
        .map_err(|e| format!("udp bind on `{}` failed: {}", addr.inner, e))
}

fn jet_net_udp_local_addr(socket: &JetUdpSocket) -> JetSocketAddr {
    JetSocketAddr {
        inner: socket
            .inner
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
    }
}

fn jet_net_udp_set_timeout(socket: &JetUdpSocket, ms: i64) -> Result<(), String> {
    let dur = jet_net_timeout(ms)?;
    socket
        .inner
        .set_read_timeout(Some(dur))
        .map_err(|e| format!("set udp read timeout failed: {}", e))?;
    socket
        .inner
        .set_write_timeout(Some(dur))
        .map_err(|e| format!("set udp write timeout failed: {}", e))
}

fn jet_net_udp_send_to(
    socket: &JetUdpSocket,
    data: &String,
    addr: &JetSocketAddr,
) -> Result<i64, String> {
    jet_net_apply_udp_deadline(&socket.inner, "udp send");
    socket
        .inner
        .send_to(data.as_bytes(), addr.inner)
        .map(|n| n as i64)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                jet_deadline_exceeded("udp send");
            }
            format!("udp send to `{}` failed: {}", addr.inner, e)
        })
}

fn jet_net_udp_recv_from(socket: &JetUdpSocket, limit: i64) -> Result<JetUdpPacket, String> {
    if limit <= 0 {
        return Err("udp receive limit must be positive".to_string());
    }
    jet_net_apply_udp_deadline(&socket.inner, "udp receive");
    let cap = std::cmp::min(limit as usize, 1 << 20);
    let mut buf = vec![0u8; cap];
    socket
        .inner
        .recv_from(&mut buf)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                jet_deadline_exceeded("udp receive");
            }
            format!("udp receive failed: {}", e)
        })
        .and_then(|(n, addr)| {
            String::from_utf8(buf[..n].to_vec())
                .map(|data| JetUdpPacket {
                    data,
                    addr: JetSocketAddr { inner: addr },
                })
                .map_err(|e| format!("udp receive: invalid UTF-8: {}", e))
        })
}

fn jet_net_udp_packet_data(packet: &JetUdpPacket) -> String {
    packet.data.clone()
}

fn jet_net_udp_packet_addr(packet: &JetUdpPacket) -> JetSocketAddr {
    packet.addr.clone()
}

#[cfg(unix)]
fn jet_net_unix_listen(path: &String) -> Result<JetUnixListener, String> {
    let _ = std::fs::remove_file(path);
    std::os::unix::net::UnixListener::bind(path)
        .map(|inner| JetUnixListener { inner })
        .map_err(|e| format!("unix listen on `{}` failed: {}", path, e))
}

#[cfg(not(unix))]
fn jet_net_unix_listen(path: &String) -> Result<JetUnixListener, String> {
    Err(format!("unix sockets are not supported on this platform: {}", path))
}

#[cfg(unix)]
fn jet_net_unix_accept(listener: &JetUnixListener) -> Result<JetUnixStream, String> {
    listener
        .inner
        .accept()
        .map(|(inner, _)| JetUnixStream { inner })
        .map_err(|e| format!("unix accept failed: {}", e))
}

#[cfg(not(unix))]
fn jet_net_unix_accept(_listener: &JetUnixListener) -> Result<JetUnixStream, String> {
    Err("unix sockets are not supported on this platform".to_string())
}

#[cfg(unix)]
fn jet_net_unix_connect(path: &String) -> Result<JetUnixStream, String> {
    std::os::unix::net::UnixStream::connect(path)
        .map(|inner| JetUnixStream { inner })
        .map_err(|e| format!("unix connect to `{}` failed: {}", path, e))
}

#[cfg(not(unix))]
fn jet_net_unix_connect(path: &String) -> Result<JetUnixStream, String> {
    Err(format!("unix sockets are not supported on this platform: {}", path))
}

#[cfg(unix)]
fn jet_net_unix_read(stream: &mut JetUnixStream) -> Result<String, String> {
    use std::io::Read;
    let mut buf = [0u8; 8192];
    stream
        .inner
        .read(&mut buf)
        .map_err(|e| format!("unix read failed: {}", e))
        .and_then(|n| {
            String::from_utf8(buf[..n].to_vec())
                .map_err(|e| format!("unix read: invalid UTF-8: {}", e))
        })
}

#[cfg(not(unix))]
fn jet_net_unix_read(_stream: &mut JetUnixStream) -> Result<String, String> {
    Err("unix sockets are not supported on this platform".to_string())
}

#[cfg(unix)]
fn jet_net_unix_write(stream: &mut JetUnixStream, data: &String) -> Result<(), String> {
    use std::io::Write;
    stream
        .inner
        .write_all(data.as_bytes())
        .map_err(|e| format!("unix write failed: {}", e))
}

#[cfg(not(unix))]
fn jet_net_unix_write(_stream: &mut JetUnixStream, _data: &String) -> Result<(), String> {
    Err("unix sockets are not supported on this platform".to_string())
}

fn jet_net_dns_system_servers() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            if parts.next() == Some("nameserver") {
                if let Some(host) = parts.next() {
                    out.push(format!("{}:53", host));
                }
            }
        }
    }
    if out.is_empty() {
        out.push("1.1.1.1:53".to_string());
    }
    out
}

fn jet_net_dns_encode_name(out: &mut Vec<u8>, name: &str) -> Result<(), String> {
    let trimmed = name.trim_end_matches('.');
    if trimmed.is_empty() {
        out.push(0);
        return Ok(());
    }
    for label in trimmed.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(format!("invalid DNS name `{}`", name));
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    Ok(())
}

fn jet_net_dns_read_name(packet: &[u8], pos: &mut usize) -> Result<String, String> {
    let mut labels = Vec::new();
    let mut p = *pos;
    let mut jumped = false;
    let mut seen = 0usize;
    loop {
        if p >= packet.len() {
            return Err("truncated DNS name".to_string());
        }
        let len = packet[p];
        if len & 0xc0 == 0xc0 {
            if p + 1 >= packet.len() {
                return Err("truncated DNS compression pointer".to_string());
            }
            let ptr = (((len & 0x3f) as usize) << 8) | packet[p + 1] as usize;
            if !jumped {
                *pos = p + 2;
            }
            p = ptr;
            jumped = true;
            seen += 1;
            if seen > packet.len() {
                return Err("cyclic DNS compression pointer".to_string());
            }
            continue;
        }
        p += 1;
        if len == 0 {
            if !jumped {
                *pos = p;
            }
            break;
        }
        let end = p + len as usize;
        if end > packet.len() {
            return Err("truncated DNS label".to_string());
        }
        labels.push(String::from_utf8_lossy(&packet[p..end]).to_string());
        p = end;
        if !jumped {
            *pos = p;
        }
    }
    Ok(if labels.is_empty() {
        ".".to_string()
    } else {
        labels.join(".")
    })
}

fn jet_net_dns_query(server: &String, name: &String, qtype: u16, ms: i64) -> Result<Vec<Vec<u8>>, String> {
    let timeout = jet_net_timeout(ms)?;
    let server_addr = server
        .parse::<std::net::SocketAddr>()
        .map_err(|e| format!("invalid DNS server `{}`: {}", server, e))?;
    let socket = std::net::UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("dns socket bind failed: {}", e))?;
    socket
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("dns timeout setup failed: {}", e))?;
    let mut req = Vec::new();
    req.extend_from_slice(&0x4a57u16.to_be_bytes());
    req.extend_from_slice(&0x0100u16.to_be_bytes());
    req.extend_from_slice(&1u16.to_be_bytes());
    req.extend_from_slice(&0u16.to_be_bytes());
    req.extend_from_slice(&0u16.to_be_bytes());
    req.extend_from_slice(&0u16.to_be_bytes());
    jet_net_dns_encode_name(&mut req, name)?;
    req.extend_from_slice(&qtype.to_be_bytes());
    req.extend_from_slice(&1u16.to_be_bytes());
    socket
        .send_to(&req, server_addr)
        .map_err(|e| format!("dns query send failed: {}", e))?;
    let mut packet = vec![0u8; 4096];
    let (n, _) = socket
        .recv_from(&mut packet)
        .map_err(|e| format!("dns query for `{}` failed: {}", name, e))?;
    packet.truncate(n);
    if packet.len() < 12 {
        return Err("truncated DNS response".to_string());
    }
    let an = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    let mut pos = 12usize;
    let _ = jet_net_dns_read_name(&packet, &mut pos)?;
    pos += 4;
    let mut out = Vec::new();
    for _ in 0..an {
        let _ = jet_net_dns_read_name(&packet, &mut pos)?;
        if pos + 10 > packet.len() {
            return Err("truncated DNS answer".to_string());
        }
        let ty = u16::from_be_bytes([packet[pos], packet[pos + 1]]);
        let class = u16::from_be_bytes([packet[pos + 2], packet[pos + 3]]);
        let rdlen = u16::from_be_bytes([packet[pos + 8], packet[pos + 9]]) as usize;
        pos += 10;
        if pos + rdlen > packet.len() {
            return Err("truncated DNS rdata".to_string());
        }
        if ty == qtype && class == 1 {
            out.push(packet[pos..pos + rdlen].to_vec());
        }
        pos += rdlen;
    }
    Ok(out)
}

fn jet_net_dns_a(name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    for server in jet_net_dns_system_servers() {
        if let Ok(v) = jet_net_dns_a_at(&server, name, ms) {
            return Ok(v);
        }
    }
    Err(format!("DNS A lookup for `{}` failed", name))
}

fn jet_net_dns_aaaa(name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    for server in jet_net_dns_system_servers() {
        if let Ok(v) = jet_net_dns_aaaa_at(&server, name, ms) {
            return Ok(v);
        }
    }
    Err(format!("DNS AAAA lookup for `{}` failed", name))
}

fn jet_net_dns_a_at(server: &String, name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    Ok(jet_net_dns_query(server, name, 1, ms)?
        .into_iter()
        .filter(|r| r.len() == 4)
        .map(|r| JetIpAddr {
            inner: std::net::IpAddr::V4(std::net::Ipv4Addr::new(r[0], r[1], r[2], r[3])),
        })
        .collect())
}

fn jet_net_dns_aaaa_at(server: &String, name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    Ok(jet_net_dns_query(server, name, 28, ms)?
        .into_iter()
        .filter(|r| r.len() == 16)
        .map(|r| {
            let mut b = [0u8; 16];
            b.copy_from_slice(&r);
            JetIpAddr {
                inner: std::net::IpAddr::V6(std::net::Ipv6Addr::from(b)),
            }
        })
        .collect())
}

fn jet_net_dns_txt(name: &String, ms: i64) -> Result<Vec<String>, String> {
    for server in jet_net_dns_system_servers() {
        if let Ok(v) = jet_net_dns_txt_at(&server, name, ms) {
            return Ok(v);
        }
    }
    Err(format!("DNS TXT lookup for `{}` failed", name))
}

fn jet_net_dns_txt_at(server: &String, name: &String, ms: i64) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for r in jet_net_dns_query(server, name, 16, ms)? {
        let mut p = 0usize;
        let mut s = String::new();
        while p < r.len() {
            let len = r[p] as usize;
            p += 1;
            if p + len > r.len() {
                return Err("truncated DNS TXT record".to_string());
            }
            s.push_str(&String::from_utf8_lossy(&r[p..p + len]));
            p += len;
        }
        out.push(s);
    }
    Ok(out)
}

fn jet_net_dns_srv(name: &String, ms: i64) -> Result<Vec<JetDnsSrv>, String> {
    for server in jet_net_dns_system_servers() {
        if let Ok(v) = jet_net_dns_srv_at(&server, name, ms) {
            return Ok(v);
        }
    }
    Err(format!("DNS SRV lookup for `{}` failed", name))
}

fn jet_net_dns_srv_at(server: &String, name: &String, ms: i64) -> Result<Vec<JetDnsSrv>, String> {
    let packets = jet_net_dns_query(server, name, 33, ms)?;
    let mut out = Vec::new();
    for r in packets {
        if r.len() < 7 {
            return Err("truncated DNS SRV record".to_string());
        }
        let priority = u16::from_be_bytes([r[0], r[1]]) as i64;
        let weight = u16::from_be_bytes([r[2], r[3]]) as i64;
        let port = u16::from_be_bytes([r[4], r[5]]) as i64;
        let mut pos = 6usize;
        let target = jet_net_dns_read_name(&r, &mut pos)?;
        out.push(JetDnsSrv {
            priority,
            weight,
            port,
            target,
        });
    }
    Ok(out)
}

fn jet_net_dns_srv_target(srv: &JetDnsSrv) -> String {
    srv.target.clone()
}

fn jet_net_dns_srv_port(srv: &JetDnsSrv) -> i64 {
    srv.port
}

fn jet_net_dns_srv_priority(srv: &JetDnsSrv) -> i64 {
    srv.priority
}

fn jet_net_dns_srv_weight(srv: &JetDnsSrv) -> i64 {
    srv.weight
}

/// Send a well-formed HTTP/1.1 response on a TcpStream and close it.
/// Handles CRLF line endings internally so Jet code doesn't need `\r`.
fn jet_net_tcp_reply(mut stream: JetTcpStream, status: &String, body: &String) {
    use std::io::Write;
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{}",
        status, body.len(), body
    );
    let _ = stream.inner.write_all(response.as_bytes());
}

// ── HTTP/1.1 client (minimal, over std::net::TcpStream) ──────────────────────

fn jet_http_get(url: &String) -> Result<JetHttpResponse, String> {
    jet_http_request(url, "GET", &[], "")
}

fn jet_http_post(url: &String, body: &String) -> Result<JetHttpResponse, String> {
    jet_http_request(url, "POST", &[], body.as_str())
}

fn jet_http_request(
    url: &str,
    method: &str,
    extra_headers: &[(&str, &str)],
    body: &str,
) -> Result<JetHttpResponse, String> {
    use std::io::{Read, Write};
    // Parse URL: http://host[:port]/path
    let url_str = url;
    let (host_port, path) = if let Some(rest) = url_str.strip_prefix("http://") {
        let slash = rest.find('/').unwrap_or(rest.len());
        let hp = &rest[..slash];
        let p = if slash < rest.len() {
            &rest[slash..]
        } else {
            "/"
        };
        (hp.to_string(), p.to_string())
    } else if let Some(rest) = url_str.strip_prefix("https://") {
        return Err("HTTPS requires the `jet.tls` package; this is plain HTTP. Add `jet.tls` to your pkg.jet to enable HTTPS.".to_string());
        // Keep the variable to silence unused warning in case we extend later.
        #[allow(unreachable_code)]
        {
            (rest.to_string(), "/".to_string())
        }
    } else {
        return Err(format!("URL must start with http:// — got `{}`", url));
    };
    // Default port 80 if not specified.
    let addr = if host_port.contains(':') {
        host_port.clone()
    } else {
        format!("{}:80", host_port)
    };
    let host = host_port.split(':').next().unwrap_or(&host_port);
    let mut stream = std::net::TcpStream::connect(&addr)
        .map_err(|e| format!("connect to `{}` failed: {}", addr, e))?;
    // Build HTTP/1.1 request.
    let content_len = body.len();
    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Jet/1.0\r\nConnection: close\r\n",
        method, path, host
    );
    if !body.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", content_len));
    }
    for (k, v) in extra_headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");
    if !body.is_empty() {
        req.push_str(body);
    }
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("http write failed: {}", e))?;
    // Read response.
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("http read failed: {}", e))?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    // Parse status line + headers + body.
    let sep = text.find("\r\n\r\n").unwrap_or(text.len());
    let header_part = &text[..sep];
    let body_part = if sep + 4 <= text.len() {
        text[sep + 4..].to_string()
    } else {
        String::new()
    };
    let mut lines = header_part.lines();
    let status_line = lines.next().unwrap_or("HTTP/1.1 200 OK");
    let status = status_line
        .splitn(2, ' ')
        .nth(1)
        .unwrap_or("200 OK")
        .to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    Ok(JetHttpResponse {
        status,
        body: body_part,
        headers,
    })
}

// ── HTTP/1.1 server (blocking, one thread per connection) ────────────────────
// note: `jet serve` uses one task per connection. This is excellent for internal
//       services and tools at hundreds of concurrent connections. For very high
//       connection counts, Jet is not the right tool yet — see docs/services.md.

fn jet_http_serve<F>(addr: &String, handler: F)
where
    F: Fn(JetHttpRequest) -> JetHttpResponse + Send + Sync + 'static,
{
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str()).unwrap_or_else(|e| {
        eprintln!("E2801: bind on `{}` failed: {}", addr, e);
        std::process::exit(1);
    });
    let handler = std::sync::Arc::new(handler);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("E2801: accept failed: {}", e);
                continue;
            }
        };
        let h = handler.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let req = jet_http_parse_request(&raw);
            let resp = h(req);
            let response_text = jet_http_format_response(&resp);
            let _ = stream.write_all(response_text.as_bytes());
        });
    }
}

fn jet_http_parse_request(raw: &str) -> JetHttpRequest {
    let sep = raw.find("\r\n\r\n").unwrap_or(raw.len());
    let header_part = &raw[..sep];
    let body = if sep + 4 <= raw.len() {
        raw[sep + 4..].to_string()
    } else {
        String::new()
    };
    let mut lines = header_part.lines();
    let request_line = lines.next().unwrap_or("GET / HTTP/1.1");
    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    JetHttpRequest {
        method,
        path,
        body,
        headers,
        params: std::collections::BTreeMap::new(),
    }
}

fn jet_http_format_response(resp: &JetHttpResponse) -> String {
    let mut out = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        resp.status,
        resp.body.len()
    );
    for (k, v) in &resp.headers {
        out.push_str(&format!("{}: {}\r\n", k, v));
    }
    out.push_str("\r\n");
    out.push_str(&resp.body);
    out
}

// D-ROUTE1=A: router runtime ──────────────────────────────────────────────────

fn jet_http_router_new() -> JetHttpRouter {
    JetHttpRouter { routes: Vec::new() }
}

fn jet_http_router_parse_pattern(pattern: &str) -> Vec<RouteSegment> {
    pattern
        .split('/')
        .filter_map(|seg| {
            if seg.is_empty() {
                return None;
            }
            if let Some(name) = seg.strip_prefix(':') {
                Some(RouteSegment::Param(name.to_string()))
            } else {
                Some(RouteSegment::Static(seg.to_string()))
            }
        })
        .collect()
}

fn jet_http_router_register(
    router: &mut JetHttpRouter,
    method: String,
    pattern: String,
    handler: JetHttpHandler,
    file: &str,
    line: u32,
) {
    // E2804 (runtime): duplicate method+pattern fails at registration time in
    // Jet-owned runtime voice, not a raw Rust panic banner.
    let segs = jet_http_router_parse_pattern(&pattern);
    let is_dup = router.routes.iter().any(|r| {
        r.method == method
            && r.segments.len() == segs.len()
            && r.segments
                .iter()
                .zip(segs.iter())
                .all(|(a, b)| match (a, b) {
                    (RouteSegment::Static(x), RouteSegment::Static(y)) => x == y,
                    (RouteSegment::Param(_), RouteSegment::Param(_)) => true,
                    _ => false,
                })
    });
    if is_dup {
        jet_panic(
            file,
            line,
            &format!("E2804: duplicate route `{} {}`", method, pattern),
        );
    }
    router.routes.push(JetHttpRoute {
        method,
        segments: segs,
        handler,
    });
}

/// Count static segments in a route (for precedence: more statics win).
fn route_static_count(segs: &[RouteSegment]) -> usize {
    segs.iter()
        .filter(|s| matches!(s, RouteSegment::Static(_)))
        .count()
}

fn jet_http_router_dispatch(router: &JetHttpRouter, req: JetHttpRequest) -> JetHttpResponse {
    let path_segs: Vec<&str> = req.path.split('/').filter(|s| !s.is_empty()).collect();
    // Collect matching routes with their static count (for precedence).
    let mut candidates: Vec<(usize, usize)> = Vec::new(); // (route_idx, static_count)
    for (i, route) in router.routes.iter().enumerate() {
        if route.segments.len() != path_segs.len() {
            continue;
        }
        let mut ok = true;
        for (rseg, pseg) in route.segments.iter().zip(path_segs.iter()) {
            if let RouteSegment::Static(s) = rseg {
                if s != pseg {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            candidates.push((i, route_static_count(&route.segments)));
        }
    }
    if candidates.is_empty() {
        return JetHttpResponse {
            status: "404 Not Found".to_string(),
            body: "404 not found".to_string(),
            headers: std::collections::BTreeMap::new(),
        };
    }
    // Pick highest static-count match with the right method; otherwise 405.
    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    let method_match = candidates
        .iter()
        .find(|(i, _)| router.routes[*i].method == req.method);
    let Some((route_idx, _)) = method_match.copied() else {
        return JetHttpResponse {
            status: "405 Method Not Allowed".to_string(),
            body: "405 method not allowed".to_string(),
            headers: std::collections::BTreeMap::new(),
        };
    };
    let route = &router.routes[route_idx];
    let mut params = std::collections::BTreeMap::new();
    for (rseg, pseg) in route.segments.iter().zip(path_segs.iter()) {
        if let RouteSegment::Param(name) = rseg {
            params.insert(name.clone(), pseg.to_string());
        }
    }
    let mut req2 = req;
    req2.params = params;
    (route.handler)(req2)
}

fn jet_http_serve_router(addr: &String, router: JetHttpRouter) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str()).unwrap_or_else(|e| {
        eprintln!("E2801: bind on `{}` failed: {}", addr, e);
        std::process::exit(1);
    });
    let router = std::sync::Arc::new(router);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("E2801: accept failed: {}", e);
                continue;
            }
        };
        let r = router.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let req = jet_http_parse_request(&raw);
            let resp = jet_http_router_dispatch(&r, req);
            let response_text = jet_http_format_response(&resp);
            let _ = stream.write_all(response_text.as_bytes());
        });
    }
}

fn jet_http_request_param(req: &JetHttpRequest, name: &String) -> Option<String> {
    req.params.get(name.as_str()).cloned()
}

// ── D-HTTPLIB2=B / D-HTTPLIB4=B: core.http.client — request builder ─────────
// JetHttpClientReq and JetHttpClientResp live here (in the generated program's
// crate) so they're accessible without cross-crate type imports. The ureq
// bridge functions use only primitive types (i64, String, Vec<String>) and are
// called through wrappers here. This is the I6-safe pattern.

#[derive(Clone)]
struct JetHttpClientReq {
    method: String,
    url: String,
    headers: Vec<String>, // alternating key, value pairs
    body: Option<String>,
    timeout_ms: Option<i64>,
    connect_timeout_ms: Option<i64>,
    read_timeout_ms: Option<i64>,
    total_timeout_ms: Option<i64>,
    redirects: Option<i64>,
    proxy: Option<String>,
    cookies: Vec<String>,   // alternating name, value pairs
    form: Vec<String>,      // alternating name, value pairs
    multipart: Vec<String>, // alternating name, value pairs
}

#[derive(Clone)]
struct JetHttpClientResp {
    status: i64,
    body: String,
    headers: Vec<String>, // alternating key, value pairs
}

fn jet_http_client_request_new(method: &String, url: &String) -> JetHttpClientReq {
    JetHttpClientReq {
        method: method.clone(),
        url: url.clone(),
        headers: Vec::new(),
        body: None,
        timeout_ms: None,
        connect_timeout_ms: None,
        read_timeout_ms: None,
        total_timeout_ms: None,
        redirects: None,
        proxy: None,
        cookies: Vec::new(),
        form: Vec::new(),
        multipart: Vec::new(),
    }
}

fn jet_http_client_request_header(
    mut req: JetHttpClientReq,
    name: &String,
    value: &String,
) -> JetHttpClientReq {
    req.headers.push(name.clone());
    req.headers.push(value.clone());
    req
}

fn jet_http_client_request_body(mut req: JetHttpClientReq, body: &String) -> JetHttpClientReq {
    req.body = Some(body.clone());
    req
}

fn jet_http_client_request_timeout(mut req: JetHttpClientReq, ms: i64) -> JetHttpClientReq {
    req.timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_connect_timeout(
    mut req: JetHttpClientReq,
    ms: i64,
) -> JetHttpClientReq {
    req.connect_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_read_timeout(mut req: JetHttpClientReq, ms: i64) -> JetHttpClientReq {
    req.read_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_total_timeout(mut req: JetHttpClientReq, ms: i64) -> JetHttpClientReq {
    req.total_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_redirects(mut req: JetHttpClientReq, limit: i64) -> JetHttpClientReq {
    req.redirects = Some(limit);
    req
}

fn jet_http_client_request_proxy(mut req: JetHttpClientReq, proxy: &String) -> JetHttpClientReq {
    req.proxy = Some(proxy.clone());
    req
}

fn jet_http_client_request_cookie(
    mut req: JetHttpClientReq,
    name: &String,
    value: &String,
) -> JetHttpClientReq {
    req.cookies.push(name.clone());
    req.cookies.push(value.clone());
    req
}

fn jet_http_client_request_form(
    mut req: JetHttpClientReq,
    name: &String,
    value: &String,
) -> JetHttpClientReq {
    req.form.push(name.clone());
    req.form.push(value.clone());
    req
}

fn jet_http_client_request_multipart_text(
    mut req: JetHttpClientReq,
    name: &String,
    value: &String,
) -> JetHttpClientReq {
    req.multipart.push(name.clone());
    req.multipart.push(value.clone());
    req
}

fn jet_http_client_response_status(resp: &JetHttpClientResp) -> i64 {
    resp.status
}
fn jet_http_client_response_body(resp: &JetHttpClientResp) -> String {
    resp.body.clone()
}
fn jet_http_client_response_header(resp: &JetHttpClientResp, name: &String) -> Option<String> {
    let name_lc = name.to_lowercase();
    let mut i = 0;
    while i + 1 < resp.headers.len() {
        if resp.headers[i].to_lowercase() == name_lc {
            return Some(resp.headers[i + 1].clone());
        }
        i += 2;
    }
    None
}

fn jet_http_client_response_cookies(resp: &JetHttpClientResp) -> Vec<String> {
    resp.headers
        .chunks(2)
        .filter(|chunk| chunk.len() == 2 && chunk[0].to_lowercase() == "set-cookie")
        .map(|chunk| chunk[1].clone())
        .collect()
}

// ── D-HTTPLIB1=A / D-HTTPLIB2=B: core.http.server — function-first mux ───────
// Plain HTTP is pure std. D-TLSSERVE1=A routes server TLS through the hidden
// rustls bridge only when the named `tls:` option is used.

#[derive(Clone)]
struct JetHttpSrvResp {
    status: i64,
    body: String,
    headers: std::collections::BTreeMap<String, String>,
}

#[derive(Clone)]
struct JetHttpSrvReq {
    method: String,
    path: String,
    params: std::collections::BTreeMap<String, String>,
    body: String,
    headers: std::collections::BTreeMap<String, String>,
}

type JetHttpMuxHandlerFn = std::sync::Arc<dyn Fn(JetHttpSrvReq) -> JetHttpSrvResp + Send + Sync>;

struct JetHttpMuxRoute {
    method: String,
    pattern: String,
    handler: JetHttpMuxHandlerFn,
}

#[derive(Clone)]
struct JetHttpMux(std::sync::Arc<std::sync::Mutex<Vec<JetHttpMuxRoute>>>);

#[derive(Clone)]
struct JetHttpServerTls {
    cert_pem: String,
    key_pem: String,
}

impl JetHttpMux {
    fn new() -> Self {
        JetHttpMux(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())))
    }
    fn add<F>(&self, method: &str, pattern: &str, f: F)
    where
        F: Fn(JetHttpSrvReq) -> JetHttpSrvResp + Send + Sync + 'static,
    {
        self.0.lock().unwrap().push(JetHttpMuxRoute {
            method: method.to_uppercase(),
            pattern: pattern.to_string(),
            handler: std::sync::Arc::new(f) as JetHttpMuxHandlerFn,
        });
    }
}

fn jet_http_mux_new() -> JetHttpMux {
    JetHttpMux::new()
}

fn jet_http_srv_tls(cert_pem: &String, key_pem: &String) -> JetHttpServerTls {
    JetHttpServerTls {
        cert_pem: cert_pem.clone(),
        key_pem: key_pem.clone(),
    }
}

fn jet_http_mux_add<F>(mux: &JetHttpMux, method: &str, pattern: &str, f: F)
where
    F: Fn(JetHttpSrvReq) -> JetHttpSrvResp + Send + Sync + 'static,
{
    mux.add(method, pattern, f);
}

fn jet_http_srv_response(status: i64, body: &String) -> JetHttpSrvResp {
    JetHttpSrvResp {
        status,
        body: body.clone(),
        headers: std::collections::BTreeMap::new(),
    }
}

fn jet_http_srv_response_header(
    mut resp: JetHttpSrvResp,
    name: &String,
    value: &String,
) -> JetHttpSrvResp {
    resp.headers.insert(name.clone(), value.clone());
    resp
}

fn jet_http_mux_serve(addr: &String, mux: JetHttpMux) -> Result<(), String> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    let mux = std::sync::Arc::new(mux);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("http accept failed: {}", e);
                continue;
            }
        };
        let m = mux.clone();
        std::thread::spawn(move || {
            let raw = match jet_http_srv_read(&mut stream) {
                Ok(raw) => raw,
                Err(e) => {
                    eprintln!("http read failed: {}", e);
                    return;
                }
            };
            let req = jet_http_srv_parse(&raw);
            let resp = jet_http_mux_dispatch(&m, req);
            let text = jet_http_srv_format(&resp);
            let _ = stream.write_all(text.as_bytes());
        });
    }
}

fn jet_http_mux_serve_once(addr: &String, mux: JetHttpMux) -> Result<(), String> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    jet_http_mux_serve_once_listener(&JetTcpListener { inner: listener }, &mux)
}

fn jet_http_mux_serve_once_listener(
    listener: &JetTcpListener,
    mux: &JetHttpMux,
) -> Result<(), String> {
    use std::io::{Read, Write};
    let (mut stream, _peer) = listener
        .inner
        .accept()
        .map_err(|e| format!("accept failed: {}", e))?;
    let raw = jet_http_srv_read(&mut stream)?;
    let req = jet_http_srv_parse(&raw);
    let resp = jet_http_mux_dispatch(mux, req);
    let text = jet_http_srv_format(&resp);
    stream
        .write_all(text.as_bytes())
        .map_err(|e| format!("http write failed: {}", e))
}

fn jet_http_srv_read(stream: &mut std::net::TcpStream) -> Result<String, String> {
    use std::io::Read;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream
            .read(&mut buf)
            .map_err(|e| format!("http read failed: {}", e))?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if let Some(header_end) = jet_http_header_end(&raw) {
            let body_start = header_end + 4;
            let content_len = jet_http_content_length(&raw[..header_end]);
            if raw.len().saturating_sub(body_start) >= content_len {
                break;
            }
        }
    }
    Ok(String::from_utf8_lossy(&raw).into_owned())
}

fn jet_http_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn jet_http_content_length(header: &[u8]) -> usize {
    let text = String::from_utf8_lossy(header);
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                return v.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }
    0
}

fn jet_http_mux_serve_tls<V, H>(
    addr: &String,
    mux: JetHttpMux,
    tls: JetHttpServerTls,
    validate: V,
    handle: H,
) -> Result<(), String>
where
    V: Fn(&String, &String) -> Result<(), String>,
    H: Fn(
            &String,
            &String,
            std::net::TcpStream,
            Box<dyn FnOnce(String) -> String + Send>,
        ) -> Result<(), String>
        + Clone
        + Send
        + Sync
        + 'static,
{
    validate(&tls.cert_pem, &tls.key_pem)?;
    let listener = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))?;
    let mux = std::sync::Arc::new(mux);
    loop {
        let (stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("http TLS accept failed: {}", e);
                continue;
            }
        };
        let m = mux.clone();
        let tls_cfg = tls.clone();
        let handle_one = handle.clone();
        std::thread::spawn(move || {
            let dispatch = Box::new(move |raw: String| {
                let req = jet_http_srv_parse(&raw);
                let resp = jet_http_mux_dispatch(&m, req);
                jet_http_srv_format(&resp)
            });
            if let Err(e) = handle_one(&tls_cfg.cert_pem, &tls_cfg.key_pem, stream, dispatch) {
                eprintln!("http TLS connection failed: {}", e);
            }
        });
    }
}

fn jet_http_srv_parse(raw: &str) -> JetHttpSrvReq {
    let sep = raw.find("\r\n\r\n").unwrap_or(raw.len());
    let header_part = &raw[..sep];
    let body = if sep + 4 <= raw.len() {
        raw[sep + 4..].to_string()
    } else {
        String::new()
    };
    let mut lines = header_part.lines();
    let req_line = lines.next().unwrap_or("GET / HTTP/1.1");
    let mut parts = req_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    JetHttpSrvReq {
        method,
        path,
        params: std::collections::BTreeMap::new(),
        body,
        headers,
    }
}

fn jet_http_mux_dispatch(mux: &JetHttpMux, req: JetHttpSrvReq) -> JetHttpSrvResp {
    let routes = mux.0.lock().unwrap();
    for route in routes.iter() {
        if route.method != req.method {
            continue;
        }
        if let Some(params) = jet_http_match_path(&route.pattern, &req.path) {
            let mut r2 = req.clone();
            r2.params = params;
            return (route.handler)(r2);
        }
    }
    JetHttpSrvResp {
        status: 404,
        body: "404 Not Found".to_string(),
        headers: std::collections::BTreeMap::new(),
    }
}

fn jet_http_match_path(
    pattern: &str,
    path: &str,
) -> Option<std::collections::BTreeMap<String, String>> {
    let p_segs: Vec<&str> = pattern.split('/').collect();
    let r_segs: Vec<&str> = path.split('?').next().unwrap_or(path).split('/').collect();
    if p_segs.last() == Some(&"*") && r_segs.len() >= p_segs.len() {
        let mut params = std::collections::BTreeMap::new();
        for (p, r) in p_segs[..p_segs.len() - 1]
            .iter()
            .zip(r_segs[..p_segs.len() - 1].iter())
        {
            if let Some(key) = p.strip_prefix(':') {
                params.insert(key.to_string(), r.to_string());
            } else if *p != *r {
                return None;
            }
        }
        params.insert("wildcard".to_string(), r_segs[p_segs.len() - 1..].join("/"));
        return Some(params);
    }
    if p_segs.len() != r_segs.len() {
        return None;
    }
    let mut params = std::collections::BTreeMap::new();
    for (p, r) in p_segs.iter().zip(r_segs.iter()) {
        if let Some(key) = p.strip_prefix(':') {
            params.insert(key.to_string(), r.to_string());
        } else if *p != *r {
            return None;
        }
    }
    Some(params)
}

fn jet_http_srv_format(resp: &JetHttpSrvResp) -> String {
    let reason = match resp.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        resp.status,
        reason,
        resp.body.len()
    );
    for (k, v) in &resp.headers {
        out.push_str(&format!("{}: {}\r\n", k, v));
    }
    out.push_str("\r\n");
    out.push_str(&resp.body);
    out
}

fn jet_http_srv_req_method(req: &JetHttpSrvReq) -> String {
    req.method.clone()
}
fn jet_http_srv_req_path(req: &JetHttpSrvReq) -> String {
    req.path.clone()
}
fn jet_http_srv_req_param(req: &JetHttpSrvReq, name: &String) -> Option<String> {
    req.params.get(name).cloned()
}
fn jet_http_srv_req_body(req: &JetHttpSrvReq) -> String {
    req.body.clone()
}
fn jet_http_srv_req_header(req: &JetHttpSrvReq, name: &String) -> Option<String> {
    req.headers.get(&name.to_lowercase()).cloned()
}

fn jet_http_srv_req_body_len(req: &JetHttpSrvReq) -> i64 {
    req.body.len() as i64
}

fn jet_http_srv_req_under_limit(req: &JetHttpSrvReq, max_bytes: i64) -> bool {
    max_bytes >= 0 && req.body.len() as i64 <= max_bytes
}

fn jet_http_srv_sse(data: &String) -> JetHttpSrvResp {
    let resp = jet_http_srv_response(200, &format!("data: {}\n\n", data));
    let resp = jet_http_srv_response_header(
        resp,
        &"content-type".to_string(),
        &"text/event-stream".to_string(),
    );
    jet_http_srv_response_header(resp, &"cache-control".to_string(), &"no-cache".to_string())
}

fn jet_http_srv_static_file(path: &String, mime: &String) -> Result<JetHttpSrvResp, String> {
    std::fs::read_to_string(path)
        .map(|body| {
            jet_http_srv_response_header(
                jet_http_srv_response(200, &body),
                &"content-type".to_string(),
                mime,
            )
        })
        .map_err(|e| format!("static file `{}` failed: {}", path, e))
}

fn jet_http_srv_static_file_range(
    req: &JetHttpSrvReq,
    path: &String,
    mime: &String,
) -> Result<JetHttpSrvResp, String> {
    let body =
        std::fs::read_to_string(path).map_err(|e| format!("static file `{}` failed: {}", path, e))?;
    let Some(range) = jet_http_srv_req_header(req, &"range".to_string()) else {
        return Ok(jet_http_srv_response_header(
            jet_http_srv_response(200, &body),
            &"content-type".to_string(),
            mime,
        ));
    };
    let Some(spec) = range.strip_prefix("bytes=") else {
        return Ok(jet_http_srv_response(416, &"range not satisfiable".to_string()));
    };
    let (start_s, end_s) = spec.split_once('-').unwrap_or((spec, ""));
    let start = start_s.parse::<usize>().unwrap_or(0);
    let end = if end_s.is_empty() {
        body.len().saturating_sub(1)
    } else {
        end_s.parse::<usize>().unwrap_or(body.len().saturating_sub(1))
    };
    if start >= body.len() || end < start {
        return Ok(jet_http_srv_response(416, &"range not satisfiable".to_string()));
    }
    let capped = std::cmp::min(end + 1, body.len());
    let part = body[start..capped].to_string();
    let resp = jet_http_srv_response_header(
        jet_http_srv_response(206, &part),
        &"content-type".to_string(),
        mime,
    );
    Ok(jet_http_srv_response_header(
        resp,
        &"content-range".to_string(),
        &format!("bytes {}-{}/{}", start, capped - 1, body.len()),
    ))
}

fn jet_http_srv_access_log(req: &JetHttpSrvReq, status: i64) -> String {
    format!("{} {} {}", req.method, req.path, status)
}

// ── D-ARGS1: declarative CLI arg parsing (ratified 2026-06-22) ───────────────
// The builder accumulates a spec; `jet_args_parse` runs it against an argv
// list, producing `ParsedArgs` or an error string (never exits — the caller
// decides what to print and how to exit, which keeps the API testable).
//
// Design: builder methods take the spec BY VALUE and return a new one —
// ownership-safe, no aliasing, works with both immutable (::) and mutable
// (:=) bindings in Jet. The parse result is cloneable.
//
// `--help` is recognized but NOT parsed out of argv here; the caller tests
// `parsed.flag("help")` if they want to handle it. The auto-generated help
// text is available via `spec.help()` and `spec.help_auto()`.

/// A single entry in the spec.
#[derive(Clone)]
enum JetArgKind {
    /// Boolean flag: `--name` sets it to true.
    Flag {
        name: String,
        short: Option<String>,
        help: String,
    },
    /// Value option: `--name VALUE` captures VALUE.
    Option {
        name: String,
        short: Option<String>,
        help: String,
        meta: String,
        default: Option<String>,
        env: Option<String>,
        required: bool,
        repeat: bool,
        value: JetArgValueKind,
    },
    /// Positional argument (in declaration order).
    Positional { name: String, help: String },
    /// Subcommand with its own nested spec.
    Subcommand {
        name: String,
        help: String,
        spec: Box<JetArgsSpec>,
    },
}

#[derive(Clone)]
enum JetArgValueKind {
    String,
    Int,
    Float,
    Choice(Vec<String>),
}

/// The builder. All methods consume self and return a new spec (builder pattern).
#[derive(Clone)]
struct JetArgsSpec {
    entries: Vec<JetArgKind>,
    prog: String,
    version: Option<String>,
}

/// The parse result.
#[derive(Clone)]
struct JetParsedArgs {
    flags: std::collections::HashMap<String, bool>,
    options: std::collections::HashMap<String, Vec<String>>,
    positionals: Vec<String>,
    subcommand: Option<String>,
}

impl JetArgsSpec {
    /// Render the generated --help text.
    fn help(&self) -> String {
        let mut s = String::new();
        // usage line
        let prog = if self.prog.is_empty() {
            "program".to_string()
        } else {
            self.prog.clone()
        };
        let has_opts = self.entries.iter().any(|e| {
            matches!(
                e,
                JetArgKind::Flag { .. } | JetArgKind::Option { .. } | JetArgKind::Subcommand { .. }
            )
        }) || self.version.is_some();
        let positionals: Vec<&JetArgKind> = self
            .entries
            .iter()
            .filter(|e| matches!(e, JetArgKind::Positional { .. }))
            .collect();
        s.push_str("Usage: ");
        s.push_str(&prog);
        if has_opts {
            s.push_str(" [options]");
        }
        for p in &positionals {
            if let JetArgKind::Positional { name, .. } = p {
                s.push(' ');
                s.push_str(name);
            }
        }
        s.push('\n');
        // flags and options
        let flags_opts: Vec<&JetArgKind> = self
            .entries
            .iter()
            .filter(|e| !matches!(e, JetArgKind::Positional { .. }))
            .collect();
        if !flags_opts.is_empty() {
            s.push('\n');
            s.push_str("Options:\n");
            for e in flags_opts {
                match e {
                    JetArgKind::Flag { name, short, help } => {
                        s.push_str(&format!("  {:<24} {}\n", jet_args_label(name, short, None), help));
                    }
                    JetArgKind::Option {
                        name,
                        short,
                        help,
                        meta,
                        default,
                        env,
                        required,
                        repeat,
                        value,
                    } => {
                        let mut note = help.clone();
                        if *required {
                            note.push_str(" (required)");
                        }
                        if *repeat {
                            note.push_str(" (repeatable)");
                        }
                        if let Some(d) = default {
                            note.push_str(&format!(" [default: {}]", d));
                        }
                        if let Some(e) = env {
                            note.push_str(&format!(" [env: {}]", e));
                        }
                        if let JetArgValueKind::Choice(choices) = value {
                            note.push_str(&format!(" [choices: {}]", choices.join(", ")));
                        }
                        s.push_str(&format!(
                            "  {:<24} {}\n",
                            jet_args_label(name, short, Some(meta)),
                            note
                        ));
                    }
                    JetArgKind::Subcommand { name, help, .. } => {
                        s.push_str(&format!("  {:<24} {}\n", name, help));
                    }
                    _ => {}
                }
            }
            s.push_str(&format!("  {:<24} {}\n", "--help", "show this help"));
            if self.version.is_some() {
                s.push_str(&format!("  {:<24} {}\n", "--version", "show version"));
            }
        }
        // positionals
        if !positionals.is_empty() {
            s.push('\n');
            s.push_str("Arguments:\n");
            for p in positionals {
                if let JetArgKind::Positional { name, help } = p {
                    s.push_str(&format!("  {:<22} {}\n", name, help));
                }
            }
        }
        s
    }
}

fn jet_args_label(name: &String, short: &Option<String>, meta: Option<&String>) -> String {
    let mut out = String::new();
    if let Some(s) = short {
        out.push('-');
        out.push_str(s);
        out.push_str(", ");
    }
    out.push_str("--");
    out.push_str(name);
    if let Some(m) = meta {
        out.push(' ');
        out.push_str(m);
    }
    out
}

fn jet_args_spec() -> JetArgsSpec {
    // argv[0] is the program name — capture it from env at spec-creation time.
    let prog = std::env::args().next().unwrap_or_default();
    JetArgsSpec {
        entries: Vec::new(),
        prog,
        version: None,
    }
}

fn jet_args_flag(mut spec: JetArgsSpec, name: &String, help: &String) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Flag {
        name: name.clone(),
        short: None,
        help: help.clone(),
    });
    spec
}

fn jet_args_flag_short(
    mut spec: JetArgsSpec,
    name: &String,
    short: &String,
    help: &String,
) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Flag {
        name: name.clone(),
        short: Some(short.clone()),
        help: help.clone(),
    });
    spec
}

fn jet_args_option(
    mut spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Option {
        name: name.clone(),
        short: None,
        help: help.clone(),
        meta: meta.clone(),
        default: None,
        env: None,
        required: false,
        repeat: false,
        value: JetArgValueKind::String,
    });
    spec
}

fn jet_args_option_base(
    mut spec: JetArgsSpec,
    name: &String,
    short: Option<String>,
    help: &String,
    meta: &String,
    default: Option<String>,
    env: Option<String>,
    required: bool,
    repeat: bool,
    value: JetArgValueKind,
) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Option {
        name: name.clone(),
        short,
        help: help.clone(),
        meta: meta.clone(),
        default,
        env,
        required,
        repeat,
        value,
    });
    spec
}

fn jet_args_option_short(
    spec: JetArgsSpec,
    name: &String,
    short: &String,
    help: &String,
    meta: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        Some(short.clone()),
        help,
        meta,
        None,
        None,
        false,
        false,
        JetArgValueKind::String,
    )
}

fn jet_args_option_default(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
    default: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        Some(default.clone()),
        None,
        false,
        false,
        JetArgValueKind::String,
    )
}

fn jet_args_option_env(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
    env: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        Some(env.clone()),
        false,
        false,
        JetArgValueKind::String,
    )
}

fn jet_args_option_int(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        None,
        false,
        false,
        JetArgValueKind::Int,
    )
}

fn jet_args_option_float(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        None,
        false,
        false,
        JetArgValueKind::Float,
    )
}

fn jet_args_option_choice(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
    choices: &String,
) -> JetArgsSpec {
    let values = choices
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        None,
        false,
        false,
        JetArgValueKind::Choice(values),
    )
}

fn jet_args_repeat(spec: JetArgsSpec, name: &String, help: &String, meta: &String) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        None,
        false,
        true,
        JetArgValueKind::String,
    )
}

fn jet_args_required_option(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        None,
        true,
        false,
        JetArgValueKind::String,
    )
}

fn jet_args_positional(mut spec: JetArgsSpec, name: &String, help: &String) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Positional {
        name: name.clone(),
        help: help.clone(),
    });
    spec
}

fn jet_args_subcommand(
    mut spec: JetArgsSpec,
    name: &String,
    help: &String,
    sub: JetArgsSpec,
) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Subcommand {
        name: name.clone(),
        help: help.clone(),
        spec: Box::new(sub),
    });
    spec
}

fn jet_args_version(mut spec: JetArgsSpec, version: &String) -> JetArgsSpec {
    spec.version = Some(version.clone());
    spec
}

fn jet_args_completion(spec: &JetArgsSpec, shell: &String) -> String {
    let mut words = vec!["--help".to_string()];
    if spec.version.is_some() {
        words.push("--version".to_string());
    }
    for e in &spec.entries {
        match e {
            JetArgKind::Flag { name, short, .. }
            | JetArgKind::Option { name, short, .. } => {
                words.push(format!("--{}", name));
                if let Some(s) = short {
                    words.push(format!("-{}", s));
                }
            }
            JetArgKind::Subcommand { name, .. } => words.push(name.clone()),
            JetArgKind::Positional { .. } => {}
        }
    }
    format!("{} completion: {}", shell, words.join(" "))
}

/// Parse argv against the spec. Returns `Err(message)` on unknown flags/options
/// or missing required positionals. `argv[0]` (the program name) is skipped.
fn jet_args_parse(spec: &JetArgsSpec, argv: &Vec<String>) -> Result<JetParsedArgs, String> {
    let mut flags: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let mut options: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut positionals: Vec<String> = Vec::new();
    let mut subcommand: Option<String> = None;

    // Seed all flags as false (so .flag("name") returns false when absent).
    flags.insert("help".to_string(), false);
    flags.insert("version".to_string(), false);
    for e in &spec.entries {
        match e {
            JetArgKind::Flag { name, .. } => {
                flags.insert(name.clone(), false);
            }
            JetArgKind::Option { name, default, env, .. } => {
                if let Some(v) = default.clone().or_else(|| env.as_ref().and_then(|k| std::env::var(k).ok())) {
                    options.insert(name.clone(), vec![v]);
                }
            }
            _ => {}
        }
    }

    let mut i = 1usize; // skip argv[0]
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--" {
            i += 1;
            // Everything after `--` is positional.
            while i < argv.len() {
                positionals.push(argv[i].clone());
                i += 1;
            }
            break;
        }
        if let Some(rest) = arg.strip_prefix("--") {
            if rest == "help" {
                flags.insert("help".to_string(), true);
                i += 1;
                continue;
            }
            if rest == "version" && spec.version.is_some() {
                flags.insert("version".to_string(), true);
                i += 1;
                continue;
            }
            // Try `--name=value` form.
            if let Some(eq) = rest.find('=') {
                let name = &rest[..eq];
                let val = &rest[eq + 1..];
                if let Some(entry) = jet_args_find_option(spec, name) {
                    jet_args_store_option(&mut options, entry, val)?;
                } else if jet_args_find_flag(spec, name).is_some() {
                    return Err(format!(
                        "--{} is a flag; it takes no value (got `={}`)\n\n{}",
                        name,
                        val,
                        spec.help()
                    ));
                } else {
                    return Err(jet_args_unknown(name, spec));
                }
            } else if jet_args_find_flag(spec, rest).is_some() {
                flags.insert(rest.to_string(), true);
            } else if let Some(entry) = jet_args_find_option(spec, rest) {
                i += 1;
                if i >= argv.len() {
                    return Err(format!("`--{}` requires a value\n\n{}", rest, spec.help()));
                }
                jet_args_store_option(&mut options, entry, &argv[i])?;
            } else {
                return Err(jet_args_unknown(rest, spec));
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            let rest = &arg[1..];
            if rest.len() > 1 {
                let mut chars = rest.chars().peekable();
                while let Some(ch) = chars.next() {
                    let short = ch.to_string();
                    if let Some((name, is_option)) = jet_args_find_short(spec, &short) {
                        if is_option {
                            let value: String = chars.collect();
                            if value.is_empty() {
                                i += 1;
                                if i >= argv.len() {
                                    return Err(format!("`-{}` requires a value\n\n{}", short, spec.help()));
                                }
                                let entry = jet_args_find_option(spec, &name).unwrap();
                                jet_args_store_option(&mut options, entry, &argv[i])?;
                            } else {
                                let entry = jet_args_find_option(spec, &name).unwrap();
                                jet_args_store_option(&mut options, entry, &value)?;
                            }
                            break;
                        }
                        flags.insert(name, true);
                    } else {
                        return Err(format!("unknown option `-{}`\n\n{}", short, spec.help()));
                    }
                }
            } else if let Some((name, is_option)) = jet_args_find_short(spec, rest) {
                if is_option {
                    i += 1;
                    if i >= argv.len() {
                        return Err(format!("`-{}` requires a value\n\n{}", rest, spec.help()));
                    }
                    let entry = jet_args_find_option(spec, &name).unwrap();
                    jet_args_store_option(&mut options, entry, &argv[i])?;
                } else {
                    flags.insert(name, true);
                }
            } else {
                return Err(format!("unknown option `-{}`\n\n{}", rest, spec.help()));
            }
        } else {
            if subcommand.is_none() {
                if let Some((name, nested)) = jet_args_find_subcommand(spec, arg) {
                    let mut nested_argv = vec![format!("{} {}", spec.prog, name)];
                    nested_argv.extend(argv.iter().skip(i + 1).cloned());
                    let parsed = jet_args_parse(nested, &nested_argv)?;
                    flags.extend(parsed.flags);
                    options.extend(parsed.options);
                    positionals.extend(parsed.positionals);
                    subcommand = Some(name.to_string());
                    break;
                }
            }
            positionals.push(arg.clone());
        }
        i += 1;
    }

    // Check required positionals.
    let required_count = spec
        .entries
        .iter()
        .filter(|e| matches!(e, JetArgKind::Positional { .. }))
        .count();
    if positionals.len() < required_count {
        let missing: Vec<&str> = spec
            .entries
            .iter()
            .filter_map(|e| {
                if let JetArgKind::Positional { name, .. } = e {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .skip(positionals.len())
            .collect();
        return Err(format!(
            "missing required argument{}: {}\n\n{}",
            if missing.len() == 1 { "" } else { "s" },
            missing.join(", "),
            spec.help()
        ));
    }
    for e in &spec.entries {
        if let JetArgKind::Option { name, required, .. } = e {
            if *required && !options.contains_key(name) {
                return Err(format!("missing required option `--{}`\n\n{}", name, spec.help()));
            }
        }
    }

    Ok(JetParsedArgs {
        flags,
        options,
        positionals,
        subcommand,
    })
}

fn jet_args_find_flag<'a>(spec: &'a JetArgsSpec, name: &str) -> Option<&'a JetArgKind> {
    spec.entries.iter().find(|e| matches!(e, JetArgKind::Flag { name: n, .. } if n == name))
}

fn jet_args_find_option<'a>(spec: &'a JetArgsSpec, name: &str) -> Option<&'a JetArgKind> {
    spec.entries.iter().find(|e| matches!(e, JetArgKind::Option { name: n, .. } if n == name))
}

fn jet_args_find_short(spec: &JetArgsSpec, short: &str) -> Option<(String, bool)> {
    for e in &spec.entries {
        match e {
            JetArgKind::Flag { name, short: Some(s), .. } if s == short => {
                return Some((name.clone(), false));
            }
            JetArgKind::Option { name, short: Some(s), .. } if s == short => {
                return Some((name.clone(), true));
            }
            _ => {}
        }
    }
    None
}

fn jet_args_find_subcommand<'a>(spec: &'a JetArgsSpec, name: &str) -> Option<(&'a str, &'a JetArgsSpec)> {
    spec.entries.iter().find_map(|e| {
        if let JetArgKind::Subcommand { name: n, spec, .. } = e {
            (n == name).then_some((n.as_str(), spec.as_ref()))
        } else {
            None
        }
    })
}

fn jet_args_store_option(
    options: &mut std::collections::HashMap<String, Vec<String>>,
    entry: &JetArgKind,
    value: &str,
) -> Result<(), String> {
    if let JetArgKind::Option { name, repeat, value: kind, .. } = entry {
        match kind {
            JetArgValueKind::String => {}
            JetArgValueKind::Int => {
                value.parse::<i64>().map_err(|_| format!("`--{}` expects an Int, got `{}`", name, value))?;
            }
            JetArgValueKind::Float => {
                value.parse::<f64>().map_err(|_| format!("`--{}` expects a Float, got `{}`", name, value))?;
            }
            JetArgValueKind::Choice(choices) => {
                if !choices.iter().any(|c| c == value) {
                    return Err(format!(
                        "`--{}` expects one of: {}; got `{}`",
                        name,
                        choices.join(", "),
                        value
                    ));
                }
            }
        }
        if *repeat {
            options.entry(name.clone()).or_default().push(value.to_string());
        } else {
            options.insert(name.clone(), vec![value.to_string()]);
        }
    }
    Ok(())
}

fn jet_args_unknown(name: &str, spec: &JetArgsSpec) -> String {
    let known: Vec<String> = spec.entries.iter().filter_map(|e| match e {
        JetArgKind::Flag { name, .. } | JetArgKind::Option { name, .. } => Some(name.clone()),
        _ => None,
    }).collect();
    let suggestion = known
        .iter()
        .find(|k| jet_args_edit_distance(k, name) <= 2)
        .map(|k| format!("\ndid you mean `--{}`?", k))
        .unwrap_or_default();
    format!("unknown option `--{}`{}\n\n{}", name, suggestion, spec.help())
}

fn jet_args_edit_distance(a: &str, b: &str) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            cur[j + 1] = if ca == cb {
                prev[j]
            } else {
                1 + prev[j].min(prev[j + 1]).min(cur[j])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn jet_parsed_flag(parsed: &JetParsedArgs, name: &String) -> bool {
    *parsed.flags.get(name.as_str()).unwrap_or(&false)
}

fn jet_parsed_option(parsed: &JetParsedArgs, name: &String) -> Option<String> {
    parsed.options.get(name.as_str()).and_then(|v| v.last().cloned())
}

fn jet_parsed_option_int(parsed: &JetParsedArgs, name: &String) -> Option<i64> {
    jet_parsed_option(parsed, name).and_then(|v| v.parse::<i64>().ok())
}

fn jet_parsed_option_float(parsed: &JetParsedArgs, name: &String) -> Option<f64> {
    jet_parsed_option(parsed, name).and_then(|v| v.parse::<f64>().ok())
}

fn jet_parsed_options(parsed: &JetParsedArgs, name: &String) -> Vec<String> {
    parsed.options.get(name.as_str()).cloned().unwrap_or_default()
}

fn jet_parsed_positional(parsed: &JetParsedArgs, idx: i64) -> Option<String> {
    if idx < 0 {
        return None;
    }
    parsed.positionals.get(idx as usize).cloned()
}

fn jet_parsed_subcommand(parsed: &JetParsedArgs) -> Option<String> {
    parsed.subcommand.clone()
}

impl JetShow for JetArgsSpec {
    fn jet_show(&self) -> String {
        format!("ArgsSpec({})", self.entries.len())
    }
}
impl JetShow for JetParsedArgs {
    fn jet_show(&self) -> String {
        format!(
            "ParsedArgs(flags={}, options={}, positionals={})",
            self.flags.len(),
            self.options.len(),
            self.positionals.len()
        )
    }
}

// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)` — the runtime reflection floor.
// `JetReflectValue` is the whole-value handle (`type_name`/`display` always
// populated; `fields` non-empty only when the reflected value was a known
// user struct — built entirely at the call site, `Codegen/TIR/emit.rs`
// `("core.reflect", "of")`). `JetReflectField` is one struct field's name
// and its `.jet_show()`-rendered value. Both are plain data — no runtime
// type registry, no raw-pointer/audited-region casting of any kind (I1):
// everything here is a string captured at compile time from the call
// site's already-known static type.

#[derive(Clone)]
struct JetReflectValue {
    type_name: String,
    display: String,
    fields: Vec<JetReflectField>,
}

#[derive(Clone)]
struct JetReflectField {
    name: String,
    value: String,
}

impl JetReflectValue {
    fn type_name(&self) -> String {
        self.type_name.clone()
    }
    fn display(&self) -> String {
        self.display.clone()
    }
    fn fields(&self) -> Vec<JetReflectField> {
        self.fields.clone()
    }
}

impl JetReflectField {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn value(&self) -> String {
        self.value.clone()
    }
}

impl JetShow for JetReflectValue {
    fn jet_show(&self) -> String {
        format!("Value({})", self.type_name)
    }
}
impl JetShow for JetReflectField {
    fn jet_show(&self) -> String {
        format!("Field({}: {})", self.name, self.value)
    }
}
