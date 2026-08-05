use super::{
    DerivationEvaluation, DerivationOutputEvaluation, DevShellEvaluation, EvaluationError,
};
use super::JSON::{self, JSON as JSONValue};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::{Rc, Weak};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use core::cell::{Cell, RefCell};
use core::fmt;
use core::mem;

const MAX_TOKENS: usize = super::EVALUATOR_TOKEN_LIMIT;
pub(crate) const MAX_EVAL_DEPTH: usize = super::EVALUATOR_EXPRESSION_LIMIT;
const MAX_DEV_SHELL_PACKAGES: usize = 256;
const MAX_PACKAGE_NAME_BYTES: usize = 128;
const MAX_IMPORTS: usize = super::EVALUATOR_IMPORT_LIMIT;
const MAX_PATH_BYTES: usize = 4_096;
const MAX_STRING_PARTS: usize = 256;
const MAX_STRING_BYTES: usize = super::EVALUATOR_STRING_BYTES;
const MAX_DERIVATION_ARGS: usize = 256;
const MAX_DERIVATION_ENV: usize = 256;
const MAX_DERIVATION_INPUTS: usize = 256;
const MAX_DERIVATION_OUTPUTS: usize = 64;
const NIX32_CHARS: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";

#[derive(Debug, Clone)]
enum Error {
    Syntax(String),
    Unsupported(String),
    Invalid(String),
    ResourceLimit(String),
    Missing(String),
    Type { expected: &'static str, actual: &'static str },
    Cycle,
}

impl Error {
    fn public(self) -> EvaluationError {
        let message = self.to_string();
        match self {
            Self::Unsupported(reason) => EvaluationError::Unsupported(reason),
            Self::ResourceLimit(reason) => EvaluationError::ResourceLimit(reason),
            Self::Syntax(_)
            | Self::Invalid(_)
            | Self::Missing(_)
            | Self::Cycle
            | Self::Type { .. } => EvaluationError::Invalid(message),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(reason) => write!(output, "invalid foreign flake expression: {reason}"),
            Self::Unsupported(reason) => {
                write!(output, "unsupported foreign flake expression: {reason}")
            }
            Self::Invalid(reason) => write!(output, "invalid foreign flake expression: {reason}"),
            Self::ResourceLimit(reason) => {
                write!(output, "foreign flake evaluator limit exceeded: {reason}")
            }
            Self::Missing(name) => write!(output, "missing foreign flake value `{name}`"),
            Self::Type { expected, actual } => {
                write!(output, "expected {expected} in foreign flake, got {actual}")
            }
            Self::Cycle => write!(output, "cyclic foreign flake evaluation"),
        }
    }
}

#[derive(Clone)]
enum Expr {
    String(String),
    StringContext(Vec<StringPart>),
    Path(String),
    Integer(i64),
    Bool(bool),
    Null,
    Identifier(String),
    AttrSet(Vec<(String, Expr)>),
    List(Vec<Expr>),
    Lambda(Pattern, Box<Expr>),
    Apply(Box<Expr>, Box<Expr>),
    Select(Box<Expr>, String),
    Let(Vec<(String, Expr)>, Box<Expr>),
    With(Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Merge(Box<Expr>, Box<Expr>),
    Equal(Box<Expr>, Box<Expr>, bool),
}

#[derive(Clone)]
enum StringPart {
    Literal(String),
    Expression(Box<Expr>),
}

#[derive(Clone)]
enum StringTokenPart {
    Literal(String),
    Expression(String),
}

#[derive(Clone)]
enum Pattern {
    Name(String),
    Attrs(Vec<(String, Option<Expr>)>),
}

#[derive(Clone)]
enum Token {
    Identifier(String),
    String(String),
    StringContext(Vec<StringTokenPart>),
    Path(String),
    Integer(i64),
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    LeftParen,
    RightParen,
    Equal,
    EqualEqual,
    NotEqual,
    Merge,
    Colon,
    Semicolon,
    Dot,
    Comma,
    Question,
    Ellipsis,
    End,
}

struct Lexer<'a> {
    source: &'a str,
    position: usize,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
            tokens: Vec::new(),
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>, Error> {
        while self.position < self.source.len() {
            self.skip_space_and_comments()?;
            if self.position == self.source.len() {
                break;
            }
            let token = self.token()?;
            self.push(token)?;
        }
        self.tokens.push(Token::End);
        Ok(self.tokens)
    }

    fn push(&mut self, token: Token) -> Result<(), Error> {
        if self.tokens.len() >= MAX_TOKENS {
            return Err(Error::ResourceLimit(format!(
                "foreign flake has more than {MAX_TOKENS} tokens"
            )));
        }
        self.tokens.push(token);
        Ok(())
    }

    fn skip_space_and_comments(&mut self) -> Result<(), Error> {
        loop {
            while self
                .source
                .as_bytes()
                .get(self.position)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.position += 1;
            }
            if self.source.as_bytes().get(self.position) == Some(&b'#') {
                while self.position < self.source.len()
                    && self.source.as_bytes()[self.position] != b'\n'
                {
                    self.position += 1;
                }
                continue;
            }
            if self.source.as_bytes().get(self.position) == Some(&b'/')
                && self.source.as_bytes().get(self.position + 1) == Some(&b'*')
            {
                self.position += 2;
                while self.position + 1 < self.source.len()
                    && !(self.source.as_bytes()[self.position] == b'*'
                        && self.source.as_bytes()[self.position + 1] == b'/')
                {
                    self.position += 1;
                }
                if self.position + 1 >= self.source.len() {
                    return Err(Error::Syntax("unterminated comment".into()));
                }
                self.position += 2;
                continue;
            }
            return Ok(());
        }
    }

    fn token(&mut self) -> Result<Token, Error> {
        let byte = self.source.as_bytes()[self.position];
        let token = match byte {
            b'{' => {
                self.position += 1;
                Token::LeftBrace
            }
            b'}' => {
                self.position += 1;
                Token::RightBrace
            }
            b'[' => {
                self.position += 1;
                Token::LeftBracket
            }
            b']' => {
                self.position += 1;
                Token::RightBracket
            }
            b'(' => {
                self.position += 1;
                Token::LeftParen
            }
            b')' => {
                self.position += 1;
                Token::RightParen
            }
            b'=' => {
                self.position += 1;
                if self.source.as_bytes().get(self.position) == Some(&b'=') {
                    self.position += 1;
                    Token::EqualEqual
                } else {
                    Token::Equal
                }
            }
            b'!' => {
                self.position += 1;
                if self.source.as_bytes().get(self.position) == Some(&b'=') {
                    self.position += 1;
                    Token::NotEqual
                } else {
                    return Err(Error::Unsupported("unary `!` is not supported".into()));
                }
            }
            b'/' if self.source.as_bytes().get(self.position + 1) == Some(&b'/') => {
                self.position += 2;
                Token::Merge
            }
            b'/' => Token::Path(self.path()?),
            b':' => {
                self.position += 1;
                Token::Colon
            }
            b';' => {
                self.position += 1;
                Token::Semicolon
            }
            b'.' if self.source.as_bytes().get(self.position + 1) == Some(&b'.')
                && self.source.as_bytes().get(self.position + 2) == Some(&b'.') =>
            {
                self.position += 3;
                Token::Ellipsis
            }
            b'.' if self.source.as_bytes().get(self.position + 1) == Some(&b'/')
                || (self.source.as_bytes().get(self.position + 1) == Some(&b'.')
                    && self.source.as_bytes().get(self.position + 2) == Some(&b'/')) =>
            {
                Token::Path(self.path()?)
            }
            b'.' => {
                self.position += 1;
                Token::Dot
            }
            b',' => {
                self.position += 1;
                Token::Comma
            }
            b'?' => {
                self.position += 1;
                Token::Question
            }
            b'"' => self.string()?,
            b'\'' if self.source.as_bytes().get(self.position + 1) == Some(&b'\'') => {
                self.indented_string()?
            }
            b'-' if self
                .source
                .as_bytes()
                .get(self.position + 1)
                .is_some_and(u8::is_ascii_digit) => Token::Integer(self.integer()?),
            b'0'..=b'9' => Token::Integer(self.integer()?),
            value if value.is_ascii_alphabetic() || value == b'_' => {
                Token::Identifier(self.identifier())
            }
            _ => {
                return Err(Error::Unsupported(format!(
                    "character `{}` is not supported",
                    byte as char
                )));
            }
        };
        Ok(token)
    }

    fn string(&mut self) -> Result<Token, Error> {
        self.position += 1;
        let mut literal = String::new();
        let mut parts: Option<Vec<StringTokenPart>> = None;
        while self.position < self.source.len() {
            let byte = self.source.as_bytes()[self.position];
            if byte == b'"' {
                self.position += 1;
                return Ok(match parts {
                    None => Token::String(literal),
                    Some(mut parts) => {
                        if !literal.is_empty() {
                            parts.push(StringTokenPart::Literal(literal));
                        }
                        Token::StringContext(parts)
                    }
                });
            }
            if byte == b'\\' {
                self.position += 1;
                let escaped = self.source.as_bytes().get(self.position).copied();
                let character = match escaped {
                    Some(b'n') => '\n',
                    Some(b'r') => '\r',
                    Some(b't') => '\t',
                    Some(b'"') => '"',
                    Some(b'\\') => '\\',
                    Some(b'$') => '$',
                    _ => return Err(Error::Syntax("invalid string escape".into())),
                };
                literal.push(character);
                self.position += 1;
                continue;
            }
            if byte == b'$' && self.source.as_bytes().get(self.position + 1) == Some(&b'{') {
                let parts = parts.get_or_insert_with(Vec::new);
                if !literal.is_empty() {
                    parts.push(StringTokenPart::Literal(mem::take(&mut literal)));
                }
                self.position += 2;
                parts.push(StringTokenPart::Expression(self.interpolation()?));
                if parts.len() > MAX_STRING_PARTS {
                    return Err(Error::ResourceLimit(format!(
                        "foreign flake string has more than {MAX_STRING_PARTS} parts"
                    )));
                }
                continue;
            }
            let character = self.source[self.position..]
                .chars()
                .next()
                .ok_or_else(|| Error::Syntax("invalid UTF-8 string".into()))?;
            literal.push(character);
            self.position += character.len_utf8();
        }
        Err(Error::Syntax("unterminated string".into()))
    }

    fn indented_string(&mut self) -> Result<Token, Error> {
        self.position += 2;
        let start = self.position;
        while self.position + 1 < self.source.len()
            && !(self.source.as_bytes()[self.position] == b'\''
                && self.source.as_bytes()[self.position + 1] == b'\'')
        {
            self.position += 1;
        }
        if self.position + 1 >= self.source.len() {
            return Err(Error::Syntax("unterminated indented string".into()));
        }
        let value = self.source[start..self.position].to_string();
        self.position += 2;
        let parts = interpolated_parts(&value)?;
        Ok(match parts {
            None => Token::String(value),
            Some(parts) => Token::StringContext(parts),
        })
    }

    fn interpolation(&mut self) -> Result<String, Error> {
        capture_interpolation(self.source, &mut self.position)
    }

    fn path(&mut self) -> Result<String, Error> {
        let start = self.position;
        while let Some(byte) = self.source.as_bytes().get(self.position).copied() {
            if byte.is_ascii_whitespace() || matches!(byte, b';' | b',' | b'}' | b']' | b')') {
                break;
            }
            self.position += 1;
        }
        let path = self.source[start..self.position].to_string();
        if path.is_empty() {
            return Err(Error::Syntax("path value requires a path".into()));
        }
        if path.contains("${") {
            return Err(Error::Unsupported(
                "path interpolation is not supported in the bounded evaluator".into(),
            ));
        }
        Ok(path)
    }

    fn integer(&mut self) -> Result<i64, Error> {
        let start = self.position;
        if self.source.as_bytes().get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        while self
            .source
            .as_bytes()
            .get(self.position)
            .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }
        self.source[start..self.position]
            .parse()
            .map_err(|_| Error::ResourceLimit("integer literal is out of range".into()))
    }

    fn identifier(&mut self) -> String {
        let start = self.position;
        self.position += 1;
        while self
            .source
            .as_bytes()
            .get(self.position)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-'))
        {
            self.position += 1;
        }
        self.source[start..self.position].to_string()
    }
}

fn interpolated_parts(source: &str) -> Result<Option<Vec<StringTokenPart>>, Error> {
    let mut position = 0;
    let mut literal = String::new();
    let mut parts = None;
    while position < source.len() {
        if source.as_bytes()[position] == b'$'
            && source.as_bytes().get(position + 1) == Some(&b'{')
        {
            let parts = parts.get_or_insert_with(Vec::new);
            if !literal.is_empty() {
                parts.push(StringTokenPart::Literal(mem::take(&mut literal)));
            }
            position += 2;
            parts.push(StringTokenPart::Expression(capture_interpolation(
                source,
                &mut position,
            )?));
            if parts.len() > MAX_STRING_PARTS {
                return Err(Error::ResourceLimit(format!(
                    "foreign flake string has more than {MAX_STRING_PARTS} parts"
                )));
            }
            continue;
        }
        let character = source[position..]
            .chars()
            .next()
            .ok_or_else(|| Error::Syntax("invalid UTF-8 string".into()))?;
        literal.push(character);
        position += character.len_utf8();
    }
    if let Some(mut parts) = parts {
        if !literal.is_empty() {
            parts.push(StringTokenPart::Literal(literal));
        }
        Ok(Some(parts))
    } else {
        Ok(None)
    }
}

fn capture_interpolation(source: &str, position: &mut usize) -> Result<String, Error> {
    let start = *position;
    let mut braces = 0usize;
    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    while *position < source.len() {
        let byte = source.as_bytes()[*position];
        match byte {
            b'"' => skip_double_string(source, position)?,
            b'\'' if source.as_bytes().get(*position + 1) == Some(&b'\'') => {
                skip_indented_string(source, position)?;
            }
            b'{' => {
                braces += 1;
                *position += 1;
            }
            b'}' if braces == 0 && parentheses == 0 && brackets == 0 => {
                let expression = source[start..*position].to_string();
                *position += 1;
                return Ok(expression);
            }
            b'}' => {
                if braces == 0 {
                    return Err(Error::Syntax("unbalanced interpolation expression".into()));
                }
                braces -= 1;
                *position += 1;
            }
            b'(' => {
                parentheses += 1;
                *position += 1;
            }
            b')' => {
                if parentheses == 0 {
                    return Err(Error::Syntax("unbalanced interpolation expression".into()));
                }
                parentheses -= 1;
                *position += 1;
            }
            b'[' => {
                brackets += 1;
                *position += 1;
            }
            b']' => {
                if brackets == 0 {
                    return Err(Error::Syntax("unbalanced interpolation expression".into()));
                }
                brackets -= 1;
                *position += 1;
            }
            _ => {
                *position += source[*position..]
                    .chars()
                    .next()
                    .ok_or_else(|| Error::Syntax("invalid UTF-8 interpolation".into()))?
                    .len_utf8();
            }
        }
    }
    Err(Error::Syntax("unterminated string interpolation".into()))
}

fn skip_double_string(source: &str, position: &mut usize) -> Result<(), Error> {
    *position += 1;
    while *position < source.len() {
        match source.as_bytes()[*position] {
            b'\\' => {
                *position += 1;
                if *position >= source.len() {
                    return Err(Error::Syntax("unterminated string escape".into()));
                }
                *position += 1;
            }
            b'"' => {
                *position += 1;
                return Ok(());
            }
            _ => *position += next_char_len(source, *position)?,
        }
    }
    Err(Error::Syntax("unterminated string interpolation expression".into()))
}

fn skip_indented_string(source: &str, position: &mut usize) -> Result<(), Error> {
    *position += 2;
    while *position + 1 < source.len() {
        if source.as_bytes()[*position] == b'\''
            && source.as_bytes()[*position + 1] == b'\''
        {
            *position += 2;
            return Ok(());
        }
        *position += next_char_len(source, *position)?;
    }
    Err(Error::Syntax(
        "unterminated indented string interpolation expression".into(),
    ))
}

fn next_char_len(source: &str, position: usize) -> Result<usize, Error> {
    source[position..]
        .chars()
        .next()
        .map(char::len_utf8)
        .ok_or_else(|| Error::Syntax("invalid UTF-8 interpolation".into()))
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    depth: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self::new_at_depth(tokens, 0)
    }

    fn new_at_depth(tokens: Vec<Token>, depth: usize) -> Self {
        Self {
            tokens,
            position: 0,
            depth,
        }
    }

    fn with_depth<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if self.depth >= MAX_EVAL_DEPTH {
            return Err(Error::ResourceLimit(format!(
                "foreign flake parser exceeded {MAX_EVAL_DEPTH} nested expressions"
            )));
        }
        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;
        result
    }

    fn parse(mut self) -> Result<Expr, Error> {
        let expression = self.expression()?;
        if !matches!(self.peek(), Token::End) {
            return Err(Error::Syntax("trailing expression".into()));
        }
        Ok(expression)
    }

    fn expression(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.expression_inner())
    }

    fn expression_inner(&mut self) -> Result<Expr, Error> {
        if let Some((pattern, body)) = self.try_lambda()? {
            return Ok(Expr::Lambda(pattern, Box::new(body)));
        }
        self.equality()
    }

    fn try_lambda(&mut self) -> Result<Option<(Pattern, Expr)>, Error> {
        self.with_depth(|parser| parser.try_lambda_inner())
    }

    fn try_lambda_inner(&mut self) -> Result<Option<(Pattern, Expr)>, Error> {
        let saved = self.position;
        if let Token::Identifier(name) = self.peek().clone() {
            self.position += 1;
            if self.take_if_colon() {
                return Ok(Some((Pattern::Name(name), self.expression()?)));
            }
        }
        self.position = saved;
        if !matches!(self.peek(), Token::LeftBrace) {
            return Ok(None);
        }
        self.position += 1;
        let mut fields = Vec::new();
        while !matches!(self.peek(), Token::RightBrace) {
            if matches!(self.peek(), Token::Ellipsis) {
                self.position += 1;
            } else {
                let Token::Identifier(name) = self.bump() else {
                    self.position = saved;
                    return Ok(None);
                };
                let default = if matches!(self.peek(), Token::Question) {
                    self.position += 1;
                    Some(self.equality()?)
                } else {
                    None
                };
                fields.push((name, default));
            }
            if matches!(self.peek(), Token::Comma) {
                self.position += 1;
            } else if !matches!(self.peek(), Token::RightBrace) {
                self.position = saved;
                return Ok(None);
            }
        }
        self.position += 1;
        if !self.take_if_colon() {
            self.position = saved;
            return Ok(None);
        }
        Ok(Some((Pattern::Attrs(fields), self.expression()?)))
    }

    fn equality(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.equality_inner())
    }

    fn equality_inner(&mut self) -> Result<Expr, Error> {
        let mut expression = self.merge()?;
        loop {
            let equal = match self.peek() {
                Token::EqualEqual => Some(true),
                Token::NotEqual => Some(false),
                _ => None,
            };
            let Some(equal) = equal else { return Ok(expression) };
            self.position += 1;
            expression = Expr::Equal(
                Box::new(expression),
                Box::new(self.merge()?),
                equal,
            );
        }
    }

    fn merge(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.merge_inner())
    }

    fn merge_inner(&mut self) -> Result<Expr, Error> {
        let mut expression = self.application()?;
        while matches!(self.peek(), Token::Merge) {
            self.position += 1;
            expression = Expr::Merge(Box::new(expression), Box::new(self.application()?));
        }
        Ok(expression)
    }

    fn application(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.application_inner())
    }

    fn application_inner(&mut self) -> Result<Expr, Error> {
        let mut expression = self.selection()?;
        while self.starts_application() {
            expression = Expr::Apply(Box::new(expression), Box::new(self.selection()?));
        }
        Ok(expression)
    }

    fn selection(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.selection_inner())
    }

    fn selection_inner(&mut self) -> Result<Expr, Error> {
        let mut expression = self.atom()?;
        while matches!(self.peek(), Token::Dot) {
            self.position += 1;
            let Token::Identifier(field) = self.bump() else {
                return Err(Error::Syntax("field selector requires an identifier".into()));
            };
            expression = Expr::Select(Box::new(expression), field);
        }
        Ok(expression)
    }

    fn atom(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.atom_inner())
    }

    fn atom_inner(&mut self) -> Result<Expr, Error> {
        match self.bump() {
            Token::String(value) => Ok(Expr::String(value)),
            Token::StringContext(parts) => self.string_context(parts),
            Token::Path(value) => Ok(Expr::Path(value)),
            Token::Integer(value) => Ok(Expr::Integer(value)),
            Token::Identifier(_name) if _name == "true" => Ok(Expr::Bool(true)),
            Token::Identifier(_name) if _name == "false" => Ok(Expr::Bool(false)),
            Token::Identifier(_name) if _name == "null" => Ok(Expr::Null),
            Token::Identifier(_name) if _name == "let" => self.let_expression(),
            Token::Identifier(_name) if _name == "with" => self.with_expression(),
            Token::Identifier(_name) if _name == "if" => self.if_expression(),
            Token::Identifier(name) => Ok(Expr::Identifier(name)),
            Token::LeftBrace => self.attrset(),
            Token::LeftBracket => self.list(),
            Token::LeftParen => {
                let expression = self.expression()?;
                self.expect_right_paren()?;
                Ok(expression)
            }
            token => Err(Error::Syntax(format!("unexpected token {}", token_name(&token)))),
        }
    }

    fn string_context(&self, parts: Vec<StringTokenPart>) -> Result<Expr, Error> {
        let mut parser = Parser::new_at_depth(Vec::new(), self.depth);
        parser.string_context_inner(parts)
    }

    fn string_context_inner(&mut self, parts: Vec<StringTokenPart>) -> Result<Expr, Error> {
        let mut expressions = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                StringTokenPart::Literal(value) => {
                    expressions.push(StringPart::Literal(value));
                }
                StringTokenPart::Expression(source) => {
                    let tokens = Lexer::new(&source).tokenize()?;
                    let expression = Parser::new_at_depth(tokens, self.depth).parse()?;
                    expressions.push(StringPart::Expression(Box::new(expression)));
                }
            }
        }
        Ok(Expr::StringContext(expressions))
    }

    fn attrset(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.attrset_inner())
    }

    fn attrset_inner(&mut self) -> Result<Expr, Error> {
        let mut fields = Vec::new();
        while !matches!(self.peek(), Token::RightBrace) {
            let key = self.key_path()?;
            self.expect_equal()?;
            fields.push((key, self.expression()?));
            if matches!(self.peek(), Token::Semicolon | Token::Comma) {
                self.position += 1;
            } else if !matches!(self.peek(), Token::RightBrace) {
                return Err(Error::Syntax("attribute values require `;`".into()));
            }
        }
        self.position += 1;
        Ok(Expr::AttrSet(fields))
    }

    fn list(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.list_inner())
    }

    fn list_inner(&mut self) -> Result<Expr, Error> {
        let mut values = Vec::new();
        while !matches!(self.peek(), Token::RightBracket) {
            values.push(self.selection()?);
            if matches!(self.peek(), Token::Comma) {
                self.position += 1;
            }
        }
        self.position += 1;
        Ok(Expr::List(values))
    }

    fn let_expression(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.let_expression_inner())
    }

    fn let_expression_inner(&mut self) -> Result<Expr, Error> {
        let mut bindings = Vec::new();
        while !self.word("in") {
            let Token::Identifier(name) = self.bump() else {
                return Err(Error::Syntax("let binding requires a name".into()));
            };
            self.expect_equal()?;
            bindings.push((name, self.expression()?));
            if !matches!(self.peek(), Token::Semicolon) {
                return Err(Error::Syntax("let binding requires `;`".into()));
            }
            self.position += 1;
        }
        self.position += 1;
        Ok(Expr::Let(bindings, Box::new(self.expression()?)))
    }

    fn with_expression(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.with_expression_inner())
    }

    fn with_expression_inner(&mut self) -> Result<Expr, Error> {
        let scope = self.expression()?;
        if !matches!(self.peek(), Token::Semicolon) {
            return Err(Error::Syntax("with expression requires `;`".into()));
        }
        self.position += 1;
        Ok(Expr::With(Box::new(scope), Box::new(self.expression()?)))
    }

    fn if_expression(&mut self) -> Result<Expr, Error> {
        self.with_depth(|parser| parser.if_expression_inner())
    }

    fn if_expression_inner(&mut self) -> Result<Expr, Error> {
        let condition = self.expression()?;
        if !self.word("then") {
            return Err(Error::Syntax("if expression requires `then`".into()));
        }
        self.position += 1;
        let when_true = self.expression()?;
        if !self.word("else") {
            return Err(Error::Syntax("if expression requires `else`".into()));
        }
        self.position += 1;
        Ok(Expr::If(
            Box::new(condition),
            Box::new(when_true),
            Box::new(self.expression()?),
        ))
    }

    fn key_path(&mut self) -> Result<String, Error> {
        let Token::Identifier(mut key) = self.bump() else {
            return Err(Error::Syntax("attribute key requires an identifier".into()));
        };
        while matches!(self.peek(), Token::Dot) {
            self.position += 1;
            let Token::Identifier(part) = self.bump() else {
                return Err(Error::Syntax("attribute key path requires an identifier".into()));
            };
            key.push('.');
            key.push_str(&part);
        }
        Ok(key)
    }

    fn starts_application(&self) -> bool {
        match self.peek() {
            Token::Identifier(name) => !matches!(name.as_str(), "in" | "then" | "else"),
            Token::String(_)
            | Token::StringContext(_)
            | Token::Path(_)
            | Token::Integer(_)
            | Token::LeftBrace
            | Token::LeftBracket => true,
            Token::LeftParen => true,
            _ => false,
        }
    }

    fn word(&self, expected: &str) -> bool {
        matches!(self.peek(), Token::Identifier(value) if value == expected)
    }

    fn take_if_colon(&mut self) -> bool {
        if matches!(self.peek(), Token::Colon) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect_equal(&mut self) -> Result<(), Error> {
        if matches!(self.peek(), Token::Equal) {
            self.position += 1;
            Ok(())
        } else {
            Err(Error::Syntax("expected `=`".into()))
        }
    }

    fn expect_right_paren(&mut self) -> Result<(), Error> {
        if matches!(self.peek(), Token::RightParen) {
            self.position += 1;
            Ok(())
        } else {
            Err(Error::Syntax("expected `)`".into()))
        }
    }

    fn bump(&mut self) -> Token {
        if self.position >= self.tokens.len() {
            return Token::End;
        }
        let token = self.tokens[self.position].clone();
        self.position += 1;
        token
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.position)
            .or_else(|| self.tokens.last())
            .expect("lexer always appends an end token")
    }
}

fn token_name(token: &Token) -> &'static str {
    match token {
        Token::Identifier(_) => "identifier",
        Token::String(_) => "string",
        Token::StringContext(_) => "string",
        Token::Path(_) => "path",
        Token::Integer(_) => "integer",
        Token::LeftBrace => "`{`",
        Token::RightBrace => "`}`",
        Token::LeftBracket => "`[`",
        Token::RightBracket => "`]`",
        Token::LeftParen => "`(`",
        Token::RightParen => "`)`",
        Token::Equal => "`=`",
        Token::EqualEqual => "`==`",
        Token::NotEqual => "`!=`",
        Token::Merge => "`//`",
        Token::Colon => "`:`",
        Token::Semicolon => "`;`",
        Token::Dot => "`.`",
        Token::Comma => "`,`",
        Token::Question => "`?`",
        Token::Ellipsis => "`...`",
        Token::End => "end of input",
    }
}

#[derive(Clone)]
struct Thunk(Rc<RefCell<ThunkState>>);

enum ThunkState {
    Unevaluated {
        expression: Expr,
        environment: Weak<RefCell<EnvironmentFrame>>,
    },
    Evaluating,
    Evaluated(Value),
    Failed(String),
}

impl Thunk {
    fn expression(expression: Expr, environment: &Environment) -> Self {
        Self(Rc::new(RefCell::new(ThunkState::Unevaluated {
            expression,
            environment: Rc::downgrade(environment),
        })))
    }

    fn value(value: Value) -> Self {
        Self(Rc::new(RefCell::new(ThunkState::Evaluated(value))))
    }

    fn force(&self) -> Result<Value, Error> {
        let state = mem::replace(&mut *self.0.borrow_mut(), ThunkState::Evaluating);
        let (expression, environment) = match state {
            ThunkState::Unevaluated {
                expression,
                environment,
            } => (expression, environment),
            ThunkState::Evaluating => return Err(Error::Cycle),
            ThunkState::Evaluated(value) => {
                *self.0.borrow_mut() = ThunkState::Evaluated(value.clone());
                return Ok(value);
            }
            ThunkState::Failed(reason) => {
                *self.0.borrow_mut() = ThunkState::Failed(reason.clone());
                return Err(Error::Invalid(reason));
            }
        };
        let Some(environment) = environment.upgrade() else {
            let error = Error::Invalid("foreign flake scope expired".into());
            *self.0.borrow_mut() = ThunkState::Failed(error.to_string());
            return Err(error);
        };
        let Some(arena) = environment.borrow().arena.upgrade() else {
            let error = Error::Invalid("foreign flake evaluation arena expired".into());
            *self.0.borrow_mut() = ThunkState::Failed(error.to_string());
            return Err(error);
        };
        let result = evaluate_expr(&expression, &environment, 0, &arena);
        match result {
            Ok(value) => {
                *self.0.borrow_mut() = ThunkState::Evaluated(value.clone());
                Ok(value)
            }
            Err(error) => {
                *self.0.borrow_mut() = ThunkState::Failed(error.to_string());
                Err(error)
            }
        }
    }
}

type Environment = Rc<RefCell<EnvironmentFrame>>;

struct EvaluationArena {
    environments: RefCell<Vec<Environment>>,
    system: String,
    import_authority: Option<Rc<ImportAuthority>>,
    imports: Cell<usize>,
    active_imports: RefCell<Vec<String>>,
}

type ImportAuthority = dyn Fn(&str) -> Result<String, String>;

impl EvaluationArena {
    fn new(system: &str, import_authority: Option<Rc<ImportAuthority>>) -> Rc<Self> {
        Rc::new(Self {
            environments: RefCell::new(Vec::new()),
            system: system.to_string(),
            import_authority,
            imports: Cell::new(0),
            active_imports: RefCell::new(Vec::new()),
        })
    }

    fn register(&self, environment: Environment) {
        self.environments.borrow_mut().push(environment);
    }
}

struct EnvironmentFrame {
    parent: Option<Environment>,
    bindings: BTreeMap<String, Thunk>,
    scopes: Vec<Thunk>,
    fuel: Rc<Cell<usize>>,
    arena: Weak<EvaluationArena>,
    base_path: String,
}

impl EnvironmentFrame {
    fn root(arena: &Rc<EvaluationArena>, base_path: &str) -> Environment {
        let environment = Rc::new(RefCell::new(Self {
            parent: None,
            bindings: BTreeMap::new(),
            scopes: Vec::new(),
            fuel: Rc::new(Cell::new(0)),
            arena: Rc::downgrade(arena),
            base_path: base_path.to_string(),
        }));
        arena.register(environment.clone());
        {
            let mut frame = environment.borrow_mut();
            let _ = frame.bindings.insert(
                "pkgs".into(),
                Thunk::value(Value::PackageNamespace(String::new())),
            );
            let _ = frame.bindings.insert(
                "legacyPackages".into(),
                Thunk::value(Value::PackageNamespace(String::new())),
            );
            let _ = frame.bindings.insert(
                "system".into(),
                Thunk::value(Value::String(arena.system.clone())),
            );
            let _ = frame.bindings.insert(
                "builtins".into(),
                Thunk::value(Value::BuiltinsNamespace(arena.system.clone())),
            );
            let _ = frame.bindings.insert(
                "mkShell".into(),
                Thunk::value(Value::Native(NativeFunction::MkShell)),
            );
            let _ = frame.bindings.insert(
                "import".into(),
                Thunk::value(Value::Native(NativeFunction::Import)),
            );
        }
        environment
    }

    fn child(parent: &Environment) -> Result<Environment, Error> {
        let (fuel, arena, base_path) = {
            let parent = parent.borrow();
            let Some(arena) = parent.arena.upgrade() else {
                return Err(Error::Invalid("foreign flake evaluation arena expired".into()));
            };
            (parent.fuel.clone(), arena, parent.base_path.clone())
        };
        let environment = Rc::new(RefCell::new(Self {
            parent: Some(parent.clone()),
            bindings: BTreeMap::new(),
            scopes: Vec::new(),
            fuel,
            arena: Rc::downgrade(&arena),
            base_path,
        }));
        arena.register(environment.clone());
        Ok(environment)
    }
}

#[derive(Clone)]
enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    String(String),
    StringContext { value: String, contexts: Vec<String> },
    Path(String),
    Package(String),
    PackageNamespace(String),
    PackageOverlay(String, Rc<RefCell<BTreeMap<String, Thunk>>>),
    BuiltinsNamespace(String),
    LibraryNamespace,
    List(Vec<Thunk>),
    AttrSet(BTreeMap<String, Thunk>),
    Derivation(Rc<DerivationValue>),
    Function(Rc<FunctionValue>),
    Native(NativeFunction),
}

#[derive(Clone)]
struct DerivationValue {
    fields: BTreeMap<String, Thunk>,
    evaluation: Option<DerivationEvaluation>,
}

#[derive(Clone)]
struct FunctionValue {
    pattern: Pattern,
    body: Expr,
    environment: Weak<RefCell<EnvironmentFrame>>,
}

#[derive(Clone)]
enum NativeFunction {
    MkShell,
    Import,
    ImportPackage(Box<Value>),
    ExtendPackage(Box<Value>),
    Unsupported(&'static str),
    ToString,
    HasContext,
    Builtin(Builtin),
    Partial {
        operation: NativeOperation,
        arguments: Vec<Value>,
    },
    Derivation {
        strict: bool,
    },
}

#[derive(Clone, Copy)]
enum Builtin {
    AttrNames,
    AttrValues,
    GetAttr,
    HasAttr,
    IsAttrs,
    IsBool,
    IsFunction,
    IsInt,
    IsList,
    IsNull,
    IsPath,
    IsString,
    Length,
    Head,
    Tail,
    ConcatLists,
    ConcatStringsSep,
    Map,
    Filter,
    ReplaceStrings,
    Substring,
    BaseNameOf,
    DirOf,
    StringLength,
    ToJSON,
    FromJSON,
    StorePath,
    Throw,
}

#[derive(Clone, Copy)]
enum NativeOperation {
    GetAttr,
    HasAttr,
    Map,
    Filter,
    ConcatStringsSep,
    ReplaceStrings,
    Substring,
}

pub(super) fn evaluate_devshell(
    source: &str,
    system: &str,
    import_authority: Option<Rc<ImportAuthority>>,
) -> Result<DevShellEvaluation, EvaluationError> {
    let tokens = Lexer::new(source).tokenize().map_err(Error::public)?;
    let expression = Parser::new(tokens).parse().map_err(Error::public)?;
    let arena = EvaluationArena::new(system, import_authority);
    let environment = EnvironmentFrame::root(&arena, "");
    let root = evaluate_expr(&expression, &environment, 0, &arena).map_err(Error::public)?;
    let shell = resolve_shell(root.clone(), system, &arena).map_err(Error::public)?;
    project_shell(shell, system).map_err(Error::public)
}

pub(super) fn evaluate_derivation(
    source: &str,
    system: &str,
) -> Result<DerivationEvaluation, EvaluationError> {
    let tokens = Lexer::new(source).tokenize().map_err(Error::public)?;
    let expression = Parser::new(tokens).parse().map_err(Error::public)?;
    let arena = EvaluationArena::new(system, None);
    let environment = EnvironmentFrame::root(&arena, "");
    let root = evaluate_expr(&expression, &environment, 0, &arena).map_err(Error::public)?;
    derivation_evaluation(root).map_err(Error::public)
}

pub(super) fn evaluate_derivation_output(
    source: &str,
    system: &str,
    attribute: &str,
) -> Result<DerivationEvaluation, EvaluationError> {
    let tokens = Lexer::new(source).tokenize().map_err(Error::public)?;
    let expression = Parser::new(tokens).parse().map_err(Error::public)?;
    let arena = EvaluationArena::new(system, None);
    let environment = EnvironmentFrame::root(&arena, "");
    let root = evaluate_expr(&expression, &environment, 0, &arena).map_err(Error::public)?;
    let path = vec![
        "packages".to_string(),
        system.to_string(),
        attribute.to_string(),
    ];
    let output = match resolve_path(root.clone(), &path, system, &arena) {
        Ok(output) => output,
        Err(Error::Missing(_)) => resolve_path(
            root,
            &["legacyPackages".into(), system.to_string(), attribute.to_string()],
            system,
            &arena,
        )
        .map_err(Error::public)?,
        Err(error) => return Err(error.public()),
    };
    derivation_evaluation(output).map_err(Error::public)
}

fn derivation_evaluation(value: Value) -> Result<DerivationEvaluation, Error> {
    let Value::Derivation(derivation) = value else {
        return Err(Error::Unsupported(format!(
            "derivation evaluation must return a derivation, got {}",
            value_name(&value)
        )));
    };
    derivation
        .evaluation
        .clone()
        .map_or_else(|| derivation_from_fields(&derivation.fields), Ok)
}

fn evaluate_expr(
    expression: &Expr,
    environment: &Environment,
    depth: usize,
    arena: &Rc<EvaluationArena>,
) -> Result<Value, Error> {
    let fuel = environment.borrow().fuel.clone();
    let spent = fuel.get();
    if spent >= MAX_EVAL_DEPTH {
        return Err(Error::ResourceLimit(format!(
            "foreign flake evaluation exceeded {MAX_EVAL_DEPTH} expression steps"
        )));
    }
    fuel.set(spent + 1);
    if depth > MAX_EVAL_DEPTH {
        return Err(Error::ResourceLimit(format!(
            "foreign flake evaluation exceeded {MAX_EVAL_DEPTH} nested expressions"
        )));
    }
    match expression {
        Expr::String(value) => Ok(Value::String(value.clone())),
        Expr::StringContext(parts) => evaluate_string_context(parts, environment, depth, arena),
        Expr::Path(raw) => {
            let base_path = environment.borrow().base_path.clone();
            let path = resolve_path_literal(raw, &base_path)?;
            if arena.import_authority.is_none() {
                return Err(Error::Unsupported(
                    "path values require explicit project-root authority".into(),
                ));
            }
            Ok(Value::Path(path))
        }
        Expr::Integer(value) => Ok(Value::Integer(*value)),
        Expr::Bool(value) => Ok(Value::Bool(*value)),
        Expr::Null => Ok(Value::Null),
        Expr::Identifier(name) => lookup(environment, name)?.force(),
        Expr::AttrSet(entries) => {
            let mut fields = BTreeMap::new();
            for (name, value) in entries {
                if fields
                    .insert(name.clone(), Thunk::expression(value.clone(), environment))
                    .is_some()
                {
                    return Err(Error::Invalid(format!("duplicate attribute `{name}`")));
                }
            }
            Ok(Value::AttrSet(fields))
        }
        Expr::List(values) => Ok(Value::List(
            values
                .iter()
                .cloned()
                .map(|value| Thunk::expression(value, environment))
                .collect(),
        )),
        Expr::Lambda(pattern, body) => Ok(Value::Function(Rc::new(FunctionValue {
            pattern: pattern.clone(),
            body: (**body).clone(),
            environment: Rc::downgrade(environment),
        }))),
        Expr::Apply(function, argument) => {
            let function = evaluate_expr(function, environment, depth + 1, arena)?;
            apply(
                function,
                Thunk::expression((**argument).clone(), environment),
                arena,
            )
        }
        Expr::Select(value, field) => {
            let value = evaluate_expr(value, environment, depth + 1, arena)?;
            select(value, field)?.force()
        }
        Expr::Let(bindings, body) => {
            let child = EnvironmentFrame::child(environment)?;
            for (name, value) in bindings {
                if child.borrow_mut().bindings.insert(
                    name.clone(),
                    Thunk::expression(value.clone(), &child),
                ).is_some() {
                    return Err(Error::Invalid(format!("duplicate let binding `{name}`")));
                }
            }
            evaluate_expr(body, &child, depth + 1, arena)
        }
        Expr::With(scope, body) => {
            let child = EnvironmentFrame::child(environment)?;
            child
                .borrow_mut()
                .scopes
                .push(Thunk::expression((**scope).clone(), environment));
            evaluate_expr(body, &child, depth + 1, arena)
        }
        Expr::If(condition, when_true, when_false) => {
            let condition = evaluate_expr(condition, environment, depth + 1, arena)?;
            match condition {
                Value::Bool(true) => evaluate_expr(when_true, environment, depth + 1, arena),
                Value::Bool(false) => evaluate_expr(when_false, environment, depth + 1, arena),
                value => Err(Error::Type {
                    expected: "boolean",
                    actual: value_name(&value),
                }),
            }
        }
        Expr::Merge(left, right) => {
            let left = evaluate_expr(left, environment, depth + 1, arena)?;
            let right = evaluate_expr(right, environment, depth + 1, arena)?;
            merge(left, right)
        }
        Expr::Equal(left, right, positive) => {
            let left = evaluate_expr(left, environment, depth + 1, arena)?;
            let right = evaluate_expr(right, environment, depth + 1, arena)?;
            Ok(Value::Bool(values_equal(&left, &right) == *positive))
        }
    }
}

fn evaluate_string_context(
    parts: &[StringPart],
    environment: &Environment,
    depth: usize,
    arena: &Rc<EvaluationArena>,
) -> Result<Value, Error> {
    let mut value = String::new();
    let mut contexts = Vec::new();
    for part in parts {
        let (text, part_contexts) = match part {
            StringPart::Literal(text) => (text.clone(), Vec::new()),
            StringPart::Expression(expression) => {
                let value = evaluate_expr(expression, environment, depth + 1, arena)?;
                stringify_value(&value)?
            }
        };
        if value.len() + text.len() > MAX_STRING_BYTES {
            return Err(Error::ResourceLimit(format!(
                "foreign flake string exceeds {MAX_STRING_BYTES} bytes"
            )));
        }
        value.push_str(&text);
        for context in part_contexts {
            if !contexts.iter().any(|existing| existing == &context) {
                contexts.push(context);
            }
        }
    }
    if contexts.is_empty() {
        Ok(Value::String(value))
    } else {
        Ok(Value::StringContext { value, contexts })
    }
}

fn stringify_value(value: &Value) -> Result<(String, Vec<String>), Error> {
    match value {
        Value::String(value) => Ok((value.clone(), Vec::new())),
        Value::StringContext { value, contexts } => Ok((value.clone(), contexts.clone())),
        Value::Integer(value) => Ok((value.to_string(), Vec::new())),
        Value::Package(name) => Ok((name.clone(), vec![format!("package:{name}")])),
        Value::Path(path) => Ok((path.clone(), vec![format!("path:{path}")])),
        value => Err(Error::Unsupported(format!(
            "value of type {} cannot be coerced to a string",
            value_name(value)
        ))),
    }
}

fn resolve_path_literal(raw: &str, base: &str) -> Result<String, Error> {
    if raw.len() > MAX_PATH_BYTES {
        return Err(Error::ResourceLimit(format!(
            "foreign flake path exceeds {MAX_PATH_BYTES} bytes"
        )));
    }
    if raw.starts_with('/') {
        return Err(Error::Unsupported(
            "absolute paths require explicit project-root authority".into(),
        ));
    }
    if raw.contains('\\') || raw.contains('\0') {
        return Err(Error::Unsupported(
            "path values must use bounded relative UTF-8 paths".into(),
        ));
    }
    if raw.contains(':') {
        return Err(Error::Unsupported(
            "URI paths require explicit fetch authority".into(),
        ));
    }

    let mut components = base
        .split('/')
        .filter(|component| !component.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    for component in raw.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(Error::Unsupported(
                        "path escapes the flake project-root authority".into(),
                    ));
                }
            }
            component if component.contains('\0') => {
                return Err(Error::Unsupported("path contains NUL".into()));
            }
            component => components.push(component.to_string()),
        }
    }
    if components.is_empty() {
        return Err(Error::Invalid("path value names the flake root directory".into()));
    }
    let path = components.join("/");
    if path.len() > MAX_PATH_BYTES {
        return Err(Error::ResourceLimit(format!(
            "foreign flake path exceeds {MAX_PATH_BYTES} bytes"
        )));
    }
    Ok(path)
}

fn imported_parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn evaluate_import(path: &str, arena: &Rc<EvaluationArena>) -> Result<Value, Error> {
    let Some(authority) = arena.import_authority.clone() else {
        return Err(Error::Unsupported(
            "import requires explicit project-root authority".into(),
        ));
    };
    let imports = arena.imports.get();
    if imports >= MAX_IMPORTS {
        return Err(Error::ResourceLimit(format!(
            "foreign flake imports exceed {MAX_IMPORTS} files"
        )));
    }
    if arena
        .active_imports
        .borrow()
        .iter()
        .any(|active| active == path)
    {
        return Err(Error::Cycle);
    }
    arena.imports.set(imports + 1);
    arena.active_imports.borrow_mut().push(path.to_string());
    let result = (|| {
        let source = authority(path).map_err(|reason| {
            Error::Invalid(format!("could not import `{path}`: {reason}"))
        })?;
        if source.len() > MAX_STRING_BYTES {
            return Err(Error::ResourceLimit(format!(
                "imported `{path}` exceeds {MAX_STRING_BYTES} bytes"
            )));
        }
        let tokens = Lexer::new(&source).tokenize()?;
        let expression = Parser::new(tokens).parse()?;
        let environment = EnvironmentFrame::root(arena, imported_parent(path));
        evaluate_expr(&expression, &environment, 0, arena)
    })();
    arena.active_imports.borrow_mut().pop();
    result
}

fn resolve_shell(
    root: Value,
    system: &str,
    arena: &Rc<EvaluationArena>,
) -> Result<Value, Error> {
    let path = [
        "outputs".to_string(),
        "devShells".to_string(),
        system.to_string(),
        "default".to_string(),
    ];
    match resolve_path(root.clone(), &path, system, arena) {
        Ok(value) => Ok(value),
        Err(Error::Missing(_)) => match resolve_path(
            root,
            &["devShells".into(), system.to_string(), "default".into()],
            system,
            arena,
        ) {
            Err(Error::Missing(_)) => Err(Error::Unsupported(
                "no supported `devShell` or `mkShell` output was found".into(),
            )),
            result => result,
        },
        Err(error) => Err(error),
    }
}

fn resolve_path(
    mut value: Value,
    path: &[String],
    system: &str,
    arena: &Rc<EvaluationArena>,
) -> Result<Value, Error> {
    for field in path {
        if matches!(value, Value::Function(_)) {
            value = apply(value, Thunk::value(flake_arguments(system)), arena)?;
        }
        value = select(value, field)?.force()?;
    }
    Ok(value)
}

fn project_shell(shell: Value, system: &str) -> Result<DevShellEvaluation, Error> {
    let mut packages = Vec::new();
    for field in ["packages", "buildInputs", "nativeBuildInputs"] {
        let Some(value) = try_select(shell.clone(), field)? else {
            continue;
        };
        let value = match value.force() {
            Ok(value) => value,
            Err(Error::Missing(_)) => {
                return Err(Error::Unsupported(format!(
                    "`{field}` must be a literal package list"
                )))
            }
            Err(error) => return Err(error),
        };
        let Value::List(values) = value else {
            return Err(Error::Unsupported(format!(
                "`{field}` must be a literal package list"
            )));
        };
        for item in values {
            if packages.len() >= MAX_DEV_SHELL_PACKAGES {
                return Err(Error::ResourceLimit(format!(
                    "devShell has more than {MAX_DEV_SHELL_PACKAGES} packages"
                )));
            }
            let item = item.force()?;
            let name = match item {
                Value::Package(name) | Value::String(name) => package_name(&name)?,
                Value::StringContext { value, contexts } => {
                    if contexts.iter().any(|context| context.starts_with("path:")) {
                        return Err(Error::Unsupported(
                            "path string contexts are not devShell packages".into(),
                        ));
                    }
                    package_name(&value)?
                }
                value => {
                    return Err(Error::Unsupported(format!(
                        "package expression has type {}",
                        value_name(&value)
                    )))
                }
            };
            packages.push(name);
        }
    }
    packages.sort();
    packages.dedup();

    let mut unsupported = Vec::new();
    if let Some(hook) = try_select(shell, "shellHook")? {
        match hook.force()? {
            Value::String(value) if value.trim().is_empty() => {}
            Value::StringContext { value, .. } if value.trim().is_empty() => {}
            Value::String(_) | Value::StringContext { .. } | Value::Integer(_) | Value::Bool(_) | Value::Null => {
                unsupported.push("shellHook".into())
            }
            _ => unsupported.push("shellHook".into()),
        }
    }
    Ok(DevShellEvaluation {
        system: system.to_string(),
        packages,
        unsupported,
    })
}

fn package_name(raw: &str) -> Result<String, Error> {
    let name = raw
        .rsplit(['.', '/'])
        .next()
        .unwrap_or(raw);
    if name.is_empty() || name.len() > MAX_PACKAGE_NAME_BYTES {
        return Err(Error::Unsupported(format!("invalid package name `{raw}`")));
    }
    Ok(name.to_string())
}

fn lookup(environment: &Environment, name: &str) -> Result<Thunk, Error> {
    let mut current = Some(environment.clone());
    while let Some(frame) = current {
        let (binding, scopes, parent) = {
            let frame = frame.borrow();
            (
                frame.bindings.get(name).cloned(),
                frame.scopes.clone(),
                frame.parent.clone(),
            )
        };
        if let Some(binding) = binding {
            return Ok(binding);
        }
        for scope in scopes.into_iter().rev() {
            if let Some(binding) = try_select(scope.force()?, name)? {
                return Ok(binding);
            }
        }
        current = parent;
    }
    Err(Error::Missing(name.to_string()))
}

fn select(value: Value, field: &str) -> Result<Thunk, Error> {
    try_select(value, field)?.ok_or_else(|| Error::Missing(field.to_string()))
}

fn package_field(
    prefix: &str,
    overrides: Option<&BTreeMap<String, Thunk>>,
    field: &str,
) -> Result<Option<Thunk>, Error> {
    if let Some(overrides) = overrides {
        if let Some(value) = overrides.get(field) {
            return Ok(Some(value.clone()));
        }
    }
    if matches!(field, "pkgsCross" | "crossSystem") {
        return Err(Error::Unsupported(
            "cross-system packages require an explicit target and provider authority".into(),
        ));
    }
    if field == "lib" {
        Ok(Some(Thunk::value(Value::LibraryNamespace)))
    } else if field == "mkShell" {
        Ok(Some(Thunk::value(Value::Native(NativeFunction::MkShell))))
    } else if field == "extend" {
        let base = match overrides {
            Some(fields) => Value::PackageOverlay(
                prefix.to_string(),
                Rc::new(RefCell::new(fields.clone())),
            ),
            None => Value::PackageNamespace(prefix.to_string()),
        };
        Ok(Some(Thunk::value(Value::Native(
            NativeFunction::ExtendPackage(Box::new(base)),
        ))))
    } else {
        let name = if prefix.is_empty() {
            field.to_string()
        } else {
            format!("{prefix}.{field}")
        };
        Ok(Some(Thunk::value(Value::Package(name))))
    }
}

fn try_select(value: Value, field: &str) -> Result<Option<Thunk>, Error> {
    match value {
        Value::AttrSet(fields) => Ok(attr_field(&fields, field)),
        Value::PackageNamespace(prefix) => package_field(&prefix, None, field),
        Value::PackageOverlay(prefix, fields) => {
            let fields = fields.borrow();
            package_field(&prefix, Some(&fields), field)
        }
        Value::Package(prefix) => Ok(Some(Thunk::value(Value::Package(format!(
            "{prefix}.{field}"
        ))))),
        Value::Derivation(derivation) => {
            let value = match field {
                "type" => Value::String("derivation".into()),
                "drvPath" | "outPath" | "out" => {
                    return Err(Error::Unsupported(
                        "derivation store paths are assigned by Jetpack's private materializer"
                            .into(),
                    ))
                }
                "name" | "system" | "builder" => {
                    if let Some(evaluation) = &derivation.evaluation {
                        Value::String(match field {
                            "name" => evaluation.name.clone(),
                            "system" => evaluation.system.clone(),
                            _ => evaluation.builder.clone(),
                        })
                    } else {
                        field_value(&derivation.fields, field)?
                            .ok_or_else(|| Error::Missing(field.into()))?
                    }
                }
                "args" => {
                    if let Some(evaluation) = &derivation.evaluation {
                        Value::List(
                            evaluation
                                .args
                                .iter()
                                .cloned()
                                .map(|value| Thunk::value(Value::String(value)))
                                .collect(),
                        )
                    } else {
                        field_value(&derivation.fields, "args")?
                            .ok_or_else(|| Error::Missing("args".into()))?
                    }
                }
                _ => {
                    if let Some(evaluation) = &derivation.evaluation {
                        if let Some(value) = evaluation.env.get(field) {
                            Value::String(value.clone())
                        } else {
                            return Ok(None);
                        }
                    } else if let Some(value) = attr_field(&derivation.fields, field) {
                        return Ok(Some(value));
                    } else {
                        return Ok(None);
                    }
                }
            };
            Ok(Some(Thunk::value(value)))
        }
        Value::LibraryNamespace => Ok(None),
        Value::BuiltinsNamespace(system) => {
            let native = match field {
                "toString" => Some(NativeFunction::ToString),
                "hasContext" => Some(NativeFunction::HasContext),
                "derivation" => Some(NativeFunction::Derivation { strict: false }),
                "derivationStrict" => Some(NativeFunction::Derivation { strict: true }),
                "attrNames" => Some(NativeFunction::Builtin(Builtin::AttrNames)),
                "attrValues" => Some(NativeFunction::Builtin(Builtin::AttrValues)),
                "getAttr" => Some(NativeFunction::Builtin(Builtin::GetAttr)),
                "hasAttr" => Some(NativeFunction::Builtin(Builtin::HasAttr)),
                "isAttrs" => Some(NativeFunction::Builtin(Builtin::IsAttrs)),
                "isBool" => Some(NativeFunction::Builtin(Builtin::IsBool)),
                "isFunction" => Some(NativeFunction::Builtin(Builtin::IsFunction)),
                "isInt" => Some(NativeFunction::Builtin(Builtin::IsInt)),
                "isList" => Some(NativeFunction::Builtin(Builtin::IsList)),
                "isNull" => Some(NativeFunction::Builtin(Builtin::IsNull)),
                "isPath" => Some(NativeFunction::Builtin(Builtin::IsPath)),
                "isString" => Some(NativeFunction::Builtin(Builtin::IsString)),
                "length" => Some(NativeFunction::Builtin(Builtin::Length)),
                "head" => Some(NativeFunction::Builtin(Builtin::Head)),
                "tail" => Some(NativeFunction::Builtin(Builtin::Tail)),
                "concatLists" => Some(NativeFunction::Builtin(Builtin::ConcatLists)),
                "concatStringsSep" => {
                    Some(NativeFunction::Builtin(Builtin::ConcatStringsSep))
                }
                "map" => Some(NativeFunction::Builtin(Builtin::Map)),
                "filter" => Some(NativeFunction::Builtin(Builtin::Filter)),
                "replaceStrings" => Some(NativeFunction::Builtin(Builtin::ReplaceStrings)),
                "substring" => Some(NativeFunction::Builtin(Builtin::Substring)),
                "baseNameOf" => Some(NativeFunction::Builtin(Builtin::BaseNameOf)),
                "dirOf" => Some(NativeFunction::Builtin(Builtin::DirOf)),
                "stringLength" => Some(NativeFunction::Builtin(Builtin::StringLength)),
                "toJSON" => Some(NativeFunction::Builtin(Builtin::ToJSON)),
                "fromJSON" => Some(NativeFunction::Builtin(Builtin::FromJSON)),
                "storePath" => Some(NativeFunction::Builtin(Builtin::StorePath)),
                "throw" => Some(NativeFunction::Builtin(Builtin::Throw)),
                "fetchurl" | "fetchTarball" | "fetchTree" | "fetchGit" => Some(
                    NativeFunction::Unsupported(
                        "fixed-output fetchers require explicit fetch authority and verified bytes",
                    ),
                ),
                "getFlake" => Some(NativeFunction::Unsupported(
                    "external flakes require explicit provider authority",
                )),
                "currentSystem" => return Ok(Some(Thunk::value(Value::String(
                    system,
                )))),
                "storeDir" => return Ok(Some(Thunk::value(Value::String("/nix/store".into())))),
                _ => None,
            };
            Ok(native.map(|function| Thunk::value(Value::Native(function))))
        }
        value => Err(Error::Type {
            expected: "attribute set",
            actual: value_name(&value),
        }),
    }
}

fn attr_field(fields: &BTreeMap<String, Thunk>, field: &str) -> Option<Thunk> {
    if let Some(value) = fields.get(field) {
        return Some(value.clone());
    }
    let prefix = format!("{field}.");
    let mut nested = BTreeMap::new();
    for (name, value) in fields {
        if let Some(rest) = name.strip_prefix(&prefix) {
            let _ = nested.insert(rest.to_string(), value.clone());
        }
    }
    (!nested.is_empty()).then(|| Thunk::value(Value::AttrSet(nested)))
}

fn apply(function: Value, argument: Thunk, arena: &Rc<EvaluationArena>) -> Result<Value, Error> {
    match function {
        Value::Function(function) => {
            let Some(function_environment) = function.environment.upgrade() else {
                return Err(Error::Invalid("foreign flake function scope expired".into()));
            };
            let environment = EnvironmentFrame::child(&function_environment)?;
            match &function.pattern {
                Pattern::Name(name) => {
                    let _ = environment
                        .borrow_mut()
                        .bindings
                        .insert(name.clone(), argument);
                }
                Pattern::Attrs(fields) => {
                    let argument_value = argument.force()?;
                    let Value::AttrSet(arguments) = argument_value else {
                        return Err(Error::Type {
                            expected: "attribute set function argument",
                            actual: value_name(&argument_value),
                        });
                    };
                    for (name, default) in fields {
                        if let Some(value) = attr_field(&arguments, name) {
                            let _ = environment.borrow_mut().bindings.insert(name.clone(), value);
                        } else if let Some(default) = default {
                            let _ = environment.borrow_mut().bindings.insert(
                                name.clone(),
                                Thunk::expression(default.clone(), &environment),
                            );
                        } else {
                            return Err(Error::Missing(name.clone()));
                        }
                    }
                }
            }
            evaluate_expr(&function.body, &environment, 0, arena)
        }
        Value::Native(NativeFunction::MkShell) => {
            let value = argument.force()?;
            if matches!(value, Value::AttrSet(_)) {
                Ok(value)
            } else {
                Err(Error::Type {
                    expected: "mkShell attribute set",
                    actual: value_name(&value),
                })
            }
        }
        Value::Native(NativeFunction::Import) => {
            let value = argument.force()?;
            match value {
                Value::Path(path) => evaluate_import(&path, arena),
                Value::PackageNamespace(_) | Value::PackageOverlay(_, _) => Ok(Value::Native(
                    NativeFunction::ImportPackage(Box::new(value)),
                )),
                value => Err(Error::Type {
                    expected: "path or package-set import argument",
                    actual: value_name(&value),
                }),
            }
        }
        Value::Native(NativeFunction::ImportPackage(base)) => {
            let base = *base;
            let arguments = argument.force()?;
            let Value::AttrSet(fields) = arguments else {
                return Err(Error::Type {
                    expected: "package import attribute set",
                    actual: value_name(&arguments),
                });
            };
            let Some(overlays) = attr_field(&fields, "overlays") else {
                return Ok(base);
            };
            let Value::List(overlays) = overlays.force()? else {
                return Err(Error::Type {
                    expected: "overlay list",
                    actual: "non-list overlays",
                });
            };
            apply_overlays(base, overlays, arena)
        }
        Value::Native(NativeFunction::ExtendPackage(base)) => {
            let base = *base;
            let overlay = argument.force()?;
            apply_overlay(base, overlay, arena)
        }
        Value::Native(NativeFunction::Unsupported(reason)) => {
            Err(Error::Unsupported(reason.into()))
        }
        Value::Native(NativeFunction::ToString) => {
            let value = argument.force()?;
            let (value, contexts) = stringify_value(&value)?;
            if contexts.is_empty() {
                Ok(Value::String(value))
            } else {
                Ok(Value::StringContext { value, contexts })
            }
        }
        Value::Native(NativeFunction::HasContext) => {
            let value = argument.force()?;
            let (_, contexts) = stringify_value(&value)?;
            Ok(Value::Bool(!contexts.is_empty()))
        }
        Value::Native(NativeFunction::Derivation { strict }) => {
            derivation_from_value(argument.force()?, strict)
        }
        Value::Native(NativeFunction::Builtin(builtin)) => {
            apply_builtin(builtin, argument.force()?, arena)
        }
        Value::Native(NativeFunction::Partial {
            operation,
            mut arguments,
        }) => {
            arguments.push(argument.force()?);
            apply_partial(operation, arguments, arena)
        }
        value => Err(Error::Type {
            expected: "function",
            actual: value_name(&value),
        }),
    }
}

/// Apply one bounded Nix overlay. The evaluator keeps the package namespace
/// lazy: the overlay only replaces explicitly named attributes, while every
/// untouched package still resolves through the canonical namespace.
fn apply_overlay(
    base: Value,
    overlay: Value,
    arena: &Rc<EvaluationArena>,
) -> Result<Value, Error> {
    let (prefix, previous) = package_parts(&base)?;
    let final_fields = Rc::new(RefCell::new(previous));
    let final_view = Value::PackageOverlay(prefix.clone(), final_fields.clone());
    let first = apply(overlay, Thunk::value(final_view), arena)?;
    let result = apply(first, Thunk::value(base), arena)?;
    let Value::AttrSet(fields) = result else {
        return Err(Error::Type {
            expected: "overlay attribute set",
            actual: value_name(&result),
        });
    };
    final_fields.borrow_mut().extend(fields);
    let snapshot = final_fields.borrow().clone();
    Ok(Value::PackageOverlay(prefix, Rc::new(RefCell::new(snapshot))))
}

fn apply_overlays(
    base: Value,
    overlays: Vec<Thunk>,
    arena: &Rc<EvaluationArena>,
) -> Result<Value, Error> {
    let (prefix, previous) = package_parts(&base)?;
    let final_fields = Rc::new(RefCell::new(previous));
    let mut current = base;
    for overlay in overlays {
        let final_view = Value::PackageOverlay(prefix.clone(), final_fields.clone());
        let first = apply(overlay.force()?, Thunk::value(final_view), arena)?;
        let result = apply(first, Thunk::value(current.clone()), arena)?;
        let Value::AttrSet(fields) = result else {
            return Err(Error::Type {
                expected: "overlay attribute set",
                actual: value_name(&result),
            });
        };
        final_fields.borrow_mut().extend(fields);
        let snapshot = final_fields.borrow().clone();
        current = Value::PackageOverlay(
            prefix.clone(),
            Rc::new(RefCell::new(snapshot)),
        );
    }
    Ok(current)
}

fn package_parts(value: &Value) -> Result<(String, BTreeMap<String, Thunk>), Error> {
    match value {
        Value::PackageNamespace(prefix) => Ok((prefix.clone(), BTreeMap::new())),
        Value::PackageOverlay(prefix, fields) => Ok((prefix.clone(), fields.borrow().clone())),
        value => Err(Error::Type {
            expected: "package namespace",
            actual: value_name(value),
        }),
    }
}

fn derivation_from_value(value: Value, strict: bool) -> Result<Value, Error> {
    let Value::AttrSet(fields) = value else {
        return Err(Error::Type {
            expected: "derivation attribute set",
            actual: value_name(&value),
        });
    };
    let evaluation = if strict {
        Some(derivation_from_fields(&fields)?)
    } else {
        None
    };
    Ok(Value::Derivation(Rc::new(DerivationValue {
        fields,
        evaluation,
    })))
}

fn derivation_from_fields(fields: &BTreeMap<String, Thunk>) -> Result<DerivationEvaluation, Error> {
    let name = required_string_field(fields, "name")?;
    validate_derivation_name(&name)?;
    let system = required_string_field(fields, "system")?;
    let builder = required_string_field(fields, "builder")?;
    let mut input_sources = Vec::new();
    let args = match field_value(fields, "args")? {
        Some(Value::List(values)) => {
            if values.len() > MAX_DERIVATION_ARGS {
                return Err(Error::ResourceLimit(format!(
                    "derivation has more than {MAX_DERIVATION_ARGS} arguments"
                )));
            }
            values
                .into_iter()
                .map(|value| {
                    let value = value.force()?;
                    let (text, contexts) = stringify_value(&value)?;
                    record_derivation_contexts(&contexts, &mut input_sources)?;
                    Ok(text)
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        Some(value) => {
            return Err(Error::Type {
                expected: "derivation argument list",
                actual: value_name(&value),
            })
        }
        None => Vec::new(),
    };

    let output_names = match field_value(fields, "outputs")? {
        Some(Value::List(values)) => values
            .into_iter()
            .map(|value| string_for_derivation(value.force()?))
            .collect::<Result<Vec<_>, _>>()?,
        Some(value) => {
            return Err(Error::Type {
                expected: "derivation output list",
                actual: value_name(&value),
            })
        }
        None => vec!["out".into()],
    };
    if output_names.is_empty() || output_names.len() > MAX_DERIVATION_OUTPUTS {
        return Err(Error::ResourceLimit(format!(
            "derivation must declare between one and {MAX_DERIVATION_OUTPUTS} outputs"
        )));
    }
    let mut unique_outputs = BTreeMap::new();
    for output_name in &output_names {
        validate_derivation_output_name(output_name)?;
        if unique_outputs.insert(output_name.clone(), ()).is_some() {
            return Err(Error::Invalid(format!(
                "derivation declares duplicate output `{output_name}`"
            )));
        }
    }

    let output_hash = optional_string_field(fields, "outputHash")?;
    let output_hash_algo = optional_string_field(fields, "outputHashAlgo")?;
    let output_hash_mode = optional_string_field(fields, "outputHashMode")?;
    let (method_algo, hash_hex) = match output_hash {
        Some(hash) => {
            if output_names.len() != 1 || output_names[0] != "out" {
                return Err(Error::Unsupported(
                    "fixed-output derivations must declare exactly the `out` output".into(),
                ));
            }
            let algo = output_hash_algo
                .clone()
                .unwrap_or_else(|| "sha256".into());
            let mode = output_hash_mode
                .clone()
                .unwrap_or_else(|| "flat".into());
            let method_algo = match mode.as_str() {
                "flat" => algo,
                "recursive" => format!("r:{algo}"),
                _ => {
                    return Err(Error::Unsupported(format!(
                        "unsupported fixed-output hash mode `{mode}`"
                    )))
                }
            };
            let hash_hex = normalize_hash_hex(&hash)?;
            (method_algo, hash_hex)
        }
        None if output_hash_algo.is_some() || output_hash_mode.is_some() => {
            return Err(Error::Invalid(
                "outputHashAlgo/outputHashMode require outputHash".into(),
            ))
        }
        None => (String::new(), String::new()),
    };

    let mut env = BTreeMap::new();
    let ignored = [
        "name",
        "system",
        "builder",
        "args",
        "outputs",
        "outputHash",
        "outputHashAlgo",
        "outputHashMode",
        "__ignoreNulls",
    ];
    let ignore_nulls = matches!(field_value(fields, "__ignoreNulls")?, Some(Value::Bool(true)));
    for (field, thunk) in fields {
        if ignored.contains(&field.as_str()) || output_names.iter().any(|name| name == field) {
            continue;
        }
        let value = thunk.force()?;
        if matches!(value, Value::Null) {
            if ignore_nulls {
                continue;
            }
            return Err(Error::Invalid(format!(
                "derivation environment field `{field}` is null"
            )));
        }
        let (text, contexts) = stringify_value(&value)?;
        record_derivation_contexts(&contexts, &mut input_sources)?;
        if env.insert(field.clone(), text).is_some() {
            return Err(Error::Invalid(format!("duplicate derivation field `{field}`")));
        }
        if env.len() > MAX_DERIVATION_ENV {
            return Err(Error::ResourceLimit(format!(
                "derivation has more than {MAX_DERIVATION_ENV} environment fields"
            )));
        }
    }
    env.insert("builder".into(), builder.clone());
    env.insert("name".into(), name.clone());
    env.insert("system".into(), system.clone());
    if !method_algo.is_empty() {
        env.insert("outputHash".into(), hash_hex.clone());
        env.insert(
            "outputHashAlgo".into(),
            output_hash_algo.unwrap_or_else(|| "sha256".into()),
        );
        env.insert(
            "outputHashMode".into(),
            output_hash_mode.unwrap_or_else(|| "flat".into()),
        );
    }
    for output_name in &output_names {
        env.insert(output_name.clone(), String::new());
    }

    if input_sources.len() > MAX_DERIVATION_INPUTS {
        return Err(Error::ResourceLimit(format!(
            "derivation has more than {MAX_DERIVATION_INPUTS} source inputs"
        )));
    }
    input_sources.sort();
    input_sources.dedup();

    Ok(DerivationEvaluation {
        name,
        system,
        builder,
        args,
        env,
        input_sources,
        outputs: output_names
            .into_iter()
            .map(|name| DerivationOutputEvaluation {
                name,
                method_algo: method_algo.clone(),
                hash_hex: hash_hex.clone(),
            })
            .collect(),
    })
}

fn field_value(fields: &BTreeMap<String, Thunk>, name: &str) -> Result<Option<Value>, Error> {
    attr_field(fields, name).map(|value| value.force()).transpose()
}

fn required_string_field(fields: &BTreeMap<String, Thunk>, name: &str) -> Result<String, Error> {
    let value = field_value(fields, name)?.ok_or_else(|| Error::Missing(name.into()))?;
    string_for_derivation(value)
}

fn optional_string_field(
    fields: &BTreeMap<String, Thunk>,
    name: &str,
) -> Result<Option<String>, Error> {
    field_value(fields, name)?
        .map(string_for_derivation)
        .transpose()
}

fn string_for_derivation(value: Value) -> Result<String, Error> {
    let (text, contexts) = stringify_value(&value)?;
    let mut inputs = Vec::new();
    record_derivation_contexts(&contexts, &mut inputs)?;
    Ok(text)
}

fn record_derivation_contexts(contexts: &[String], inputs: &mut Vec<String>) -> Result<(), Error> {
    for context in contexts {
        let Some(path) = context.strip_prefix("path:") else {
            return Err(Error::Unsupported(
                "derivation inputs must have canonical store-path context".into(),
            ));
        };
        if !is_store_path(path) {
            return Err(Error::Unsupported(
                "project-relative paths do not have canonical derivation identities".into(),
            ));
        }
        inputs.push(path.to_string());
    }
    Ok(())
}

fn is_store_path(path: &str) -> bool {
    let Some(name) = path.strip_prefix("/nix/store/") else {
        return false;
    };
    let Some((hash, label)) = name.split_once('-') else {
        return false;
    };
    hash.len() == 32
        && hash.bytes().all(|byte| NIX32_CHARS.contains(&byte))
        && !label.is_empty()
        && label.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'_' | b'?' | b'=' | b'-')
        })
}

fn validate_derivation_name(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > MAX_PATH_BYTES
        || name.contains('/')
        || name.contains('\0')
        || name == "."
        || name == ".."
    {
        return Err(Error::Invalid(format!("invalid derivation name `{name}`")));
    }
    Ok(())
}

fn validate_derivation_output_name(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > MAX_PACKAGE_NAME_BYTES
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('=')
        || name.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(Error::Invalid(format!("invalid derivation output name `{name}`")));
    }
    Ok(())
}

fn normalize_hash_hex(hash: &str) -> Result<String, Error> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Unsupported(
            "only canonical 256-bit hexadecimal output hashes are supported".into(),
        ));
    }
    Ok(hash.to_ascii_lowercase())
}

fn apply_builtin(
    builtin: Builtin,
    argument: Value,
    _arena: &Rc<EvaluationArena>,
) -> Result<Value, Error> {
    match builtin {
        Builtin::AttrNames => {
            let Value::AttrSet(fields) = argument else {
                return Err(Error::Type {
                    expected: "attribute set",
                    actual: value_name(&argument),
                });
            };
            Ok(Value::List(
                fields
                    .keys()
                    .map(|name| Thunk::value(Value::String(name.clone())))
                    .collect(),
            ))
        }
        Builtin::AttrValues => {
            let Value::AttrSet(fields) = argument else {
                return Err(Error::Type {
                    expected: "attribute set",
                    actual: value_name(&argument),
                });
            };
            Ok(Value::List(fields.into_values().collect()))
        }
        Builtin::GetAttr => Ok(Value::Native(NativeFunction::Partial {
            operation: NativeOperation::GetAttr,
            arguments: vec![argument],
        })),
        Builtin::HasAttr => Ok(Value::Native(NativeFunction::Partial {
            operation: NativeOperation::HasAttr,
            arguments: vec![argument],
        })),
        Builtin::IsAttrs => Ok(Value::Bool(matches!(
            argument,
            Value::AttrSet(_) | Value::PackageNamespace(_) | Value::PackageOverlay(_, _)
        ))),
        Builtin::IsBool => Ok(Value::Bool(matches!(argument, Value::Bool(_)))),
        Builtin::IsFunction => Ok(Value::Bool(matches!(
            argument,
            Value::Function(_) | Value::Native(_)
        ))),
        Builtin::IsInt => Ok(Value::Bool(matches!(argument, Value::Integer(_)))),
        Builtin::IsList => Ok(Value::Bool(matches!(argument, Value::List(_)))),
        Builtin::IsNull => Ok(Value::Bool(matches!(argument, Value::Null))),
        Builtin::IsPath => Ok(Value::Bool(matches!(argument, Value::Path(_)))),
        Builtin::IsString => Ok(Value::Bool(matches!(
            argument,
            Value::String(_) | Value::StringContext { .. }
        ))),
        Builtin::Length => match argument {
            Value::List(values) => Ok(Value::Integer(values.len() as i64)),
            value => Err(Error::Type {
                expected: "list",
                actual: value_name(&value),
            }),
        },
        Builtin::Head => match argument {
            Value::List(values) => values
                .into_iter()
                .next()
                .ok_or_else(|| Error::Invalid("head called on an empty list".into()))?
                .force(),
            value => Err(Error::Type {
                expected: "list",
                actual: value_name(&value),
            }),
        },
        Builtin::Tail => match argument {
            Value::List(values) => {
                if values.is_empty() {
                    return Err(Error::Invalid("tail called on an empty list".into()));
                }
                Ok(Value::List(values.into_iter().skip(1).collect()))
            }
            value => Err(Error::Type {
                expected: "list",
                actual: value_name(&value),
            }),
        },
        Builtin::ConcatLists => {
            let Value::List(values) = argument else {
                return Err(Error::Type {
                    expected: "list of lists",
                    actual: value_name(&argument),
                });
            };
            let mut flattened = Vec::new();
            for value in values {
                let Value::List(items) = value.force()? else {
                    return Err(Error::Type {
                        expected: "list of lists",
                        actual: "non-list item",
                    });
                };
                flattened.extend(items);
            }
            Ok(Value::List(flattened))
        }
        Builtin::ConcatStringsSep => Ok(Value::Native(NativeFunction::Partial {
            operation: NativeOperation::ConcatStringsSep,
            arguments: vec![argument],
        })),
        Builtin::Map => Ok(Value::Native(NativeFunction::Partial {
            operation: NativeOperation::Map,
            arguments: vec![argument],
        })),
        Builtin::Filter => Ok(Value::Native(NativeFunction::Partial {
            operation: NativeOperation::Filter,
            arguments: vec![argument],
        })),
        Builtin::ReplaceStrings => Ok(Value::Native(NativeFunction::Partial {
            operation: NativeOperation::ReplaceStrings,
            arguments: vec![argument],
        })),
        Builtin::Substring => Ok(Value::Native(NativeFunction::Partial {
            operation: NativeOperation::Substring,
            arguments: vec![argument],
        })),
        Builtin::BaseNameOf => {
            let value = plain_string(argument)?;
            Ok(Value::String(
                value.rsplit('/').next().unwrap_or(value.as_str()).into(),
            ))
        }
        Builtin::DirOf => {
            let value = plain_string(argument)?;
            let directory = value.rsplit_once('/').map_or(".", |(directory, _)| {
                if directory.is_empty() {
                    "/"
                } else {
                    directory
                }
            });
            Ok(Value::String(directory.into()))
        }
        Builtin::StringLength => Ok(Value::Integer(plain_string(argument)?.chars().count() as i64)),
        Builtin::ToJSON => Ok(Value::String(encode_json(&value_to_json(&argument, 0)?)?)),
        Builtin::FromJSON => {
            let text = plain_string(argument)?;
            let json = JSON::parse(&text).map_err(Error::Invalid)?;
            json_to_value(json, 0)
        }
        Builtin::StorePath => {
            let path = plain_string(argument)?;
            if !is_store_path(&path) {
                return Err(Error::Unsupported(
                    "builtins.storePath requires a canonical /nix/store path".into(),
                ));
            }
            Ok(Value::StringContext {
                value: path.clone(),
                contexts: vec![format!("path:{path}")],
            })
        }
        Builtin::Throw => Err(Error::Invalid(format!(
            "foreign flake evaluation threw: {}",
            plain_string(argument)?
        ))),
    }
}

fn apply_partial(
    operation: NativeOperation,
    arguments: Vec<Value>,
    arena: &Rc<EvaluationArena>,
) -> Result<Value, Error> {
    let arity = match operation {
        NativeOperation::GetAttr
        | NativeOperation::HasAttr
        | NativeOperation::Map
        | NativeOperation::Filter
        | NativeOperation::ConcatStringsSep => 2,
        NativeOperation::ReplaceStrings | NativeOperation::Substring => 3,
    };
    if arguments.len() < arity {
        return Ok(Value::Native(NativeFunction::Partial {
            operation,
            arguments,
        }));
    }
    if arguments.len() > arity {
        return Err(Error::Invalid("builtin received too many arguments".into()));
    }
    match operation {
        NativeOperation::GetAttr => {
            let name = plain_string(arguments[0].clone())?;
            let Value::AttrSet(fields) = arguments[1].clone() else {
                return Err(Error::Type {
                    expected: "attribute set",
                    actual: value_name(&arguments[1]),
                });
            };
            attr_field(&fields, &name)
                .ok_or(Error::Missing(name))?
                .force()
        }
        NativeOperation::HasAttr => {
            let name = plain_string(arguments[0].clone())?;
            let Value::AttrSet(fields) = arguments[1].clone() else {
                return Err(Error::Type {
                    expected: "attribute set",
                    actual: value_name(&arguments[1]),
                });
            };
            Ok(Value::Bool(attr_field(&fields, &name).is_some()))
        }
        NativeOperation::Map | NativeOperation::Filter => {
            let function = arguments[0].clone();
            let Value::List(values) = arguments[1].clone() else {
                return Err(Error::Type {
                    expected: "list",
                    actual: value_name(&arguments[1]),
                });
            };
            let mut output = Vec::new();
            for value in values {
                let candidate = value.clone();
                let result = apply(function.clone(), value, arena)?;
                if matches!(operation, NativeOperation::Filter) {
                    match result {
                        Value::Bool(true) => {
                            output.push(candidate);
                            continue;
                        }
                        Value::Bool(false) => continue,
                        value => {
                            return Err(Error::Type {
                                expected: "boolean predicate result",
                                actual: value_name(&value),
                            })
                        }
                    }
                }
                output.push(Thunk::value(result));
            }
            Ok(Value::List(output))
        }
        NativeOperation::ConcatStringsSep => {
            let separator = plain_string(arguments[0].clone())?;
            let Value::List(values) = arguments[1].clone() else {
                return Err(Error::Type {
                    expected: "list",
                    actual: value_name(&arguments[1]),
                });
            };
            let mut output = String::new();
            let mut contexts = Vec::new();
            for (index, value) in values.into_iter().enumerate() {
                if index > 0 {
                    output.push_str(&separator);
                }
                let (text, value_contexts) = stringify_value(&value.force()?)?;
                output.push_str(&text);
                for context in value_contexts {
                    if !contexts.contains(&context) {
                        contexts.push(context);
                    }
                }
            }
            if contexts.is_empty() {
                Ok(Value::String(output))
            } else {
                Ok(Value::StringContext { value: output, contexts })
            }
        }
        NativeOperation::ReplaceStrings => {
            let from = string_list(arguments[0].clone())?;
            let to = string_list(arguments[1].clone())?;
            if from.len() != to.len() {
                return Err(Error::Invalid(
                    "replaceStrings requires equal search and replacement lists".into(),
                ));
            }
            let (subject, contexts) = stringify_value(&arguments[2])?;
            let mut output = String::new();
            let mut position = 0;
            while position < subject.len() {
                let match_index = from.iter().enumerate().find_map(|(index, needle)| {
                    (!needle.is_empty() && subject[position..].starts_with(needle.as_str()))
                        .then_some(index)
                });
                if let Some(index) = match_index {
                    output.push_str(&to[index]);
                    position += from[index].len();
                } else {
                    let character = subject[position..]
                        .chars()
                        .next()
                        .ok_or_else(|| Error::Invalid("invalid UTF-8 string boundary".into()))?;
                    output.push(character);
                    position += character.len_utf8();
                }
            }
            if contexts.is_empty() {
                Ok(Value::String(output))
            } else {
                Ok(Value::StringContext { value: output, contexts })
            }
        }
        NativeOperation::Substring => {
            let Value::Integer(start) = arguments[0] else {
                return Err(Error::Type {
                    expected: "integer substring start",
                    actual: value_name(&arguments[0]),
                });
            };
            let Value::Integer(length) = arguments[1] else {
                return Err(Error::Type {
                    expected: "integer substring length",
                    actual: value_name(&arguments[1]),
                });
            };
            let (text, contexts) = stringify_value(&arguments[2])?;
            if start < 0 {
                return Err(Error::Invalid(
                    "negative start position in substring".into(),
                ));
            }
            let start = start as usize;
            let value: String = if length < 0 {
                text.chars().skip(start).collect()
            } else {
                text.chars().skip(start).take(length as usize).collect()
            };
            if contexts.is_empty() {
                Ok(Value::String(value))
            } else {
                Ok(Value::StringContext { value, contexts })
            }
        }
    }
}

fn plain_string(value: Value) -> Result<String, Error> {
    stringify_value(&value).map(|(text, _)| text)
}

fn string_list(value: Value) -> Result<Vec<String>, Error> {
    let Value::List(values) = value else {
        return Err(Error::Type {
            expected: "string list",
            actual: value_name(&value),
        });
    };
    values
        .into_iter()
        .map(|value| plain_string(value.force()?))
        .collect()
}

fn value_to_json(value: &Value, depth: usize) -> Result<JSONValue, Error> {
    if depth > MAX_EVAL_DEPTH {
        return Err(Error::ResourceLimit("JSON value is too deeply nested".into()));
    }
    match value {
        Value::Null => Ok(JSONValue::Null),
        Value::Bool(value) => Ok(JSONValue::Bool(*value)),
        Value::Integer(value) => Ok(JSONValue::Num(*value as f64)),
        Value::String(value)
        | Value::StringContext { value, .. }
        | Value::Path(value)
        | Value::Package(value) => Ok(JSONValue::Str(value.clone())),
        Value::List(values) => values
            .iter()
            .map(|value| value.force().and_then(|value| value_to_json(&value, depth + 1)))
            .collect::<Result<Vec<_>, _>>()
            .map(JSONValue::Array),
        Value::AttrSet(fields) => {
            let mut output = BTreeMap::new();
            for (name, value) in fields {
                let value = value.force()?;
                if output
                    .insert(name.clone(), value_to_json(&value, depth + 1)?)
                    .is_some()
                {
                    return Err(Error::Invalid(format!("duplicate JSON attribute {name}")));
                }
            }
            Ok(JSONValue::Object(output))
        }
        Value::PackageNamespace(_)
        | Value::PackageOverlay(_, _)
        | Value::BuiltinsNamespace(_)
        | Value::LibraryNamespace
        | Value::Derivation(_)
        | Value::Function(_)
        | Value::Native(_) => Err(Error::Unsupported(format!(
            "value of type {} cannot be encoded as JSON",
            value_name(value)
        ))),
    }
}

fn json_to_value(value: JSONValue, depth: usize) -> Result<Value, Error> {
    if depth > MAX_EVAL_DEPTH {
        return Err(Error::ResourceLimit("JSON value is too deeply nested".into()));
    }
    match value {
        JSONValue::Null => Ok(Value::Null),
        JSONValue::Bool(value) => Ok(Value::Bool(value)),
        JSONValue::Num(value)
            if value.is_finite()
                && value >= i64::MIN as f64
                && value <= i64::MAX as f64
                && (value as i64) as f64 == value =>
        {
            Ok(Value::Integer(value as i64))
        }
        JSONValue::Num(_) => Err(Error::Unsupported(
            "fromJSON only supports Nix integer JSON numbers".into(),
        )),
        JSONValue::Str(value) => Ok(Value::String(value)),
        JSONValue::Array(values) => Ok(Value::List(
            values
                .into_iter()
                .map(|value| json_to_value(value, depth + 1).map(Thunk::value))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        JSONValue::Object(values) => {
            let mut fields = BTreeMap::new();
            for (name, value) in values {
                let _ = fields.insert(name, Thunk::value(json_to_value(value, depth + 1)?));
            }
            Ok(Value::AttrSet(fields))
        }
    }
}

fn encode_json(value: &JSONValue) -> Result<String, Error> {
    match value {
        JSONValue::Null => Ok("null".into()),
        JSONValue::Bool(value) => Ok(if *value { "true" } else { "false" }.into()),
        JSONValue::Num(value) => Ok(format!("{value}")),
        JSONValue::Str(value) => Ok(encode_json_string(value)),
        JSONValue::Array(values) => {
            let values = values
                .iter()
                .map(encode_json)
                .collect::<Result<Vec<_>, _>>()?
                .join(",");
            Ok(format!("[{values}]"))
        }
        JSONValue::Object(values) => {
            let values = values
                .iter()
                .map(|(name, value)| {
                    Ok(format!(
                        "{}:{}",
                        encode_json_string(name),
                        encode_json(value)?
                    ))
                })
                .collect::<Result<Vec<_>, Error>>()?
                .join(",");
            Ok(format!("{{{values}}}"))
        }
    }
}

fn encode_json_string(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if (character as u32) == 8 => output.push_str("\\b"),
            character if (character as u32) == 12 => output.push_str("\\f"),
            character if (character as u32) < 0x20 => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn merge(left: Value, right: Value) -> Result<Value, Error> {
    if matches!(&left, Value::PackageNamespace(_) | Value::PackageOverlay(_, _)) {
        let Value::AttrSet(fields) = right else {
            return Err(Error::Type {
                expected: "package overlay attribute set",
                actual: value_name(&right),
            });
        };
        return match left {
            Value::PackageNamespace(prefix) => Ok(Value::PackageOverlay(
                prefix,
                Rc::new(RefCell::new(fields)),
            )),
            Value::PackageOverlay(prefix, previous) => {
                previous.borrow_mut().extend(fields);
                Ok(Value::PackageOverlay(prefix, previous))
            }
            _ => unreachable!(),
        };
    }
    let Value::AttrSet(mut left) = left else {
        return Err(Error::Type {
            expected: "attribute set",
            actual: value_name(&left),
        });
    };
    let Value::AttrSet(right) = right else {
        return Err(Error::Type {
            expected: "attribute set",
            actual: value_name(&right),
        });
    };
    left.extend(right);
    Ok(Value::AttrSet(left))
}

fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Integer(left), Value::Integer(right)) => left == right,
        (Value::String(left), Value::String(right)) => left == right,
        (
            Value::StringContext { value: left, .. },
            Value::StringContext { value: right, .. },
        ) => left == right,
        (Value::String(left), Value::StringContext { value: right, .. })
        | (Value::StringContext { value: left, .. }, Value::String(right)) => left == right,
        (Value::Package(left), Value::Package(right)) => left == right,
        _ => false,
    }
}

fn value_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Integer(_) => "integer",
        Value::String(_) | Value::StringContext { .. } => "string",
        Value::Path(_) => "path",
        Value::Package(_) => "package",
        Value::PackageNamespace(_) => "package namespace",
        Value::PackageOverlay(_, _) => "package namespace",
        Value::BuiltinsNamespace(_) => "builtins namespace",
        Value::LibraryNamespace => "library namespace",
        Value::List(_) => "list",
        Value::AttrSet(_) => "attribute set",
        Value::Derivation(_) => "derivation",
        Value::Function(_) | Value::Native(_) => "function",
    }
}

fn flake_arguments(system: &str) -> Value {
    let mut fields = BTreeMap::new();
    let _ = fields.insert(
        "self".into(),
        Thunk::value(Value::AttrSet(BTreeMap::new())),
    );
    let _ = fields.insert(
        "nixpkgs".into(),
        Thunk::value(Value::PackageNamespace("".into())),
    );
    let _ = fields.insert(
        "legacyPackages".into(),
        Thunk::value(Value::PackageNamespace("".into())),
    );
    let _ = fields.insert("system".into(), Thunk::value(Value::String(system.to_string())));
    Value::AttrSet(fields)
}
