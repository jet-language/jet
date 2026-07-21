#[derive(Clone)]
pub(super) struct RegexLite {
    root: Node,
    groups: usize,
}

#[derive(Clone)]
enum Node {
    Seq(Vec<Piece>),
    Alt(Vec<Node>),
}

#[derive(Clone)]
struct Piece {
    atom: Atom,
    quant: Quant,
}

#[derive(Clone)]
enum Atom {
    Literal(char),
    Any,
    Class(Class),
    Group(usize, Box<Node>),
    Start,
    End,
}

#[derive(Clone)]
struct Class {
    negated: bool,
    items: Vec<ClassItem>,
}

#[derive(Clone)]
enum ClassItem {
    Char(char),
    Range(char, char),
    Digit,
    Word,
    Space,
}

#[derive(Clone, Copy)]
enum Quant {
    One,
    ZeroOrMore,
    OneOrMore,
    ZeroOrOne,
    Exact(usize),
}

#[derive(Clone)]
struct State {
    pos: usize,
    caps: Vec<Option<(usize, usize)>>,
}

#[derive(Clone)]
pub(super) struct MatchLite {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) groups: Vec<Option<(usize, usize)>>,
}

impl RegexLite {
    pub(super) fn parse(pattern: &str) -> Result<Self, String> {
        let mut parser = Parser {
            chars: pattern.chars().collect(),
            pos: 0,
            groups: 0,
        };
        let root = parser.parse_alt(None)?;
        if parser.pos != parser.chars.len() {
            return Err("unexpected trailing regex input".to_string());
        }
        Ok(Self {
            root,
            groups: parser.groups,
        })
    }

    pub(super) fn is_match(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    pub(super) fn find(&self, text: &str) -> Option<MatchLite> {
        self.find_from(text, 0)
    }

    pub(super) fn find_all<'a>(&self, text: &'a str) -> Vec<&'a str> {
        let mut out = Vec::new();
        let mut pos = 0;
        while pos <= text.len() {
            let Some(m) = self.find_from(text, pos) else {
                break;
            };
            out.push(&text[m.start..m.end]);
            pos = next_search_pos(text, m.start, m.end);
        }
        out
    }

    pub(super) fn split<'a>(&self, text: &'a str) -> Vec<&'a str> {
        let mut out = Vec::new();
        let mut pos = 0;
        while pos <= text.len() {
            let Some(m) = self.find_from(text, pos) else {
                break;
            };
            out.push(&text[pos..m.start]);
            pos = next_search_pos(text, m.start, m.end);
        }
        out.push(&text[pos.min(text.len())..]);
        out
    }

    pub(super) fn replace_all(&self, text: &str, repl: &str) -> String {
        let mut out = String::new();
        let mut pos = 0;
        while pos <= text.len() {
            let Some(m) = self.find_from(text, pos) else {
                break;
            };
            out.push_str(&text[pos..m.start]);
            out.push_str(repl);
            pos = next_search_pos(text, m.start, m.end);
        }
        out.push_str(&text[pos.min(text.len())..]);
        out
    }

    fn find_from(&self, text: &str, start: usize) -> Option<MatchLite> {
        for pos in search_positions(text, start) {
            let state = State {
                pos,
                caps: vec![None; self.groups + 1],
            };
            let matches = match_node(&self.root, text, state);
            if let Some(mut found) = matches.into_iter().next() {
                found.caps[0] = Some((pos, found.pos));
                return Some(MatchLite {
                    start: pos,
                    end: found.pos,
                    groups: found.caps,
                });
            };
        }
        None
    }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    groups: usize,
}

impl Parser {
    fn parse_alt(&mut self, terminator: Option<char>) -> Result<Node, String> {
        let mut arms = vec![self.parse_seq(terminator)?];
        while self.peek() == Some('|') {
            self.pos += 1;
            arms.push(self.parse_seq(terminator)?);
        }
        if let Some(end) = terminator {
            if self.peek() != Some(end) {
                return Err(format!("missing `{end}` in regex pattern"));
            }
            self.pos += 1;
        }
        if arms.len() == 1 {
            Ok(arms.remove(0))
        } else {
            Ok(Node::Alt(arms))
        }
    }

    fn parse_seq(&mut self, terminator: Option<char>) -> Result<Node, String> {
        let mut pieces = Vec::new();
        while let Some(ch) = self.peek() {
            if Some(ch) == terminator || ch == '|' {
                break;
            }
            let atom = self.parse_atom()?;
            let quant = self.parse_quant()?;
            pieces.push(Piece { atom, quant });
        }
        Ok(Node::Seq(pieces))
    }

    fn parse_atom(&mut self) -> Result<Atom, String> {
        let Some(ch) = self.bump() else {
            return Err("empty regex atom".to_string());
        };
        match ch {
            '.' => Ok(Atom::Any),
            '^' => Ok(Atom::Start),
            '$' => Ok(Atom::End),
            '(' => {
                self.groups += 1;
                let idx = self.groups;
                Ok(Atom::Group(idx, Box::new(self.parse_alt(Some(')'))?)))
            }
            ')' => Err("unmatched `)` in regex pattern".to_string()),
            '[' => Ok(Atom::Class(self.parse_class()?)),
            '\\' => self.parse_escape_atom(),
            '*' | '+' | '?' => Err(format!("regex quantifier `{ch}` has nothing to repeat")),
            '{' => Err("regex `{n}` quantifier has nothing to repeat".to_string()),
            other => Ok(Atom::Literal(other)),
        }
    }

    fn parse_quant(&mut self) -> Result<Quant, String> {
        match self.peek() {
            Some('*') => {
                self.pos += 1;
                Ok(Quant::ZeroOrMore)
            }
            Some('+') => {
                self.pos += 1;
                Ok(Quant::OneOrMore)
            }
            Some('?') => {
                self.pos += 1;
                Ok(Quant::ZeroOrOne)
            }
            Some('{') => {
                self.pos += 1;
                let n = self.parse_number()?;
                if self.bump() != Some('}') {
                    return Err(
                        "only exact `{n}` regex quantifiers are supported at comptime".to_string(),
                    );
                }
                Ok(Quant::Exact(n))
            }
            _ => Ok(Quant::One),
        }
    }

    fn parse_class(&mut self) -> Result<Class, String> {
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
                return Ok(Class { negated, items });
            }
            let first = self.parse_class_item()?;
            if self.peek() == Some('-') && self.peek_n(1) != Some(']') {
                self.pos += 1;
                let second = self.parse_class_char()?;
                let ClassItem::Char(a) = first else {
                    return Err("regex class ranges need literal endpoints".to_string());
                };
                items.push(ClassItem::Range(a, second));
            } else {
                items.push(first);
            }
        }
        Err("missing `]` in regex pattern".to_string())
    }

    fn parse_class_item(&mut self) -> Result<ClassItem, String> {
        if self.peek() == Some('\\') {
            self.pos += 1;
            return match self.bump() {
                Some('d') => Ok(ClassItem::Digit),
                Some('w') => Ok(ClassItem::Word),
                Some('s') => Ok(ClassItem::Space),
                Some(ch) => Ok(ClassItem::Char(escaped_literal(ch))),
                None => Err("missing regex escape".to_string()),
            };
        }
        self.parse_class_char().map(ClassItem::Char)
    }

    fn parse_class_char(&mut self) -> Result<char, String> {
        match self.bump() {
            Some(']') | None => Err("missing class character in regex pattern".to_string()),
            Some('\\') => self
                .bump()
                .map(escaped_literal)
                .ok_or_else(|| "missing regex escape".to_string()),
            Some(ch) => Ok(ch),
        }
    }

    fn parse_escape_atom(&mut self) -> Result<Atom, String> {
        match self.bump() {
            Some('d') => Ok(Atom::Class(Class {
                negated: false,
                items: vec![ClassItem::Digit],
            })),
            Some('D') => Ok(Atom::Class(Class {
                negated: true,
                items: vec![ClassItem::Digit],
            })),
            Some('w') => Ok(Atom::Class(Class {
                negated: false,
                items: vec![ClassItem::Word],
            })),
            Some('W') => Ok(Atom::Class(Class {
                negated: true,
                items: vec![ClassItem::Word],
            })),
            Some('s') => Ok(Atom::Class(Class {
                negated: false,
                items: vec![ClassItem::Space],
            })),
            Some('S') => Ok(Atom::Class(Class {
                negated: true,
                items: vec![ClassItem::Space],
            })),
            Some(ch) => Ok(Atom::Literal(escaped_literal(ch))),
            None => Err("missing regex escape".to_string()),
        }
    }

    fn parse_number(&mut self) -> Result<usize, String> {
        let start = self.pos;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == start {
            return Err("regex `{n}` quantifier needs a number".to_string());
        }
        self.chars[start..self.pos]
            .iter()
            .collect::<String>()
            .parse::<usize>()
            .map_err(|_| "regex quantifier is too large".to_string())
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

fn match_node(node: &Node, text: &str, state: State) -> Vec<State> {
    match node {
        Node::Seq(pieces) => match_pieces(pieces, text, vec![state]),
        Node::Alt(arms) => arms
            .iter()
            .flat_map(|arm| match_node(arm, text, state.clone()))
            .collect(),
    }
}

fn match_pieces(pieces: &[Piece], text: &str, states: Vec<State>) -> Vec<State> {
    if pieces.is_empty() {
        return states;
    }
    let mut next = Vec::new();
    for state in states {
        next.extend(match_piece(&pieces[0], text, state));
    }
    match_pieces(&pieces[1..], text, next)
}

fn match_piece(piece: &Piece, text: &str, state: State) -> Vec<State> {
    match piece.quant {
        Quant::One => match_atom(&piece.atom, text, state),
        Quant::ZeroOrMore => repeat_atom(&piece.atom, text, state, 0, None),
        Quant::OneOrMore => repeat_atom(&piece.atom, text, state, 1, None),
        Quant::ZeroOrOne => {
            let mut out = match_atom(&piece.atom, text, state.clone());
            out.push(state);
            out
        }
        Quant::Exact(n) => repeat_atom(&piece.atom, text, state, n, Some(n)),
    }
}

fn repeat_atom(
    atom: &Atom,
    text: &str,
    state: State,
    min: usize,
    max: Option<usize>,
) -> Vec<State> {
    fn rec(
        atom: &Atom,
        text: &str,
        state: State,
        count: usize,
        min: usize,
        max: Option<usize>,
        out: &mut Vec<State>,
    ) {
        if max.is_some_and(|limit| count == limit) {
            if count >= min {
                out.push(state);
            }
            return;
        }
        let next_states = match_atom(atom, text, state.clone());
        for next in next_states {
            if next.pos == state.pos {
                continue;
            }
            rec(atom, text, next, count + 1, min, max, out);
        }
        if count >= min && max.is_none() {
            out.push(state);
        }
    }

    let mut out = Vec::new();
    rec(atom, text, state, 0, min, max, &mut out);
    out
}

fn match_atom(atom: &Atom, text: &str, state: State) -> Vec<State> {
    match atom {
        Atom::Literal(expected) => match_char(text, state, |ch| ch == *expected),
        Atom::Any => match_char(text, state, |ch| ch != '\n'),
        Atom::Class(class) => match_char(text, state, |ch| class_matches(class, ch)),
        Atom::Start => {
            if state.pos == 0 {
                vec![state]
            } else {
                Vec::new()
            }
        }
        Atom::End => {
            if state.pos == text.len() {
                vec![state]
            } else {
                Vec::new()
            }
        }
        Atom::Group(idx, node) => {
            let start = state.pos;
            match_node(node, text, state)
                .into_iter()
                .map(|mut next| {
                    next.caps[*idx] = Some((start, next.pos));
                    next
                })
                .collect()
        }
    }
}

fn match_char(text: &str, state: State, pred: impl FnOnce(char) -> bool) -> Vec<State> {
    let Some(ch) = text[state.pos..].chars().next() else {
        return Vec::new();
    };
    if pred(ch) {
        vec![State {
            pos: state.pos + ch.len_utf8(),
            caps: state.caps,
        }]
    } else {
        Vec::new()
    }
}

fn class_matches(class: &Class, ch: char) -> bool {
    let yes = class.items.iter().any(|item| match item {
        ClassItem::Char(c) => *c == ch,
        ClassItem::Range(a, b) => *a <= ch && ch <= *b,
        ClassItem::Digit => ch.is_ascii_digit(),
        ClassItem::Word => ch == '_' || ch.is_ascii_alphanumeric(),
        ClassItem::Space => super::TextLite::whitespace(ch as u32),
    });
    if class.negated {
        !yes
    } else {
        yes
    }
}

fn escaped_literal(ch: char) -> char {
    match ch {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        other => other,
    }
}

fn search_positions(text: &str, start: usize) -> impl Iterator<Item = usize> + '_ {
    text.char_indices()
        .map(|(idx, _)| idx)
        .filter(move |idx| *idx >= start)
        .chain(std::iter::once(text.len()).filter(move |idx| *idx >= start))
}

fn next_search_pos(text: &str, start: usize, end: usize) -> usize {
    if end > start {
        return end;
    }
    text[end..]
        .chars()
        .next()
        .map(|ch| end + ch.len_utf8())
        .unwrap_or(text.len() + 1)
}
