use super::{DevShellEvaluation, EvaluationError};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::{Rc, Weak};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::fmt;
use core::mem;

const MAX_TOKENS: usize = 65_536;
const MAX_EVAL_DEPTH: usize = 256;
const MAX_DEV_SHELL_PACKAGES: usize = 256;
const MAX_PACKAGE_NAME_BYTES: usize = 128;

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
            Self::Cycle => write!(output, "cyclic foreign flake thunk"),
        }
    }
}

#[derive(Clone)]
enum Expr {
    String(String),
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
enum Pattern {
    Name(String),
    Attrs(Vec<(String, Option<Expr>)>),
}

#[derive(Clone)]
enum Token {
    Identifier(String),
    String(String),
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
            b'/' => {
                return Err(Error::Unsupported(
                    "path values belong to the authority stage".into(),
                ));
            }
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
            b'"' => Token::String(self.string()?),
            b'\'' if self.source.as_bytes().get(self.position + 1) == Some(&b'\'') => {
                Token::String(self.indented_string()?)
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

    fn string(&mut self) -> Result<String, Error> {
        self.position += 1;
        let mut value = String::new();
        while self.position < self.source.len() {
            let byte = self.source.as_bytes()[self.position];
            if byte == b'"' {
                self.position += 1;
                return Ok(value);
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
                value.push(character);
                self.position += 1;
                continue;
            }
            if byte == b'$' && self.source.as_bytes().get(self.position + 1) == Some(&b'{') {
                return Err(Error::Unsupported(
                    "string interpolation belongs to the string-context stage".into(),
                ));
            }
            let character = self.source[self.position..]
                .chars()
                .next()
                .ok_or_else(|| Error::Syntax("invalid UTF-8 string".into()))?;
            value.push(character);
            self.position += character.len_utf8();
        }
        Err(Error::Syntax("unterminated string".into()))
    }

    fn indented_string(&mut self) -> Result<String, Error> {
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
        if value.contains("${") {
            return Err(Error::Unsupported(
                "string interpolation belongs to the string-context stage".into(),
            ));
        }
        self.position += 2;
        Ok(value)
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

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, position: 0 }
    }

    fn parse(mut self) -> Result<Expr, Error> {
        let expression = self.expression()?;
        if !matches!(self.peek(), Token::End) {
            return Err(Error::Syntax("trailing expression".into()));
        }
        Ok(expression)
    }

    fn expression(&mut self) -> Result<Expr, Error> {
        if let Some((pattern, body)) = self.try_lambda()? {
            return Ok(Expr::Lambda(pattern, Box::new(body)));
        }
        self.equality()
    }

    fn try_lambda(&mut self) -> Result<Option<(Pattern, Expr)>, Error> {
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
        let mut expression = self.application()?;
        while matches!(self.peek(), Token::Merge) {
            self.position += 1;
            expression = Expr::Merge(Box::new(expression), Box::new(self.application()?));
        }
        Ok(expression)
    }

    fn application(&mut self) -> Result<Expr, Error> {
        let mut expression = self.selection()?;
        while self.starts_application() {
            expression = Expr::Apply(Box::new(expression), Box::new(self.selection()?));
        }
        Ok(expression)
    }

    fn selection(&mut self) -> Result<Expr, Error> {
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
        match self.bump() {
            Token::String(value) => Ok(Expr::String(value)),
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

    fn attrset(&mut self) -> Result<Expr, Error> {
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
        let scope = self.expression()?;
        if !matches!(self.peek(), Token::Semicolon) {
            return Err(Error::Syntax("with expression requires `;`".into()));
        }
        self.position += 1;
        Ok(Expr::With(Box::new(scope), Box::new(self.expression()?)))
    }

    fn if_expression(&mut self) -> Result<Expr, Error> {
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
            Token::String(_) | Token::Integer(_) | Token::LeftBrace | Token::LeftBracket => true,
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
}

impl EvaluationArena {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            environments: RefCell::new(Vec::new()),
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
}

impl EnvironmentFrame {
    fn root(arena: &Rc<EvaluationArena>, system: &str) -> Environment {
        let environment = Rc::new(RefCell::new(Self {
            parent: None,
            bindings: BTreeMap::new(),
            scopes: Vec::new(),
            fuel: Rc::new(Cell::new(0)),
            arena: Rc::downgrade(arena),
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
                Thunk::value(Value::String(system.to_string())),
            );
            let _ = frame.bindings.insert(
                "builtins".into(),
                Thunk::value(Value::BuiltinsNamespace),
            );
            let _ = frame.bindings.insert(
                "mkShell".into(),
                Thunk::value(Value::Native(NativeFunction::MkShell)),
            );
        }
        environment
    }

    fn child(parent: &Environment) -> Result<Environment, Error> {
        let (fuel, arena) = {
            let parent = parent.borrow();
            let Some(arena) = parent.arena.upgrade() else {
                return Err(Error::Invalid("foreign flake evaluation arena expired".into()));
            };
            (parent.fuel.clone(), arena)
        };
        let environment = Rc::new(RefCell::new(Self {
            parent: Some(parent.clone()),
            bindings: BTreeMap::new(),
            scopes: Vec::new(),
            fuel,
            arena: Rc::downgrade(&arena),
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
    Package(String),
    PackageNamespace(String),
    BuiltinsNamespace,
    LibraryNamespace,
    List(Vec<Thunk>),
    AttrSet(BTreeMap<String, Thunk>),
    Function(Rc<FunctionValue>),
    Native(NativeFunction),
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
}

pub(super) fn evaluate_devshell(
    source: &str,
    system: &str,
) -> Result<DevShellEvaluation, EvaluationError> {
    let tokens = Lexer::new(source).tokenize().map_err(Error::public)?;
    let expression = Parser::new(tokens).parse().map_err(Error::public)?;
    let arena = EvaluationArena::new();
    let environment = EnvironmentFrame::root(&arena, system);
    let root = evaluate_expr(&expression, &environment, 0, &arena).map_err(Error::public)?;
    let shell = resolve_shell(root.clone(), system, &arena).map_err(Error::public)?;
    project_shell(shell, system).map_err(Error::public)
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
            Value::String(_) | Value::Integer(_) | Value::Bool(_) | Value::Null => {
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
        .rsplit(|character| matches!(character, '.' | '/'))
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

fn try_select(value: Value, field: &str) -> Result<Option<Thunk>, Error> {
    match value {
        Value::AttrSet(fields) => Ok(attr_field(&fields, field)),
        Value::PackageNamespace(prefix) => {
            if field == "lib" {
                Ok(Some(Thunk::value(Value::LibraryNamespace)))
            } else if field == "mkShell" {
                Ok(Some(Thunk::value(Value::Native(NativeFunction::MkShell))))
            } else {
                let name = if prefix.is_empty() {
                    field.to_string()
                } else {
                    format!("{prefix}.{field}")
                };
                Ok(Some(Thunk::value(Value::Package(name))))
            }
        }
        Value::Package(prefix) => Ok(Some(Thunk::value(Value::Package(format!(
            "{prefix}.{field}"
        ))))),
        Value::LibraryNamespace => Ok(None),
        Value::BuiltinsNamespace => Ok(None),
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
        value => Err(Error::Type {
            expected: "function",
            actual: value_name(&value),
        }),
    }
}

fn merge(left: Value, right: Value) -> Result<Value, Error> {
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
        (Value::Package(left), Value::Package(right)) => left == right,
        _ => false,
    }
}

fn value_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Integer(_) => "integer",
        Value::String(_) => "string",
        Value::Package(_) => "package",
        Value::PackageNamespace(_) => "package namespace",
        Value::BuiltinsNamespace => "builtins namespace",
        Value::LibraryNamespace => "library namespace",
        Value::List(_) => "list",
        Value::AttrSet(_) => "attribute set",
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
