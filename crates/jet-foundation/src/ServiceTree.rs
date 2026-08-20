//! The compiler-owned shape of the first `service.tree` declaration slice.
//!
//! Sema and TIR both consume this structural fact. Keeping the grammar here
//! prevents one tier from accepting a worker declaration that another tier
//! cannot carry to the Prelude.

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

fn valid_name(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.chars().any(char::is_control)
        && value.len() <= MAX_NAME_BYTES
}

/// Return the statically declared workers in the first typed tree slice.
///
/// The accepted callback is deliberately narrow: one parameter, a block
/// body, and only `root.worker("name", handler)` expression statements. An
/// ordinary named function is carried as an identity here; invocation,
/// mailbox delivery, and supervision belong to later service-tree slices.
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
