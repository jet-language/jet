//! AST nodes. Grows with each milestone; keep nodes small and keep spans on
//! anything an error might need to point at.

use crate::diag::Span;
use crate::syntax;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessConvention {
    /// Default: shared read borrow (`&T` in Rust; scalars pass by value).
    Read,
    /// Mutable borrow (`&mut T`).
    Mutate,
    /// Ownership transfer (`T` by value).
    Move,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    /// S41 (M5): Unicode scalar value.
    Char,
    List(Box<Type>),
    /// S38 (M5): keyed collection `Map<K, V>`.
    Map {
        key: Box<Type>,
        value: Box<Type>,
    },
    Shared(Box<Type>),
    /// S32: `T?` optional value.
    Option(Box<Type>),
    /// S34: `Result<T, E>` fallible return.
    Result {
        ok: Box<Type>,
        err: Box<Type>,
    },
    Named(String),
}

impl Type {
    /// Plain-words name for diagnostics (docs/04 voice: name both types).
    pub fn show(&self) -> String {
        match self {
            Type::Int => "Int (a whole number)".to_string(),
            Type::Float => "Float (a decimal number)".to_string(),
            Type::Bool => "Bool (true or false)".to_string(),
            Type::String => "String (text)".to_string(),
            Type::Char => "Char (one character)".to_string(),
            Type::List(inner) => format!("List<{}>", inner.name()),
            Type::Map { key, value } => format!("Map<{}, {}>", key.name(), value.name()),
            Type::Shared(inner) => format!("Shared<{}>", inner.name()),
            Type::Option(inner) => format!("{}?", inner.name()),
            Type::Result { ok, err } => {
                format!("Result<{}, {}>", ok.name(), err.name())
            }
            Type::Named(n) => format!("`{}`", n),
        }
    }

    /// Bare type name, no gloss.
    pub fn name(&self) -> String {
        match self {
            Type::Int => "Int".to_string(),
            Type::Float => "Float".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::String => "String".to_string(),
            Type::Char => "Char".to_string(),
            Type::List(inner) => format!("List<{}>", inner.name()),
            Type::Map { key, value } => format!("Map<{}, {}>", key.name(), value.name()),
            Type::Shared(inner) => format!("Shared<{}>", inner.name()),
            Type::Option(inner) => format!("{}?", inner.name()),
            Type::Result { ok, err } => format!("Result<{}, {}>", ok.name(), err.name()),
            Type::Named(n) => n.clone(),
        }
    }

    pub fn is_scalar(&self) -> bool {
        matches!(self, Type::Int | Type::Float | Type::Bool)
    }

    pub fn unwrap_option(&self) -> Option<&Type> {
        match self {
            Type::Option(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn unwrap_result(&self) -> Option<(&Type, &Type)> {
        match self {
            Type::Result { ok, err } => Some((ok, err)),
            _ => None,
        }
    }

    pub fn is_fallible(&self) -> bool {
        matches!(self, Type::Option(_) | Type::Result { .. })
    }
}

#[derive(Debug)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug)]
pub enum Item {
    Func(Func),
    Struct(StructDef),
    Enum(EnumDef),
    Impl(ImplDef),
    Const(ConstDef),
}

#[derive(Debug)]
pub struct Func {
    pub is_pub: bool,
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub is_view_return: bool,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct Param {
    pub convention: AccessConvention,
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub ty_span: Span,
}

#[derive(Debug)]
pub struct StructDef {
    pub is_pub: bool,
    pub name: String,
    pub name_span: Span,
    pub fields: Vec<Field>,
    pub methods: Vec<Func>,
}

#[derive(Debug)]
pub struct EnumDef {
    pub is_pub: bool,
    pub name: String,
    pub name_span: Span,
    pub variants: Vec<Variant>,
    pub methods: Vec<Func>,
}

#[derive(Debug)]
pub struct Variant {
    pub name: String,
    pub name_span: Span,
    pub payload: VariantPayload,
}

#[derive(Debug, Clone)]
pub enum VariantPayload {
    Unit,
    /// S30: single-field variants use a positional type only.
    Single(Type, Span),
    /// S30: two or more payload fields are named in the declaration.
    Named(Vec<VariantField>),
}

#[derive(Debug, Clone)]
pub struct VariantField {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub ty_span: Span,
}

#[derive(Debug)]
pub struct ImplDef {
    pub type_name: String,
    pub type_span: Span,
    pub methods: Vec<Func>,
}

#[derive(Debug)]
pub struct Field {
    pub is_stored_ref: bool,
    pub stored_ref_label: Option<String>,
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub ty_span: Span,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Variant {
        variant: String,
        bindings: Vec<String>,
        span: Span,
    },
    Present {
        binding: String,
        span: Span,
    },
    Absent(Span),
    /// S34: `ok(binding)` pattern on `Result<T, E>`.
    Ok {
        binding: String,
        span: Span,
    },
    /// S34: `err(binding)` pattern on `Result<T, E>`.
    Err {
        binding: String,
        span: Span,
    },
}

/// S35: right-hand side of `expr or …`.
#[derive(Debug, Clone)]
pub enum OrFallback {
    Value(Box<Expr>),
    Return(Option<Box<Expr>>, Span),
    Panic {
        name_span: Span,
        args: Vec<CallArg>,
    },
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Variant { span, .. }
            | Pattern::Present { span, .. }
            | Pattern::Ok { span, .. }
            | Pattern::Err { span, .. } => *span,
            Pattern::Absent(span) => *span,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EnumLitArg {
    Positional(Expr),
    Named {
        label: String,
        expr: Expr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstAttr {
    ForceStatic,
    ForceInline,
}

#[derive(Debug)]
pub struct ConstDef {
    pub name: String,
    pub name_span: Span,
    pub value: Expr,
    pub attrs: Vec<ConstAttr>,
    pub rust_kind: RustConstKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustConstKind {
    Const,
    Static,
}

/// One `if`/`else if`/`else` chain.
#[derive(Debug)]
pub struct IfStmt {
    pub cond: Expr,
    pub then_body: Vec<Stmt>,
    pub else_branch: Option<ElseBranch>,
    pub span: Span,
}

#[derive(Debug)]
pub enum ElseBranch {
    ElseIf(Box<IfStmt>),
    Else(Vec<Stmt>),
}

/// One `switch` arm: a condition and a body (S24).
#[derive(Debug)]
pub struct SwitchArm {
    pub cond: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug)]
pub enum Stmt {
    /// A call used for its effect, e.g. `print(x);`.
    Expr(Expr),
    Val(Binding),
    /// `target = e;` (op None) or `target += e;` etc. (op Some, S17).
    Assign {
        target: LValue,
        op: Option<BinOp>,
        op_span: Span,
        value: Expr,
    },
    Return(Option<Expr>, Span),
    If(IfStmt),
    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `for i in a..b` (S22) or `for x in collection` / `for k, v in map` (M5).
    For {
        var: String,
        var_span: Span,
        /// Second binding for `for key, value in map`.
        var2: Option<(String, Span)>,
        kind: ForKind,
        body: Vec<Stmt>,
        span: Span,
    },
    Switch {
        subject: Expr,
        arms: Vec<SwitchArm>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Loop(Vec<Stmt>, Span),
    Unsafe(Vec<Stmt>, Span),
}

/// Assignment target: local name or indexed collection slot (M5).
#[derive(Debug, Clone)]
pub enum LValue {
    Local {
        name: String,
        name_span: Span,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndexKind {
    #[default]
    Unknown,
    List,
    Map,
}

/// `for i in 1..10` vs `for x in xs` (M5).
#[derive(Debug)]
pub enum ForKind {
    Range {
        start: Expr,
        end: Expr,
    },
    In {
        collection: Expr,
    },
}

#[derive(Debug)]
pub struct Binding {
    pub mutable: bool,
    pub name: String,
    pub name_span: Span,
    pub ty: Option<Type>,
    pub ty_span: Option<Span>,
    pub init: Expr,
}

#[derive(Debug, Clone)]
pub struct Call {
    pub name: String,
    pub name_span: Span,
    pub args: Vec<CallArg>,
}

#[derive(Debug, Default, Clone)]
pub struct CallArgFlags {
    pub implicit_clone: bool,
    pub shared_auto_clone: bool,
}

#[derive(Debug, Clone)]
pub struct CallArg {
    pub convention: AccessConvention,
    pub expr: Expr,
    pub span: Span,
    pub flags: CallArgFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
        )
    }

    /// The user-typed spelling (for diagnostics and codegen).
    pub fn spell(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

/// One piece of a string literal (S8): literal text or an interpolated
/// expression.
#[derive(Debug, Clone)]
pub enum StrPart {
    Lit(String),
    Interp(Expr),
}

#[derive(Debug, Clone)]
pub enum Expr {
    /// String literal, possibly with interpolation parts.
    Str(Vec<StrPart>, Span),
    Int(i64, Span),
    Float(f64, Span),
    Bool(bool, Span),
    /// S41: single-quoted `'a'`.
    Char(char, Span),
    /// S37: `[a, b, c]` or `[]`.
    ListLit(Vec<Expr>, Span),
    /// S38: `["k": v]` or `[:]`.
    MapLit(Vec<(Expr, Expr)>, Span),
    /// S39: `xs[i]` or `m[k]`.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
        /// Filled by sema so codegen picks the right runtime helper.
        kind: IndexKind,
    },
    /// S40: inclusive copy slice `xs[a..b]`.
    Slice {
        base: Box<Expr>,
        start: Box<Expr>,
        end: Box<Expr>,
        span: Span,
    },
    Ident(String, Span),
    Call(Call),
    Unary(UnOp, Box<Expr>, Span),
    Binary(BinOp, Box<Expr>, Box<Expr>, Span),
    Deref(Box<Expr>, Span),
    /// Field access: `v.field`.
    Field(Box<Expr>, String, Span),
    /// Method call: `v.method(args)`.
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        method_span: Span,
        args: Vec<CallArg>,
    },
    /// S29: `Type { field: expr, ... }`.
    StructLit {
        type_name: String,
        fields: Vec<(String, Span, Expr)>,
        span: Span,
    },
    /// S30: `Type.Variant(args)`.
    EnumLit {
        type_name: String,
        variant: String,
        args: Vec<EnumLitArg>,
        span: Span,
    },
    /// S32: `value(expr)` — present optional.
    Present(Box<Expr>, Span),
    /// S32: bare `null` — absent optional.
    Absent(Span),
    /// S31: `subject == pattern` (stored as dedicated node for sema/codegen).
    PatternTest {
        subject: Box<Expr>,
        pattern: Pattern,
        span: Span,
    },
    /// S34: `ok(expr)` — success value for `Result<T, E>`.
    Ok(Box<Expr>, Span),
    /// S34: `err(expr)` — failure value for `Result<T, E>`.
    Err(Box<Expr>, Span),
    /// S7: postfix `?` — propagate a fallible value.
    Try(Box<Expr>, Span),
    /// S35: `value or fallback`.
    OrFallback {
        value: Box<Expr>,
        fallback: OrFallback,
        /// Set during typechecking: `true` when the left side is `T?`.
        is_option: bool,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Str(_, s)
            | Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Bool(_, s)
            | Expr::Char(_, s)
            | Expr::ListLit(_, s)
            | Expr::MapLit(_, s)
            | Expr::Index { span: s, .. }
            | Expr::Slice { span: s, .. }
            | Expr::Ident(_, s)
            | Expr::Unary(_, _, s)
            | Expr::Binary(_, _, _, s)
            | Expr::Deref(_, s)
            | Expr::Field(_, _, s)
            | Expr::StructLit { span: s, .. }
            | Expr::EnumLit { span: s, .. }
            | Expr::Present(_, s)
            | Expr::Absent(s)
            | Expr::Ok(_, s)
            | Expr::Err(_, s)
            | Expr::Try(_, s)
            | Expr::OrFallback { span: s, .. }
            | Expr::PatternTest { span: s, .. } => *s,
            Expr::Call(c) => c.name_span,
            Expr::MethodCall { method_span, .. } => *method_span,
        }
    }
}

impl Func {
    /// S27: first parameter named `self`.
    pub fn self_param(&self) -> Option<&Param> {
        self.params.first().filter(|p| p.name == syntax::KW_SELF)
    }

    pub fn is_static_method(&self) -> bool {
        self.self_param().is_none()
    }
}
