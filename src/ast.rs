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
    List(Box<Type>),
    Shared(Box<Type>),
    /// S32: `T?` optional value.
    Option(Box<Type>),
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
            Type::List(inner) => format!("List[{}]", inner.name()),
            Type::Shared(inner) => format!("Shared[{}]", inner.name()),
            Type::Option(inner) => format!("{}?", inner.name()),
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
            Type::List(inner) => format!("List[{}]", inner.name()),
            Type::Shared(inner) => format!("Shared[{}]", inner.name()),
            Type::Option(inner) => format!("{}?", inner.name()),
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
    /// `name = e;` (op None) or `name += e;` etc. (op Some, S17).
    Assign {
        name: String,
        name_span: Span,
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
    /// `for i in a..b` — inclusive on both ends (S22).
    For {
        var: String,
        var_span: Span,
        start: Expr,
        end: Expr,
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
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Str(_, s)
            | Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Bool(_, s)
            | Expr::Ident(_, s)
            | Expr::Unary(_, _, s)
            | Expr::Binary(_, _, _, s)
            | Expr::Deref(_, s)
            | Expr::Field(_, _, s)
            | Expr::StructLit { span: s, .. }
            | Expr::EnumLit { span: s, .. }
            | Expr::Present(_, s)
            | Expr::Absent(s)
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
