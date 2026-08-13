//! Readable Core prelude registry (D-NAME-ALIAS1=A).
//!
//! The source file is the one human-readable home for ambient names. This
//! module also owns small diagnostic kernels shared by every execution tier.

use std::sync::LazyLock;

use crate::AST::CtValue;
use crate::Diagnostics::{Diagnostic, Span};

pub const SOURCE: &str = include_str!("../../jet-codegen/src/Prelude/core/prelude.jet");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    /// A name whose behavior remains in the embedded Prelude/CoreLib kernel.
    Builtin,
    /// A readable alias into the canonical Core tree.
    Core {
        module: &'static str,
        item: &'static str,
    },
    /// An ambient name whose existing comptime gate remains in force.
    Comptime,
    /// A type re-export from the canonical Core tree.
    Type,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entry {
    pub name: &'static str,
    pub target: Target,
}

static ENTRIES: LazyLock<Vec<Entry>> = LazyLock::new(|| parse(SOURCE));

pub fn entries() -> &'static [Entry] {
    ENTRIES.as_slice()
}

pub fn entry(name: &str) -> Option<&'static Entry> {
    entries().iter().find(|entry| entry.name == name)
}

pub fn names() -> impl Iterator<Item = &'static str> {
    entries().iter().map(|entry| entry.name)
}

/// The one registered refusal for a construct that no evaluator can run yet.
pub fn jet_e0956_unsupported(what: &str, span: Span) -> Diagnostic {
    Diagnostic::from_row("E0956", &[("what", what)], Some(span))
}

/// The one `[U8]` conversion and rejection voice shared by every evaluator.
pub fn jet_as_bytes(value: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    let reject = || jet_e0956_unsupported("this as_bytes call", span);

    match value {
        CtValue::Bytes(bytes) => Ok(bytes.clone()),
        CtValue::List(items) => {
            let mut bytes = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    CtValue::Int(value) if (0..=255).contains(value) => bytes.push(*value as u8),
                    _ => return Err(reject()),
                }
            }
            Ok(bytes)
        }
        _ => Err(reject()),
    }
}

fn parse(source: &'static str) -> Vec<Entry> {
    let mut entries = Vec::new();
    for raw_line in source.lines() {
        let line = raw_line.split("//").next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("pub fn ") {
            if let Some(name) = rest.split(['(', ' ', '{']).next().filter(|name| !name.is_empty()) {
                entries.push(Entry {
                    name,
                    target: Target::Builtin,
                });
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("pub use ") else {
            continue;
        };
        let Some((module, members)) = rest.split_once(".[") else {
            continue;
        };
        let Some(members) = members.strip_suffix(']') else {
            continue;
        };
        for member in members.split(',').map(str::trim).filter(|member| !member.is_empty()) {
            let mut names = member.split(" as ").map(str::trim);
            let original = names.next().unwrap_or_default();
            let local = names.next().unwrap_or(original);
            if original.is_empty() || local.is_empty() {
                continue;
            }
            let target = if original.chars().next().is_some_and(char::is_uppercase) {
                Target::Type
            } else if module == "core.comptime" {
                Target::Comptime
            } else {
                Target::Core {
                    module,
                    item: original,
                }
            };
            entries.push(Entry { name: local, target });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::{entry, entries, jet_as_bytes, names, Target};
    use crate::AST::CtValue;
    use crate::Diagnostics::Span;

    #[test]
    fn source_is_the_complete_ambient_registry() {
        assert_eq!(entries().len(), 19);
        assert_eq!(entry("print").map(|entry| entry.target), Some(Target::Builtin));
        assert_eq!(entry("assert_eq").map(|entry| entry.target), Some(Target::Builtin));
        assert_eq!(
            entry("read_file").map(|entry| entry.target),
            Some(Target::Core {
                module: "core.files",
                item: "read",
            })
        );
        assert_eq!(entry("embed_file").map(|entry| entry.target), Some(Target::Comptime));
        assert_eq!(entry("Clock").map(|entry| entry.target), Some(Target::Type));
        assert!(names().any(|name| name == "file_exists"));
    }

    #[test]
    fn as_bytes_uses_one_shared_rejection() {
        let span = Span::new(4, 7);
        for value in [
            CtValue::Int(1),
            CtValue::List(vec![CtValue::Int(256)]),
            CtValue::List(vec![CtValue::Str("not a byte".to_string())]),
        ] {
            let error = jet_as_bytes(&value, span)
                .expect_err("invalid byte value must be rejected");
            assert_eq!(error.code, "E0956");
            assert_eq!(error.what, "`this as_bytes call` can't run at compile time yet");
            assert_eq!(
                error.why,
                "the canonical TIR evaluator doesn't cover this construct yet"
            );
            assert_eq!(error.fix, "use a simpler form, or run via `jet build` / `jet run`");
        }
    }
}
