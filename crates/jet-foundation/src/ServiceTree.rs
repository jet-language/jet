//! Shared validation for the typed service-tree builder.
//!
//! Sema and TIR consume the same structural fact. This keeps one declaration
//! shape across all tiers without making the compiler engines execute a
//! callback.

use crate::AST::{Expr, Lambda, LambdaBody, Stmt, StrPart};

pub const MAX_NAME_BYTES: usize = 256;
pub const MAX_WORKERS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerDeclaration {
    pub name: String,
    pub handler: String,
}

fn literal_string(expr: &Expr) -> Option<String> {
    let Expr::Str(parts, _) = expr else {
        return None;
    };
    let mut value = String::new();
    for part in parts {
        match part {
            StrPart::Lit(text) => value.push_str(text),
            StrPart::Interp(_, _) => return None,
        }
    }
    Some(value)
}

pub fn valid_name(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.chars().any(char::is_control)
        && value.len() <= MAX_NAME_BYTES
}

/// Read the static worker declarations from the first typed tree builder.
///
/// The callback is declaration-only: one builder parameter and expression
/// statements of `root.worker("name", handler)`. The handler identity crosses
/// to the Prelude; arbitrary callback execution stays out of this substrate.
pub fn worker_declarations(lambda: &Lambda) -> Option<Vec<WorkerDeclaration>> {
    if lambda.params.len() != 1 {
        return None;
    }
    let root = &lambda.params[0].name;
    let LambdaBody::Block(statements) = &lambda.body else {
        return None;
    };
    if statements.len() > MAX_WORKERS {
        return None;
    }
    let mut workers = Vec::with_capacity(statements.len());
    for statement in statements {
        let Stmt::Expr(Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        }) = statement
        else {
            return None;
        };
        if method != "worker"
            || args.len() != 2
            || args.iter().any(|arg| arg.label.is_some())
        {
            return None;
        }
        let Expr::Ident(receiver_name, _) = receiver.as_ref() else {
            return None;
        };
        if receiver_name != root {
            return None;
        }
        let name = literal_string(&args[0].expr)?;
        let Expr::Ident(handler, _) = &args[1].expr else {
            return None;
        };
        if !valid_name(&name)
            || !valid_name(handler)
            || workers.iter().any(|worker: &WorkerDeclaration| worker.name == name)
        {
            return None;
        }
        workers.push(WorkerDeclaration {
            name,
            handler: handler.clone(),
        });
    }
    Some(workers)
}
