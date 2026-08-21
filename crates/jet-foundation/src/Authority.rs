//! D-AUTHORITY-MODEL1=A: one rights tree, one holds relation, one gate record.
//!
//! Authority facts are compile-time data. The parser, sema, package budget,
//! build evaluator, comptime checker, and REPL only consume this module's
//! names and laws. No authority value reaches TIR or generated Rust.

use crate::Diagnostics::{Diagnostic, Span};
use std::collections::BTreeSet;
use std::sync::LazyLock;

/// D-META-ONE1: the readable effect source is the only root-table input.
pub const EFFECT_SOURCE: &str = include_str!("../../jet-codegen/src/Prelude/Effects.jet");

/// Closed authority roots, read once from the embedded Prelude source.
///
/// `Panic` and `Mem` remain parseable deny-only rows, but are deliberately not
/// part of this grantable root table (D-PANICROOT1, D-AUTHORITY-MEM1).
pub static EFFECT_ROOTS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    EFFECT_SOURCE
        .lines()
        .filter_map(|line| line.trim().strip_prefix("effect "))
        .map(str::trim)
        .filter(|root| !root.contains('.'))
        .collect()
});

/// Typed view of the canonical effect-root table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    Net,
    FS,
    IO,
    DB,
    Time,
    Rand,
    Env,
    Exec,
    Log,
    GPU,
    Panic,
    FFI,
    Browser,
    Secret,
    Mem,
}

impl Effect {
    pub fn requires_comptime_gate(self) -> bool {
        matches!(
            self,
            Self::Net
                | Self::FS
                | Self::IO
                | Self::DB
                | Self::Env
                | Self::Exec
                | Self::Browser
                | Self::Secret
        )
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Net => "Net",
            Self::FS => "FS",
            Self::IO => "IO",
            Self::DB => "DB",
            Self::Time => "Time",
            Self::Rand => "Rand",
            Self::Env => "Env",
            Self::Exec => "Exec",
            Self::Log => "Log",
            Self::GPU => "GPU",
            Self::Panic => "Panic",
            Self::FFI => "FFI",
            Self::Browser => "Browser",
            Self::Secret => "Secret",
            Self::Mem => "Mem",
        }
    }

    pub fn parse(root: &str) -> Option<Self> {
        let canonical = parse_root(root)?;
        if canonical != root {
            return None;
        }
        Some(match canonical {
            "Net" => Self::Net,
            "FS" => Self::FS,
            "IO" => Self::IO,
            "DB" => Self::DB,
            "Time" => Self::Time,
            "Rand" => Self::Rand,
            "Env" => Self::Env,
            "Exec" => Self::Exec,
            "Log" => Self::Log,
            "GPU" => Self::GPU,
            "Panic" => Self::Panic,
            "FFI" => Self::FFI,
            "Browser" => Self::Browser,
            "Secret" => Self::Secret,
            "Mem" => Self::Mem,
            _ => return None,
        })
    }

    pub fn all() -> Holds {
        EFFECT_ROOTS.iter().map(|root| (*root).to_string()).collect()
    }
}

/// Build authority is a typed view over the same canonical root table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuildEffect {
    Net,
    FS,
    IO,
    DB,
    Time,
    Rand,
    Env,
    Exec,
    Log,
    GPU,
}

impl BuildEffect {
    pub const ALL: [Self; 10] = [
        Self::Net,
        Self::FS,
        Self::IO,
        Self::DB,
        Self::Time,
        Self::Rand,
        Self::Env,
        Self::Exec,
        Self::Log,
        Self::GPU,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Net => "Net",
            Self::FS => "FS",
            Self::IO => "IO",
            Self::DB => "DB",
            Self::Time => "Time",
            Self::Rand => "Rand",
            Self::Env => "Env",
            Self::Exec => "Exec",
            Self::Log => "Log",
            Self::GPU => "GPU",
        }
    }

    pub const fn flag(self) -> &'static str {
        match self {
            Self::Net => "net",
            Self::FS => "fs",
            Self::IO => "io",
            Self::DB => "db",
            Self::Time => "time",
            Self::Rand => "rand",
            Self::Env => "env",
            Self::Exec => "exec",
            Self::Log => "log",
            Self::GPU => "gpu",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.contains('.') {
            return None;
        }
        let canonical = parse_root(value)?;
        Self::ALL.into_iter().find(|effect| canonical == effect.name())
    }
}

/// Root segment of a dotted right (`FS.Read` → `FS`).
pub fn root(right: &str) -> &str {
    right.split('.').next().unwrap_or(right)
}

/// Resolve one root using the canonical table. Dotted input is accepted only
/// for its root; leaf declaration remains sema's job.
pub fn parse_root(right: &str) -> Option<&'static str> {
    let root = root(right);
    if root.eq_ignore_ascii_case("Panic") {
        return Some("Panic");
    }
    if root.eq_ignore_ascii_case("Mem") {
        return Some("Mem");
    }
    EFFECT_ROOTS
        .iter()
        .copied()
        .find(|known| known.eq_ignore_ascii_case(root))
}

/// Preserve a right's leaf spelling while normalizing its canonical root.
pub fn parse_right(right: &str) -> Option<String> {
    let right = right.trim();
    let root = parse_root(right)?;
    if root == "Panic" && right != "Panic" {
        return None;
    }
    Some(match right.split_once('.') {
        Some((_, leaf)) => format!("{root}.{leaf}"),
        None => root.to_string(),
    })
}

/// D-EFFTREE1: a bound covers itself and every descendant in the rights tree.
pub fn covers(bound: &str, right: &str) -> bool {
    let bound = parse_right(bound).unwrap_or_else(|| bound.trim().to_string());
    let right = parse_right(right).unwrap_or_else(|| right.trim().to_string());
    right == bound
        || right
            .strip_prefix(&bound)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

/// One rights carrier for every authority checkpoint.
pub type Holds = BTreeSet<String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Denied,
    Missing,
}

pub fn covers_any(bounds: &Holds, right: &str) -> bool {
    bounds.iter().any(|bound| covers(bound, right))
}

pub fn answer(held: &Holds, denied: &Holds, right: &str) -> Verdict {
    if covers_any(denied, right) {
        Verdict::Denied
    } else if covers_any(held, right) {
        Verdict::Allowed
    } else {
        Verdict::Missing
    }
}

/// D-AUTHORITY-MODEL1: inner scope may only tighten its parent's holds set.
pub fn tighten(outer: &Holds, inner: &Holds) -> bool {
    inner
        .iter()
        .all(|right| outer.iter().any(|bound| covers(bound, right)))
}

/// Rights in `used` not covered by any held right.
pub fn uncovered(used: &Holds, held: &Holds) -> Holds {
    used.iter()
        .filter(|right| !covers_any(held, right))
        .cloned()
        .collect()
}

/// Rights in `used` covered by a prohibition or other matching set.
pub fn covered(used: &Holds, matching: &Holds) -> Holds {
    used.iter()
        .filter(|right| covers_any(matching, right))
        .cloned()
        .collect()
}

/// D-MARK-SCOPE1: the lexical authority ladder, outer to inner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum Scope {
    Organization,
    Package,
    Module,
    Function,
    Block,
}

impl Scope {
    pub const ALL: [Self; 5] = [
        Self::Organization,
        Self::Package,
        Self::Module,
        Self::Function,
        Self::Block,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Organization => "organization",
            Self::Package => "package",
            Self::Module => "module",
            Self::Function => "function",
            Self::Block => "block",
        }
    }

    pub(crate) const fn rank(self) -> u8 {
        match self {
            Self::Organization => 0,
            Self::Package => 1,
            Self::Module => 2,
            Self::Function => 3,
            Self::Block => 4,
        }
    }
}

/// One kind for every written widening or audited fact move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GateKind {
    Unsafe,
    Impure,
    DependencyGrant,
    BuildFlag,
    SessionFlag,
    TrustGrant,
    ForcePin,
    TaintScrub,
    DutyDrop,
    StateTransition,
    PrecisionDemotion,
    Nondeterministic,
}

impl GateKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unsafe => "unsafe",
            Self::Impure => "impure",
            Self::DependencyGrant => "dependency_grant",
            Self::BuildFlag => "build_flag",
            Self::SessionFlag => "session_flag",
            Self::TrustGrant => "trust_grant",
            Self::ForcePin => "force_pin",
            Self::TaintScrub => "taint_scrub",
            Self::DutyDrop => "duty_drop",
            Self::StateTransition => "state_transition",
            Self::PrecisionDemotion => "precision_demotion",
            Self::Nondeterministic => "nondeterministic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "unsafe" | "unsafe_region" | "unsafe_fn" => Some(Self::Unsafe),
            "impure" => Some(Self::Impure),
            "dependency" | "dependency_grant" | "grant" => Some(Self::DependencyGrant),
            "build" | "build_flag" => Some(Self::BuildFlag),
            "session" | "session_flag" => Some(Self::SessionFlag),
            "trust" | "trust_grant" => Some(Self::TrustGrant),
            "force" | "force_pin" => Some(Self::ForcePin),
            "scrub" | "taint" | "taint_scrub" => Some(Self::TaintScrub),
            "drop" | "detach" | "duty" | "duty_drop" => Some(Self::DutyDrop),
            "state" | "transition" | "state_transition" => Some(Self::StateTransition),
            "approx"
            | "precision"
            | "precision_demotion"
            | "rounded"
            | "wrapping"
            | "saturating"
            | "checked" => Some(Self::PrecisionDemotion),
            "nondeterministic" | "determinism" => Some(Self::Nondeterministic),
            _ => None,
        }
    }

    pub const fn is_security(self) -> bool {
        matches!(
            self,
            Self::Unsafe
                | Self::Impure
                | Self::DependencyGrant
                | Self::BuildFlag
                | Self::SessionFlag
                | Self::TrustGrant
                | Self::ForcePin
                | Self::Nondeterministic
        )
    }

    pub const fn is_rights_kind(self) -> bool {
        self.is_security()
    }

    const fn display_order(self) -> u8 {
        match self {
            Self::Unsafe => 0,
            Self::Impure => 1,
            Self::Nondeterministic => 2,
            Self::DependencyGrant => 3,
            Self::BuildFlag => 4,
            Self::SessionFlag => 5,
            Self::TrustGrant => 6,
            Self::ForcePin => 7,
            Self::TaintScrub => 8,
            Self::DutyDrop => 9,
            Self::StateTransition => 10,
            Self::PrecisionDemotion => 11,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateOperation {
    pub kind: String,
    pub span: Span,
    pub required: Vec<String>,
    pub asserted: Vec<String>,
    pub discharged: bool,
}

/// D-AUTHORITY-GATE1: the one record shape for every gate source.
#[derive(Debug, Clone)]
pub struct GateEntry {
    pub kind: GateKind,
    pub domain: String,
    pub scope: String,
    pub source: String,
    pub span: Option<Span>,
    pub subject: String,
    pub reason: Option<String>,
    pub status: Option<String>,
    pub detail: String,
    pub provenance: Vec<String>,
    pub operations: Vec<GateOperation>,
}

#[derive(Debug, Clone)]
pub struct GateDiagnostic {
    pub source: String,
    pub diagnostic: Diagnostic,
}

/// Merged authority-gate read model. Writers stay in their owning subsystems;
/// all readers append this same record and retain every provenance source.
#[derive(Debug, Clone, Default)]
pub struct GateLedger {
    entries: Vec<GateEntry>,
    diagnostics: Vec<GateDiagnostic>,
}

impl GateLedger {
    pub fn entries(&self) -> &[GateEntry] {
        &self.entries
    }

    pub fn diagnostics(&self) -> &[GateDiagnostic] {
        &self.diagnostics
    }

    pub fn set_diagnostics(&mut self, diagnostics: Vec<GateDiagnostic>) {
        self.diagnostics = diagnostics;
    }

    /// Add one gate while coalescing the same fact with another provenance
    /// source. The ledger never drops provenance.
    pub fn push(&mut self, mut entry: GateEntry) {
        if entry.provenance.is_empty() {
            entry.provenance.push(entry.source.clone());
        }
        if let Some(existing) = self.entries.iter_mut().find(|candidate| same_fact(candidate, &entry)) {
            for provenance in entry.provenance {
                if !existing.provenance.contains(&provenance) {
                    existing.provenance.push(provenance);
                }
            }
            if existing.reason.is_none() {
                existing.reason = entry.reason;
            }
            if existing.status.is_none() {
                existing.status = entry.status;
            }
            existing.provenance.sort();
            return;
        }
        entry.provenance.sort();
        self.entries.push(entry);
    }

    pub fn sort(&mut self) {
        self.entries.sort_by(|left, right| {
            (
                !left.kind.is_security(),
                left.kind.display_order(),
                left.kind.name(),
                left.source.as_str(),
                left.span.map(|span| span.start).unwrap_or(usize::MAX),
                left.span.map(|span| span.end).unwrap_or(usize::MAX),
                left.subject.as_str(),
                left.detail.as_str(),
            )
                .cmp(&(
                    !right.kind.is_security(),
                    right.kind.display_order(),
                    right.kind.name(),
                    right.source.as_str(),
                    right.span.map(|span| span.start).unwrap_or(usize::MAX),
                    right.span.map(|span| span.end).unwrap_or(usize::MAX),
                    right.subject.as_str(),
                    right.detail.as_str(),
                ))
        });
    }
}

fn same_fact(left: &GateEntry, right: &GateEntry) -> bool {
    left.kind == right.kind
        && left.domain == right.domain
        && left.scope == right.scope
        && left.subject == right.subject
        && left.detail == right.detail
        && match (left.span, right.span) {
            (None, None) => true,
            (Some(left_span), Some(right_span)) => {
                left_span == right_span && left.source == right.source
            }
            _ => false,
        }
}

/// Shared purity classification consumed by both purity walkers.
pub fn builtin_effect(name: &str) -> Option<Effect> {
    crate::Syntax::IMPURE_BUILTINS
        .contains(&name)
        .then_some(Effect::IO)
}

/// Core calls that consume ambient input rather than only transforming values.
pub fn is_impure_core(module: &str, method: &str) -> bool {
    matches!(
        (module, method),
        (
            "core.term",
            "stdin" | "input" | "confirm" | "choose" | "input_secret" | "read_all_input"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rights_use_one_tree_and_only_tighten() {
        let outer = Holds::from(["FS".to_string(), "Net".to_string()]);
        let inner = Holds::from([
            "FS.Read".to_string(),
            "Net".to_string(),
            "DB".to_string(),
        ]);
        assert!(covers("FS", "FS.Read"));
        assert!(!tighten(&outer, &inner));
        assert!(!tighten(&inner, &outer));
        assert_eq!(uncovered(&inner, &outer), Holds::from(["DB".to_string()]));
    }

    #[test]
    fn every_checkpoint_uses_one_canonical_answer() {
        let held = Holds::from(["FS".to_string()]);
        let denied = Holds::from(["Secret".to_string()]);
        let missing = Holds::new();
        let right = parse_right("fs.Read").expect("known right");
        assert_eq!(right, "FS.Read");
        assert_eq!(parse_right("fs.read").as_deref(), Some("FS.read"));
        let checkpoints = [
            ("compile", answer(&held, &denied, &right)),
            ("build", answer(&held, &denied, &right)),
            ("session", answer(&held, &denied, &right)),
            ("repl", answer(&held, &denied, &right)),
        ];
        assert!(checkpoints
            .iter()
            .all(|(_, verdict)| *verdict == Verdict::Allowed));
        assert_eq!(
            answer(&held, &Holds::from(["FS".to_string()]), &right),
            Verdict::Denied
        );
        assert_eq!(answer(&held, &denied, "Secret"), Verdict::Denied);
        assert_eq!(answer(&held, &denied, "Net"), Verdict::Missing);
        assert_eq!(
            answer(&held, &missing, "FS.Read"),
            answer(&held, &missing, "fs.Read")
        );
    }

    #[test]
    fn one_gate_record_keeps_provenance() {
        let mut ledger = GateLedger::default();
        let entry = |provenance: &str| GateEntry {
            kind: GateKind::TrustGrant,
            domain: "security".to_string(),
            scope: "package".to_string(),
            source: "package.jet".to_string(),
            span: None,
            subject: "dep".to_string(),
            reason: None,
            status: Some("recorded".to_string()),
            detail: "FS.Read".to_string(),
            provenance: vec![provenance.to_string()],
            operations: Vec::new(),
        };
        ledger.push(entry("lockfile"));
        ledger.push(entry("trust store"));
        assert_eq!(ledger.entries().len(), 1);
        assert_eq!(ledger.entries()[0].provenance.len(), 2);
    }

    #[test]
    fn effect_roots_are_the_thirteen_grantable_roots() {
        assert_eq!(
            EFFECT_ROOTS.as_slice(),
            &[
                "Net", "FS", "IO", "DB", "Time", "Rand", "Env", "Exec", "Log", "GPU",
                "FFI", "Browser", "Secret",
            ]
        );
        assert_eq!(Effect::all().len(), 13);
        assert_eq!(parse_right("Panic").as_deref(), Some("Panic"));
        assert_eq!(parse_right("Mem.Alloc").as_deref(), Some("Mem.Alloc"));
    }

    #[test]
    fn every_ffi_language_leaf_is_covered_by_the_ffi_root() {
        for leaf in crate::Syntax::BUILTIN_EFFECT_LEAVES
            .iter()
            .copied()
            .filter(|leaf| leaf.starts_with("FFI."))
        {
            assert!(covers("FFI", leaf), "FFI must cover {leaf}");
            assert!(parse_right(leaf).is_some(), "leaf must parse: {leaf}");
            assert_eq!(
                answer(&Holds::new(), &Holds::from(["FFI".to_string()]), leaf),
                Verdict::Denied,
                "FFI denial must cover {leaf}"
            );
        }
    }

    #[test]
    fn retired_flat_ffi_spellings_do_not_parse() {
        for root in [
            "Go", "Java", "DotNet", "Fortran", "Cobol", "Tcl", "Lua", "Ada", "Pascal",
            "Dart", "PowerShell", "Perl", "Ruby", "Php", "R", "Com", "Cpp", "Py", "Octave",
        ] {
            assert!(parse_right(root).is_none(), "retired spelling parsed: {root}");
            assert!(Effect::parse(root).is_none(), "retired root parsed: {root}");
        }
    }
}
