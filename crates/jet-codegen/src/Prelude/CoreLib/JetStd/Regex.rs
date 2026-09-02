#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegexFlags {
    pub case_insensitive: bool,
    pub multiline: bool,
    pub dotall: bool,
}

#[derive(Clone, Debug)]
pub struct JetRegex {
    pattern: std::sync::Arc<str>,
    flags: RegexFlags,
    program: std::sync::Arc<RegexProgram>,
    group_names: std::sync::Arc<[Option<String>]>,
    groups: usize,
}

#[derive(Debug)]
pub struct JetRegexMatch {
    text: std::sync::Arc<str>,
    span: (usize, usize),
    program: std::sync::Arc<RegexProgram>,
    flags: RegexFlags,
    groups: usize,
    names: std::sync::Arc<[Option<String>]>,
    capture_cache: std::sync::OnceLock<Vec<Option<(usize, usize)>>>,
}

#[derive(Debug)]
struct RegexProgram {
    insts: Vec<RegexInst>,
    start: usize,
    anchored_start: bool,
    literal: Option<Vec<u8>>,
    required_literal: Option<Vec<u8>>,
    // ponytail: one per-regex scratch lock; split per-worker scratch if concurrent matching contends.
    scratch: std::sync::Mutex<RegexScratch>,
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
    ascii: [u64; 2],
}
impl RegexClass {
    fn new(negated: bool, items: Vec<RegexClassItem>) -> Self {
        let mut ascii = [0u64; 2];
        for cp in 0u8..=127 {
            if items.iter().any(|item| regex_class_item_matches(item, cp as char, false)) {
                ascii[(cp >> 6) as usize] |= 1u64 << (cp & 63);
            }
        }
        Self {
            negated,
            items,
            ascii,
        }
    }

    fn matches_ascii(&self, ch: u8) -> bool {
        let yes = self.ascii[(ch >> 6) as usize] & (1u64 << (ch & 63)) != 0;
        if self.negated { !yes } else { yes }
    }
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

#[derive(Debug)]
struct RegexCaptureNode {
    slot: usize,
    pos: usize,
    previous: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct RegexThread {
    pc: usize,
    start: usize,
    caps: Option<usize>,
}

#[derive(Debug)]
struct RegexState {
    threads: Vec<RegexThread>,
    seen: Vec<u32>,
    stack: Vec<RegexThread>,
    epoch: u32,
    min_start: Option<usize>,
    matched: Option<RegexThread>,
}

impl RegexState {
    fn new(inst_count: usize) -> Self {
        Self {
            threads: Vec::with_capacity(inst_count),
            seen: vec![0; inst_count],
            stack: Vec::with_capacity(inst_count),
            epoch: 0,
            min_start: None,
            matched: None,
        }
    }

    fn clear(&mut self) {
        self.threads.clear();
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.seen.fill(0);
            self.epoch = 1;
        }
        self.min_start = None;
        self.matched = None;
    }
}
const REGEX_CACHE_LIMIT: usize = 32;

#[derive(Debug)]
struct RegexCache {
    // Eight flag combinations keep lookups borrowed (`&str`) and avoid
    // allocating a composite key on every compile call.
    entries: [std::collections::HashMap<std::sync::Arc<str>, JetRegex>; 8],
    order: std::collections::VecDeque<(usize, std::sync::Arc<str>)>,
}

impl RegexCache {
    fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| std::collections::HashMap::new()),
            order: std::collections::VecDeque::new(),
        }
    }

    fn bucket(flags: &RegexFlags) -> usize {
        usize::from(flags.case_insensitive)
            | (usize::from(flags.multiline) << 1)
            | (usize::from(flags.dotall) << 2)
    }

    fn get(&self, pattern: &str, flags: &RegexFlags) -> Option<JetRegex> {
        self.entries[Self::bucket(flags)].get(pattern).cloned()
    }

    fn insert(&mut self, regex: JetRegex) {
        let bucket = Self::bucket(&regex.flags);
        let pattern = std::sync::Arc::clone(&regex.pattern);
        if self.entries[bucket].contains_key(pattern.as_ref()) {
            return;
        }
        self.entries[bucket].insert(std::sync::Arc::clone(&pattern), regex);
        self.order.push_back((bucket, pattern));
        if self.order.len() > REGEX_CACHE_LIMIT {
            if let Some((old_bucket, old_pattern)) = self.order.pop_front() {
                self.entries[old_bucket].remove(old_pattern.as_ref());
            }
        }
    }
}

// Cache key is the exact pattern plus every compilation flag. Split limits
// are execution arguments, not compiled-regex state, so they do not enter it.
static REGEX_CACHE: std::sync::LazyLock<std::sync::Mutex<RegexCache>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(RegexCache::new()));

fn regex_cache() -> &'static std::sync::Mutex<RegexCache> {
    &REGEX_CACHE
}


#[derive(Debug)]
struct RegexScratch {
    current: RegexState,
    next: RegexState,
    capture_arena: Vec<RegexCaptureNode>,
}


fn regex_outcome<T>(value: Option<T>) -> JetOutcome<T, JetAbsent> {
    value.ok_or(JetAbsent)
}

fn regex_char_index(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
}

fn regex_simple_fold(cp: u32) -> u32 {
    char::from_u32(cp)
        .and_then(|ch| ch.to_lowercase().next())
        .map_or(cp, |ch| ch as u32)
}

impl RegexScratch {
    fn new(inst_count: usize) -> Self {
        Self {
            current: RegexState::new(inst_count),
            next: RegexState::new(inst_count),
            capture_arena: Vec::new(),
        }
    }
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
impl Clone for JetRegexMatch {
    fn clone(&self) -> Self {
        // The capture cache is an execution hint, not observable state.
        // Cloning a match keeps the carrier allocation-free and lets the
        // clone materialize captures only if it is actually queried.
        Self {
            text: std::sync::Arc::clone(&self.text),
            span: self.span,
            program: std::sync::Arc::clone(&self.program),
            flags: self.flags.clone(),
            groups: self.groups,
            names: std::sync::Arc::clone(&self.names),
            capture_cache: std::sync::OnceLock::new(),
        }
    }
}


impl JetRegexMatch {
    pub fn group(&self, n: i64) -> JetOutcome<String, JetAbsent> {
        let Ok(n) = usize::try_from(n) else { return Err(JetAbsent) };
        let span = if n == 0 {
            Some(self.span)
        } else if n > self.groups {
            None
        } else {
            self.capture_spans().get(n).copied().flatten()
        };
        let Some((start, end)) = span else { return Err(JetAbsent) };
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

    /// Match spans use String character indices, not UTF-8 byte offsets, so
    /// they compose directly with Jet String indexing.
    pub fn start(&self) -> i64 {
        self.group_start(0).unwrap_or(-1)
    }

    pub fn end(&self) -> i64 {
        self.group_end(0).unwrap_or(-1)
    }

    pub fn group_start(&self, n: i64) -> JetOutcome<i64, JetAbsent> {
        let Ok(n) = usize::try_from(n) else { return Err(JetAbsent) };
        regex_outcome(self.span_for(n).map(|(start, _)| regex_char_index(&self.text, start) as i64))
    }

    pub fn group_end(&self, n: i64) -> JetOutcome<i64, JetAbsent> {
        let Ok(n) = usize::try_from(n) else { return Err(JetAbsent) };
        regex_outcome(self.span_for(n).map(|(_, end)| regex_char_index(&self.text, end) as i64))
    }

    pub fn group_count(&self) -> usize {
        self.groups
    }

    pub fn capture_names(&self) -> Vec<Option<String>> {
        self.names.as_ref().to_vec()
    }

    /// Named capture pairs as `[[name, value], …]` (unnamed groups omitted).
    pub fn named_captures(&self) -> Vec<Vec<String>> {
        if self.groups == 0 {
            return Vec::new();
        }
        let spans = self.capture_spans();
        self.names
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let name = n.as_ref()?.clone();
                let (start, end) = spans.get(i).copied().flatten()?;
                let value = self.text[start..end].to_string();
                Some(vec![name, value])
            })
            .collect()
    }

    fn span_for(&self, n: usize) -> Option<(usize, usize)> {
        if n == 0 {
            Some(self.span)
        } else if n > self.groups {
            None
        } else {
            self.capture_spans().get(n).copied().flatten()
        }
    }

    fn capture_spans(&self) -> &[Option<(usize, usize)>] {
        self.capture_cache
            .get_or_init(|| {
                if self.groups == 0 {
                    return vec![Some(self.span)];
                }
                let base = self.span.0;
                let window = &self.text[self.span.0..self.span.1];
                regex_run(
                    &self.program,
                    &self.flags,
                    self.groups,
                    window,
                    0,
                    true,
                    true,
                )
                .and_then(|run| run.caps)
                .map(|caps| regex_slots_to_spans(&caps))
                .map(|spans| {
                    spans
                        .into_iter()
                        .map(|span| span.map(|(start, end)| (start + base, end + base)))
                        .collect()
                })
                .unwrap_or_else(|| vec![Some(self.span)])
            })
            .as_slice()
    }
}

impl JetRegex {
    pub fn parse(pattern: &str) -> Result<Self, String> {
        jet_regex_compile_with(pattern, &RegexFlags::default())
    }

    pub fn pattern(&self) -> String {
        self.pattern.to_string()
    }

    pub fn source(&self) -> String {
        self.pattern.to_string()
    }

    pub fn flags(&self) -> String {
        let mut s = String::with_capacity(3);
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
        let mut count = 0usize;
        regex_scan(
            &self.program,
            &self.flags,
            self.groups,
            text,
            0,
            false,
            false,
            |_run| {
                count += 1;
                true
            },
        );
        count as i64
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.find_span(text).is_some()
    }

    pub fn full_match(&self, text: &str) -> bool {
        regex_run(&self.program, &self.flags, self.groups, text, 0, true, false)
            .is_some_and(|run| run.span == (0, text.len()))
    }

    pub fn match_value(&self, text: &str) -> JetOutcome<JetRegexMatch, JetAbsent> {
        regex_outcome(self.find_match(text))
    }

    pub fn find(&self, text: &str) -> JetOutcome<String, JetAbsent> {
        regex_outcome(self.find_span(text).map(|(start, end)| text[start..end].to_string()))
    }

    pub fn find_all(&self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        regex_scan(
            &self.program,
            &self.flags,
            self.groups,
            text,
            0,
            false,
            false,
            |run| {
                let (start, end) = run.span;
                out.push(text[start..end].to_string());
                true
            },
        );
        out
    }
    fn find_span(&self, text: &str) -> Option<(usize, usize)> {
        regex_run(
            &self.program,
            &self.flags,
            self.groups,
            text,
            0,
            false,
            false,
        )
        .map(|run| run.span)
    }

    fn find_match(&self, text: &str) -> Option<JetRegexMatch> {
        let run = regex_run(
            &self.program,
            &self.flags,
            self.groups,
            text,
            0,
            false,
            true,
        )?;
        let text = std::sync::Arc::<str>::from(text);
        Some(self.make_match_with_captures(&text, run.span, run.caps))
    }

    pub fn matches(&self, text: &str) -> Vec<JetRegexMatch> {
        let shared = std::sync::Arc::<str>::from(text);
        let mut out = Vec::new();
        regex_scan(
            &self.program,
            &self.flags,
            self.groups,
            text,
            0,
            false,
            false,
            |run| {
                out.push(self.make_match(&shared, run.span));
                true
            },
        );
        out
    }
    /// Replace every non-overlapping match from left to right.
    pub fn replace(&self, text: &str, repl: &str) -> String {
        self.replace_impl(text, |m| expand_regex_replacement(repl, m), true)
    }

    /// Replace at most the first non-overlapping match.
    pub fn replace_first(&self, text: &str, repl: &str) -> String {
        self.replace_impl(text, |m| expand_regex_replacement(repl, m), false)
    }

    pub fn replace_all_with<F>(&self, text: &str, f: F) -> String
    where
        F: Fn(JetRegexMatch) -> String,
    {
        self.replace_all_with_result(text, |m| Ok::<_, ()>(f(m)))
            .expect("infallible regex replacement")
    }

    pub fn replace_all_with_result<E, F>(&self, text: &str, mut replace: F) -> Result<String, E>
    where
        F: FnMut(JetRegexMatch) -> Result<String, E>,
    {
        let shared = std::sync::Arc::<str>::from(text);
        let mut out = String::with_capacity(text.len());
        let mut pos = 0;
        let mut error = None;
        regex_scan(
            &self.program,
            &self.flags,
            self.groups,
            text,
            0,
            false,
            false,
            |run| {
                let (start, end) = run.span;
                out.push_str(&text[pos..start]);
                let found = self.make_match_with_captures(&shared, run.span, run.caps);
                match replace(found) {
                    Ok(value) => {
                        out.push_str(&value);
                        pos = end;
                        true
                    }
                    Err(value) => {
                        error = Some(value);
                        false
                    }
                }
            },
        );
        if let Some(error) = error {
            return Err(error);
        }
        out.push_str(&text[pos.min(text.len())..]);
        Ok(out)
    }

    pub fn split(&self, text: &str) -> Vec<String> {
        self.split_limit(text, 0)
    }

    pub fn split_limit(&self, text: &str, limit: i64) -> Vec<String> {
        let mut out = Vec::new();
        let mut pos = 0;
        let mut splits = 0i64;
        if limit != 1 {
            regex_scan(
                &self.program,
                &self.flags,
                self.groups,
                text,
                0,
                false,
                false,
                |run| {
                    if limit > 0 && splits >= limit - 1 {
                        return false;
                    }
                    let (start, end) = run.span;
                    out.push(text[pos..start].to_string());
                    // Advance at every match boundary. For a zero-width
                    // delimiter `end == start`, so the next boundary emits
                    // exactly the intervening character(s) once.
                    pos = end;
                    splits += 1;
                    true
                },
            );
        }
        out.push(text[pos.min(text.len())..].to_string());
        out
    }

    fn replace_impl<F>(&self, text: &str, repl: F, all: bool) -> String
    where
        F: Fn(&JetRegexMatch) -> String,
    {
        let shared = std::sync::Arc::<str>::from(text);
        let mut out = String::with_capacity(text.len());
        let mut pos = 0;
        regex_scan(
            &self.program,
            &self.flags,
            self.groups,
            text,
            0,
            false,
            false,
            |run| {
                let (start, end) = run.span;
                out.push_str(&text[pos..start]);
                let mat = self.make_match(&shared, run.span);
                out.push_str(&repl(&mat));
                pos = end;
                all
            },
        );
        out.push_str(&text[pos.min(text.len())..]);
        out
    }

    fn make_match(
        &self,
        text: &std::sync::Arc<str>,
        span: (usize, usize),
    ) -> JetRegexMatch {
        self.make_match_with_captures(text, span, None)
    }

    fn make_match_with_captures(
        &self,
        text: &std::sync::Arc<str>,
        span: (usize, usize),
        caps: Option<Vec<Option<usize>>>,
    ) -> JetRegexMatch {
        let capture_cache = std::sync::OnceLock::new();
        if let Some(caps) = caps {
            let _ = capture_cache.set(regex_slots_to_spans(&caps));
        }
        JetRegexMatch {
            text: std::sync::Arc::clone(text),
            span,
            program: std::sync::Arc::clone(&self.program),
            flags: self.flags.clone(),
            groups: self.groups,
            names: std::sync::Arc::clone(&self.group_names),
            capture_cache,
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
    if let Some(regex) = regex_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(pattern, flags)
    {
        return Ok(regex);
    }
    // Do not hold the global cache lock while parsing or compiling. A cache
    // miss is rare after warm-up; concurrent misses may compile independently,
    // then converge on the first result inserted for this key.
    let regex = jet_regex_compile_uncached(pattern, flags)?;
    let mut cache = regex_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = cache.get(pattern, flags) {
        return Ok(existing);
    }
    cache.insert(regex.clone());
    Ok(regex)
}

fn jet_regex_compile_uncached(pattern: &str, flags: &RegexFlags) -> Result<JetRegex, String> {
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
    let insts = compiler.insts;
    Ok(JetRegex {
        pattern: std::sync::Arc::<str>::from(pattern),
        flags: flags.clone(),
        program: std::sync::Arc::new(RegexProgram {
            anchored_start: matches!(&insts[frag.start], RegexInst::AssertStart(_)),
            scratch: std::sync::Mutex::new(RegexScratch::new(insts.len())),
            insts,
            start: frag.start,
            literal: regex_literal_candidate(&root),
            required_literal: regex_required_literal(&root),
        }),
        group_names: std::sync::Arc::from(parser.names.into_boxed_slice()),
        groups: parser.groups,
    })
}

pub fn jet_regex_is_match(pattern: &JetRegex, text: &str) -> bool {
    pattern.is_match(text)
}

pub fn jet_regex_full_match(pattern: &JetRegex, text: &str) -> bool {
    pattern.full_match(text)
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

pub fn jet_regex_replace(
    pattern: &JetRegex,
    repl: &str,
    text: &str,
) -> String {
    pattern.replace(text, repl)
}

pub fn jet_regex_replace_first(pattern: &JetRegex, repl: &str, text: &str) -> String {
    pattern.replace_first(text, repl)
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
            if ch == ']' && !items.is_empty() {
                self.pos += 1;
                return Ok(RegexClass::new(negated, items));
            }
            // Standard class grammar: `]` as the FIRST member (after the
            // optional `^`) is a literal — `[^]]+` matches any run of
            // non-`]` characters, it is not an empty negated class.
            let first = if ch == ']' {
                self.pos += 1;
                RegexClassItem::Char(']')
            } else {
                self.parse_class_item()?
            };
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
            Some('d') => Ok(RegexAtom::Class(RegexClass::new(
                false,
                vec![RegexClassItem::Digit],
            ))),
            Some('D') => Ok(RegexAtom::Class(RegexClass::new(
                true,
                vec![RegexClassItem::Digit],
            ))),
            Some('w') => Ok(RegexAtom::Class(RegexClass::new(
                false,
                vec![RegexClassItem::Word],
            ))),
            Some('W') => Ok(RegexAtom::Class(RegexClass::new(
                true,
                vec![RegexClassItem::Word],
            ))),
            Some('s') => Ok(RegexAtom::Class(RegexClass::new(
                false,
                vec![RegexClassItem::Space],
            ))),
            Some('S') => Ok(RegexAtom::Class(RegexClass::new(
                true,
                vec![RegexClassItem::Space],
            ))),
            Some('p') => Ok(RegexAtom::Class(RegexClass::new(
                false,
                vec![self.parse_unicode_class(false)?],
            ))),
            Some('P') => Ok(RegexAtom::Class(RegexClass::new(
                true,
                vec![self.parse_unicode_class(true)?],
            ))),
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
                regex_simple_fold(*expected as u32) == regex_simple_fold(ch as u32)
            } else {
                *expected == ch
            }
        }
        RegexMatcher::Any => flags.dotall || ch != '\n',
        RegexMatcher::Class(class) => regex_class_matches(class, ch, flags),
    }
}

fn regex_class_item_matches(
    item: &RegexClassItem,
    ch: char,
    case_insensitive: bool,
) -> bool {
    match item {
        RegexClassItem::Char(c) => {
            if case_insensitive {
                regex_simple_fold(*c as u32) == regex_simple_fold(ch as u32)
            } else {
                *c == ch
            }
        }
        RegexClassItem::Range(a, b) => {
            if case_insensitive {
                let lc = char::from_u32(regex_simple_fold(ch as u32)).unwrap_or(ch);
                let la = char::from_u32(regex_simple_fold(*a as u32)).unwrap_or(*a);
                let lb = char::from_u32(regex_simple_fold(*b as u32)).unwrap_or(*b);
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
    }
}

fn regex_class_matches(class: &RegexClass, ch: char, flags: &RegexFlags) -> bool {
    if !flags.case_insensitive && ch.is_ascii() {
        return class.matches_ascii(ch as u8);
    }
    let yes = class
        .items
        .iter()
        .any(|item| regex_class_item_matches(item, ch, flags.case_insensitive));
    if class.negated { !yes } else { yes }
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

fn regex_capture_slots(
    mut head: Option<usize>,
    arena: &[RegexCaptureNode],
    len: usize,
) -> Vec<Option<usize>> {
    let mut slots = vec![None; len];
    while let Some(index) = head {
        let Some(node) = arena.get(index) else { break };
        if node.slot < len && slots[node.slot].is_none() {
            slots[node.slot] = Some(node.pos);
        }
        head = node.previous;
    }
    slots
}

struct RegexRun {
    span: (usize, usize),
    caps: Option<Vec<Option<usize>>>,
}

fn regex_run(
    program: &RegexProgram,
    flags: &RegexFlags,
    groups: usize,
    text: &str,
    start: usize,
    anchored: bool,
    capture: bool,
) -> Option<RegexRun> {
    let mut result = None;
    regex_scan(
        program,
        flags,
        groups,
        text,
        start,
        anchored,
        capture,
        |run| {
            result = Some(run);
            false
        },
    );
    result
}

// Unanchored Thompson/Pike scan. Start threads enter one shared VM at each
// UTF-8 boundary until the leftmost match begins; then no overlapping start
// can win, so the state resets at the non-overlap boundary. The VM never
// restarts from every candidate position. Thread lists and epsilon stacks
// stay instruction-sized and reusable.
fn regex_scan<F>(
    program: &RegexProgram,
    flags: &RegexFlags,
    groups: usize,
    text: &str,
    start: usize,
    anchored: bool,
    capture: bool,
    mut on_match: F,
) where
    F: FnMut(RegexRun) -> bool,
{
    if start > text.len() {
        return;
    }
    if !anchored && !capture && !flags.case_insensitive {
        if let Some(literal) = program.literal.as_deref() {
            regex_scan_literal(text, literal, start, &mut on_match);
            return;
        }
    }
    if !flags.case_insensitive && (!program.anchored_start || flags.multiline) {
        if let Some(literal) = program.required_literal.as_deref() {
            if regex_find_literal(text.as_bytes(), literal, start).is_none() {
                return;
            }
        }
    }

    let mut scratch = program.scratch.lock().unwrap();
    scratch.current.clear();
    scratch.next.clear();
    scratch.capture_arena.clear();
    let mut pos = start;
    let mut winner_start = None;
    let mut last_match = None;
    let single_start = anchored || (program.anchored_start && !flags.multiline);

    loop {
        let can_start = !program.anchored_start
            || flags.multiline
            || pos == 0;
        if winner_start.is_none() && can_start && (!anchored || pos == start) {
            let scratch_ref = &mut *scratch;
            let (current, capture_arena) =
                (&mut scratch_ref.current, &mut scratch_ref.capture_arena);
            regex_add_thread(
                current,
                program,
                flags,
                RegexThread {
                    pc: program.start,
                    start: pos,
                    caps: None,
                },
                pos,
                text,
                capture,
                capture_arena,
            );
        }
        if scratch.current.threads.is_empty() && single_start {
            return;
        }

        let found = scratch.current.matched;
        if winner_start.is_none() {
            winner_start = found.map(|thread| thread.start);
        }
        if let Some(winner) = winner_start {
            if let Some(found) = found.filter(|thread| thread.start == winner) {
                let caps = if capture {
                    found.caps.map(|head| {
                        let mut caps = regex_capture_slots(
                            Some(head),
                            &scratch.capture_arena,
                            (groups + 1) * 2,
                        );
                        caps[0] = Some(found.start);
                        caps[1] = Some(pos);
                        caps
                    })
                } else {
                    None
                };
                last_match = Some(RegexRun {
                    span: (found.start, pos),
                    caps,
                });
            }
        }

        if pos == text.len() {
            let Some(run) = last_match.take() else {
                return;
            };
            let resume = regex_next_search_pos(text, run.span.0, run.span.1);
            drop(scratch);
            if !on_match(run) {
                return;
            }
            if resume > text.len() {
                return;
            }
            scratch = program.scratch.lock().unwrap();
            scratch.current.clear();
            winner_start = None;
            last_match = None;
            pos = resume;
            continue;
        }
        let Some((ch, next_pos)) = regex_next_char(text, pos) else {
            return;
        };
        {
            let scratch_ref = &mut *scratch;
            let (current, next, capture_arena) = (
                &mut scratch_ref.current,
                &mut scratch_ref.next,
                &mut scratch_ref.capture_arena,
            );
            next.clear();
            for thread in &current.threads {
                let RegexInst::Consume(matcher, Some(target)) = &program.insts[thread.pc]
                else { continue };
                if !regex_matcher_matches(matcher, ch, flags) {
                    continue;
                };
                if capture {
                    regex_add_thread(
                        next,
                        program,
                        flags,
                        RegexThread {
                            pc: *target,
                            start: thread.start,
                            caps: thread.caps,
                        },
                        next_pos,
                        text,
                        capture,
                        capture_arena,
                    );
                } else {
                    regex_add_thread(
                        next,
                        program,
                        flags,
                        RegexThread {
                            pc: *target,
                            start: thread.start,
                            caps: None,
                        },
                        next_pos,
                        text,
                        capture,
                        capture_arena,
                    );
                }
            }
            std::mem::swap(current, next);
        }
        if let Some(winner) = winner_start {
            if scratch.current.min_start != Some(winner) {
                let Some(run) = last_match.take() else { return };
                let resume = regex_next_search_pos(text, run.span.0, run.span.1);
                drop(scratch);
                if !on_match(run) {
                    return;
                }
                if resume > text.len() {
                    return;
                }
                scratch = program.scratch.lock().unwrap();
                scratch.current.clear();
                winner_start = None;
                last_match = None;
                pos = resume;
                continue;
            }
        }
        pos = next_pos;
    }
}

fn regex_add_thread(
    state: &mut RegexState,
    program: &RegexProgram,
    flags: &RegexFlags,
    thread: RegexThread,
    pos: usize,
    text: &str,
    capture: bool,
    capture_arena: &mut Vec<RegexCaptureNode>,
) {
    state.stack.clear();
    state.stack.push(thread);
    while let Some(mut thread) = state.stack.pop() {
        if state.seen[thread.pc] == state.epoch {
            continue;
        }
        state.seen[thread.pc] = state.epoch;
        match &program.insts[thread.pc] {
            RegexInst::Save(slot, Some(next)) => {
                if capture && *slot != usize::MAX {
                    let index = capture_arena.len();
                    capture_arena.push(RegexCaptureNode {
                        slot: *slot,
                        pos,
                        previous: thread.caps,
                    });
                    thread.caps = Some(index);
                }
                thread.pc = *next;
                state.stack.push(thread);
            }
            RegexInst::Split(a, Some(b)) => {
                let mut right = thread;
                right.pc = *b;
                state.stack.push(right);
                thread.pc = *a;
                state.stack.push(thread);
            }
            RegexInst::AssertStart(Some(next)) => {
                if pos == 0
                    || (flags.multiline && text[..pos].ends_with('\n'))
                {
                    thread.pc = *next;
                    state.stack.push(thread);
                }
            }
            RegexInst::AssertEnd(Some(next)) => {
                if pos == text.len() {
                    thread.pc = *next;
                    state.stack.push(thread);
                }
            }
            RegexInst::Match => {
                state.min_start = Some(
                    state
                        .min_start
                        .map_or(thread.start, |start| start.min(thread.start)),
                );
                if state
                    .matched
                    .map_or(true, |matched| thread.start < matched.start)
                {
                    state.matched = Some(thread);
                }
                state.threads.push(thread);
            }
            RegexInst::Consume(_, _) => {
                state.min_start = Some(
                    state
                        .min_start
                        .map_or(thread.start, |start| start.min(thread.start)),
                );
                state.threads.push(thread);
            }
            _ => {}
        }
    }
}

fn regex_literal_candidate(node: &RegexNode) -> Option<Vec<u8>> {
    let RegexNode::Seq(pieces) = node else { return None };
    let mut literal = String::new();
    for piece in pieces {
        if !matches!(&piece.quant, RegexQuant::One) {
            return None;
        }
        let RegexAtom::Literal(ch) = &piece.atom else { return None };
        literal.push(*ch);
    }
    (!literal.is_empty()).then(|| literal.into_bytes())
}

fn regex_required_literal(node: &RegexNode) -> Option<Vec<u8>> {
    match node {
        RegexNode::Seq(pieces) => {
            let mut best = None;
            let mut run = Vec::new();
            for piece in pieces {
                if !regex_quant_required(&piece.quant) {
                    regex_prefer_longer(&mut best, std::mem::take(&mut run));
                    continue;
                }
                match (&piece.atom, &piece.quant) {
                    (RegexAtom::Literal(ch), RegexQuant::One) => {
                        let mut bytes = [0; 4];
                        run.extend(ch.encode_utf8(&mut bytes).as_bytes());
                    }
                    (RegexAtom::Literal(ch), _) => {
                        regex_prefer_longer(&mut best, std::mem::take(&mut run));
                        regex_prefer_longer(&mut best, ch.to_string().into_bytes());
                    }
                    _ => {
                        regex_prefer_longer(&mut best, std::mem::take(&mut run));
                        if let Some(literal) = regex_required_atom_literal(&piece.atom) {
                            regex_prefer_longer(&mut best, literal);
                        }
                    }
                }
            }
            regex_prefer_longer(&mut best, run);
            best
        }
        RegexNode::Alt(arms) => {
            let mut arms = arms.iter();
            let first = regex_required_literal(arms.next()?)?;
            arms.all(|arm| regex_required_literal(arm).as_deref() == Some(first.as_slice()))
                .then_some(first)
        }
    }
}

fn regex_required_atom_literal(atom: &RegexAtom) -> Option<Vec<u8>> {
    match atom {
        RegexAtom::Literal(ch) => Some(ch.to_string().into_bytes()),
        RegexAtom::Group(_, node) => regex_required_literal(node),
        RegexAtom::Any
        | RegexAtom::Class(_)
        | RegexAtom::Start
        | RegexAtom::End => None,
    }
}

fn regex_prefer_longer(best: &mut Option<Vec<u8>>, candidate: Vec<u8>) {
    if candidate.is_empty() {
        return;
    }
    if best
        .as_ref()
        .map_or(true, |current| candidate.len() > current.len())
    {
        *best = Some(candidate);
    }
}

fn regex_quant_required(quant: &RegexQuant) -> bool {
    !matches!(quant, RegexQuant::ZeroOrMore | RegexQuant::ZeroOrOne)
        && !matches!(quant, RegexQuant::Range { min: 0, .. })
}

// Required-literal prefilter. It rejects haystacks that cannot match before
// entering the VM. The VM remains the authority for non-literal programs.
fn regex_find_literal(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(haystack.len()));
    }
    let last = haystack.len().checked_sub(needle.len())?;
    let first = needle[0];
    let mut pos = start.min(haystack.len());
    while pos <= last {
        let candidate = regex_find_byte(haystack, first, pos)?;
        if candidate > last {
            return None;
        }
        pos = candidate;
        if haystack[pos..].starts_with(needle) {
            return Some(pos);
        }
        pos += 1;
    }
    None
}

fn regex_scan_literal<F>(
    text: &str,
    needle: &[u8],
    start: usize,
    mut on_match: F,
) where
    F: FnMut(RegexRun) -> bool,
{
    // A compiled literal is always valid UTF-8. Delegate the hot search to
    // `str::find`, whose std implementation uses its optimized memmem path;
    // unlike the byte prefilter this also guarantees UTF-8 boundary spans.
    let Ok(needle) = std::str::from_utf8(needle) else {
        return;
    };
    let mut pos = start.min(text.len());
    while let Some(offset) = text.get(pos..).and_then(|rest| rest.find(needle)) {
        let candidate = pos + offset;
        let end = candidate + needle.len();
        if !on_match(RegexRun {
            span: (candidate, end),
            caps: None,
        }) {
            return;
        }
        pos = regex_next_search_pos(text, candidate, end);
        if pos > text.len() {
            return;
        }
    }
}

// I1: SIMD helpers are the same vetted std::arch carve-out used by
// core.compute. Runtime dispatch keeps unsupported CPUs on scalar code.
#[derive(Clone, Copy)]
enum RegexByteBackend {
    Scalar,
    #[cfg(target_arch = "x86_64")]
    Avx2,
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    Sse2,
}

static REGEX_BYTE_BACKEND: std::sync::LazyLock<RegexByteBackend> =
    std::sync::LazyLock::new(|| {
        // JET_VETTED_UNSAFE_BEGIN: jet_regex_cpu_simd_dispatch
        #[cfg(target_arch = "x86_64")]
        if is_x86_feature_detected!("avx2") {
            return RegexByteBackend::Avx2;
        }
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        if is_x86_feature_detected!("sse2") {
            return RegexByteBackend::Sse2;
        }
        // JET_VETTED_UNSAFE_END: jet_regex_cpu_simd_dispatch
        RegexByteBackend::Scalar
    });

fn regex_find_byte(haystack: &[u8], needle: u8, start: usize) -> Option<usize> {
    match *REGEX_BYTE_BACKEND {
        // JET_VETTED_UNSAFE_BEGIN: jet_regex_cpu_simd_dispatch
        #[cfg(target_arch = "x86_64")]
        RegexByteBackend::Avx2 => unsafe { regex_find_byte_avx2(haystack, needle, start) },
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        RegexByteBackend::Sse2 => unsafe { regex_find_byte_sse2(haystack, needle, start) },
        // JET_VETTED_UNSAFE_END: jet_regex_cpu_simd_dispatch
        RegexByteBackend::Scalar => regex_find_byte_scalar(haystack, needle, start),
    }
}

fn regex_find_byte_scalar(haystack: &[u8], needle: u8, start: usize) -> Option<usize> {
    let mut pos = start.min(haystack.len());
    while pos < haystack.len() && pos % 8 != 0 {
        if haystack[pos] == needle {
            return Some(pos);
        }
        pos += 1;
    }
    let repeated = u64::from_ne_bytes([needle; 8]);
    const LOW_BITS: u64 = 0x0101_0101_0101_0101;
    const HIGH_BITS: u64 = 0x8080_8080_8080_8080;
    while haystack.len().saturating_sub(pos) >= 8 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&haystack[pos..pos + 8]);
        let different = u64::from_ne_bytes(bytes) ^ repeated;
        if different.wrapping_sub(LOW_BITS) & !different & HIGH_BITS != 0 {
            for offset in 0..8 {
                if haystack[pos + offset] == needle {
                    return Some(pos + offset);
                }
            }
        }
        pos += 8;
    }
    haystack
        .get(pos..)?
        .iter()
        .position(|byte| *byte == needle)
        .map(|offset| pos + offset)
}

// JET_VETTED_UNSAFE_BEGIN: jet_regex_cpu_simd
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn regex_find_byte_avx2(
    haystack: &[u8],
    needle: u8,
    start: usize,
) -> Option<usize> {
    use std::arch::x86_64::{
        _mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8, _mm256_set1_epi8,
    };
    let mut pos = start;
    let target = _mm256_set1_epi8(needle as i8);
    while pos <= haystack.len().saturating_sub(32) {
        let chunk = _mm256_loadu_si256(haystack.as_ptr().add(pos).cast());
        let mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, target)) as u32;
        if mask != 0 {
            return Some(pos + mask.trailing_zeros() as usize);
        }
        pos += 32;
    }
    haystack
        .get(pos..)?
        .iter()
        .position(|byte| *byte == needle)
        .map(|offset| pos + offset)
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "avx2")]
unsafe fn regex_find_byte_avx2(
    haystack: &[u8],
    needle: u8,
    start: usize,
) -> Option<usize> {
    use std::arch::x86::{
        _mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8, _mm256_set1_epi8,
    };
    let mut pos = start;
    let target = _mm256_set1_epi8(needle as i8);
    while pos <= haystack.len().saturating_sub(32) {
        let chunk = _mm256_loadu_si256(haystack.as_ptr().add(pos).cast());
        let mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, target)) as u32;
        if mask != 0 {
            return Some(pos + mask.trailing_zeros() as usize);
        }
        pos += 32;
    }
    haystack
        .get(pos..)?
        .iter()
        .position(|byte| *byte == needle)
        .map(|offset| pos + offset)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn regex_find_byte_sse2(
    haystack: &[u8],
    needle: u8,
    start: usize,
) -> Option<usize> {
    use std::arch::x86_64::{
        _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_set1_epi8,
    };
    let mut pos = start;
    let target = _mm_set1_epi8(needle as i8);
    while pos <= haystack.len().saturating_sub(16) {
        let chunk = _mm_loadu_si128(haystack.as_ptr().add(pos).cast());
        let mask = _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, target)) as u32;
        if mask != 0 {
            return Some(pos + mask.trailing_zeros() as usize);
        }
        pos += 16;
    }
    haystack
        .get(pos..)?
        .iter()
        .position(|byte| *byte == needle)
        .map(|offset| pos + offset)
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "sse2")]
unsafe fn regex_find_byte_sse2(
    haystack: &[u8],
    needle: u8,
    start: usize,
) -> Option<usize> {
    use std::arch::x86::{
        _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_set1_epi8,
    };
    let mut pos = start;
    let target = _mm_set1_epi8(needle as i8);
    while pos <= haystack.len().saturating_sub(16) {
        let chunk = _mm_loadu_si128(haystack.as_ptr().add(pos).cast());
        let mask = _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, target)) as u32;
        if mask != 0 {
            return Some(pos + mask.trailing_zeros() as usize);
        }
        pos += 16;
    }
    haystack
        .get(pos..)?
        .iter()
        .position(|byte| *byte == needle)
        .map(|offset| pos + offset)
}
// JET_VETTED_UNSAFE_END: jet_regex_cpu_simd

fn regex_next_search_pos(text: &str, start: usize, end: usize) -> usize {
    if end > start {
        return end;
    }
    regex_next_char(text, end)
        .map(|(_, next)| next)
        .unwrap_or(text.len() + 1)
}

fn regex_next_char(text: &str, pos: usize) -> Option<(char, usize)> {
    let byte = *text.as_bytes().get(pos)?;
    if byte.is_ascii() {
        return Some((byte as char, pos + 1));
    }
    text[pos..]
        .chars()
        .next()
        .map(|ch| (ch, pos + ch.len_utf8()))
}

fn expand_regex_replacement(repl: &str, mat: &JetRegexMatch) -> String {
    let mut out = String::with_capacity(repl.len());
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
                let mut idx = 0i64;
                let mut valid = true;
                while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                    let digit = chars.next().unwrap() as i64 - '0' as i64;
                    if let Some(next) = idx.checked_mul(10).and_then(|n| n.checked_add(digit)) {
                        idx = next;
                    } else {
                        valid = false;
                    }
                }
                if valid {
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_reuses_compiled_program() {
        let first = jet_regex_compile("__regex_cache_probe__").unwrap();
        let second = jet_regex_compile("__regex_cache_probe__").unwrap();
        assert!(std::sync::Arc::ptr_eq(&first.program, &second.program));
    }

    #[test]
    fn match_and_replacement_preserve_captures() {
        let regex = JetRegex::parse(r"(?<word>[A-Za-z]+)_(\d+)").unwrap();
        let Some(matched) = regex.match_value("xx Jet_2026 yy").ok() else {
            panic!("expected regex match");
        };
        assert_eq!(matched.group(0).ok().as_deref(), Some("Jet_2026"));
        assert_eq!(matched.group(1).ok().as_deref(), Some("Jet"));
        assert_eq!(matched.group(2).ok().as_deref(), Some("2026"));
        assert_eq!(matched.name("word").ok().as_deref(), Some("Jet"));
        assert_eq!(matched.start(), 3);
        assert_eq!(matched.end(), 11);
        assert_eq!(
            regex.replace("Jet_2026 Rust_2025", "${word}:$2"),
            "Jet:2026 Rust:2025"
        );
    }

    #[test]
    fn ascii_class_bitmap_keeps_unicode_fallback() {
        let not_close = JetRegex::parse(r"[^]]+").unwrap();
        assert_eq!(not_close.find("abc]def").ok().as_deref(), Some("abc"));
        assert_eq!(not_close.find("éx]def").ok().as_deref(), Some("éx"));

        let alphabetic = JetRegex::parse(r"\p{Alphabetic}+").unwrap();
        assert_eq!(alphabetic.find("123λ!").ok().as_deref(), Some("λ"));
    }
}
