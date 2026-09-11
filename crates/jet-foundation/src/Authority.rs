//! D-AUTHORITY-MODEL1=A: one rights tree, one holds relation, one gate record.
//!
//! Authority names and laws are compile-time facts. Named `#FX` scopes
//! also lower to the shared Prelude's ordinary runtime carrier; this module
//! does not define a second value or policy representation.

use crate::Diagnostics::{Diagnostic, Span};
use std::collections::BTreeSet;

pub use crate::BuildEffects::BuildEffect;
pub use crate::Effects::{
    effect_declarations, Effect, EffectDeclaration, EFFECT_DECLARATIONS, EFFECT_ROOTS,
    EFFECT_SOURCE,
};

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
/// True when the source-declared irreversible fact covers `right`.
///
/// A root declaration such as `effect FFI @irreversible` covers every
/// foreign leaf through the same ancestor relation used by authority rows.
pub fn is_declared_irreversible(right: &str) -> bool {
    let canonical = parse_right(right).unwrap_or_else(|| right.trim().to_string());
    effect_declarations()
        .iter()
        .filter(|declaration| declaration.irreversible)
        .any(|declaration| covers(declaration.name, &canonical))
}

/// D-EFFTREE1: a bound covers itself and every descendant in the rights tree.
///
/// Resource-qualified rights use `:` for hosts and paths (for example,
/// `FS.Read:/data` or `Net.Connect:api.example.com`). A qualified descendant
/// must cross a separator boundary, so `/data` does not accidentally cover
/// `/database`.
pub fn covers(bound: &str, right: &str) -> bool {
    let bound = parse_right(bound).unwrap_or_else(|| bound.trim().to_string());
    let right = parse_right(right).unwrap_or_else(|| right.trim().to_string());
    let qualified = bound.contains(':');
    right == bound
        || right
            .strip_prefix(&bound)
            .is_some_and(|suffix| {
                suffix.as_bytes().first().is_some_and(|byte| {
                    if qualified {
                        *byte == b'/'
                    } else {
                        matches!(byte, b'.' | b':' | b'/')
                    }
                })
            })
}

/// One rights carrier for every authority checkpoint.
pub type Holds = BTreeSet<String>;

/// D-AUTHORITY-MODEL1: the canonical authority value projected to runtime
/// adapters. It owns the same `Holds` relation used by compile/build/session
/// policy; sandbox wrappers must not maintain a second grant set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Authority {
    holds: Holds,
}

/// A requested scope that would widen its parent authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TightenError {
    pub uncovered: Holds,
}

impl std::fmt::Display for TightenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let rights = self.uncovered.iter().cloned().collect::<Vec<_>>().join(", ");
        write!(f, "authority scope widens its parent for `{rights}`")
    }
}

impl std::error::Error for TightenError {}

impl Authority {
    /// Construct an authority from canonical or resource-qualified rights.
    /// Unknown spellings are retained for the caller's diagnostic; policy
    /// checks still fail closed because they are never covered by a known hold.
    pub fn from_rights<I, S>(rights: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let holds = rights
            .into_iter()
            .map(Into::into)
            .map(|right| parse_right(&right).unwrap_or(right))
            .collect();
        Self { holds }
    }

    pub fn from_holds(holds: Holds) -> Self {
        Self { holds }
    }

    pub fn holds(&self) -> &Holds {
        &self.holds
    }

    pub fn rights(&self) -> impl Iterator<Item = &str> {
        self.holds.iter().map(String::as_str)
    }

    pub fn allows(&self, right: &str) -> bool {
        covers_any(&self.holds, right)
    }

    /// True when this authority can satisfy a host operation whose declared
    /// right is `right`.  Unlike `allows`, this also accepts a resource
    /// qualified hold (`FS.Read:/data`) for the unqualified operation
    /// (`FS.Read`).  The operation is still bounded by the held descendant;
    /// callers must apply that resource to their path/host argument.
    pub fn allows_operation(&self, right: &str) -> bool {
        self.allows(right) || self.holds.iter().any(|held| covers(right, held))
    }

    /// Check one typed host import against this authority without letting an
    /// adapter reinterpret the right or keep a parallel grant table.
    pub fn decide_import(&self, import: &HostImportFact) -> Verdict {
        if self.allows_operation(import.required_grant()) {
            Verdict::Allowed
        } else {
            Verdict::Missing
        }
    }

    /// Return a child authority only when every requested right is covered by
    /// this authority. The child retains exactly the requested scope.
    pub fn tighten(&self, requested: &Holds) -> Result<Self, TightenError> {
        let uncovered = uncovered(requested, &self.holds);
        if uncovered.is_empty() {
            Ok(Self::from_holds(requested.clone()))
        } else {
            Err(TightenError { uncovered })
        }
    }
}

/// Stable, typed result of checking one host import.  This is deliberately
/// data-only so AOT, JIT, interpreter, web, and host adapters all marshal the
/// same authority verdict rather than re-solving policy locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostImportDecision {
    pub import: HostImportFact,
    pub verdict: Verdict,
}

impl HostImportDecision {
    pub fn check(authority: &Authority, import: HostImportFact) -> Self {
        let verdict = authority.decide_import(&import);
        Self { import, verdict }
    }

    pub fn is_allowed(&self) -> bool {
        self.verdict == Verdict::Allowed
    }
}

impl HostImportFact {
    pub fn decision(&self, authority: &Authority) -> HostImportDecision {
        HostImportDecision::check(authority, self.clone())
    }
}

/// Backend-neutral description of one host-provided operation. Payloads are
/// copied at the boundary and refer to the existing interface snapshot type
/// identities rather than introducing a second wire type registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostImportFact {
    pub id: String,
    pub operation: String,
    pub required_right: String,
    pub parameter_type_ids: Vec<String>,
    pub result_type_id: Option<String>,
}

impl HostImportFact {
    pub fn new(
        id: impl Into<String>,
        operation: impl Into<String>,
        required_right: impl Into<String>,
        parameter_type_ids: impl IntoIterator<Item = String>,
        result_type_id: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            operation: operation.into(),
            required_right: required_right.into(),
            parameter_type_ids: parameter_type_ids.into_iter().collect(),
            result_type_id,
        }
    }

    /// All payload type identities crossed by this import. The host owns
    /// copying and validation; no borrowed or untyped payload is admitted.
    pub fn payload_type_ids(&self) -> impl Iterator<Item = &str> {
        self.parameter_type_ids
            .iter()
            .map(String::as_str)
            .chain(self.result_type_id.iter().map(String::as_str))
    }

    pub fn required_grant(&self) -> &str {
        &self.required_right
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Denied,
    Missing,
}

/// D-EFFECT-AUTHORITY1: the application boundary's one checked policy fact.
///
/// Sema owns `required_effects`; the package loader owns the initial policy
/// rows; an interactive CLI may replace those rows with its once/project
/// decision. Every execution tier receives this same carrier through the
/// checked `ProgramBundle`. It is deliberately separate from `JetAuthority`,
/// which is the ordinary source-level value lowered by the Prelude.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationAuthority {
    pub required_effects: Holds,
    pub granted_effects: Holds,
    pub denied_effects: Holds,
    pub authority: String,
}

impl Default for ApplicationAuthority {
    fn default() -> Self {
        Self::ambient_basics()
    }
}

impl ApplicationAuthority {
    /// D-AUTH-AMBIENT1=A: a manifest-less application may use the beginner
    /// basics without an authority ceremony. A package manifest replaces this
    /// default with its explicit holds, so expert deny/audit control remains.
    pub const AMBIENT_BASIC_EFFECTS: [&'static str; 3] = ["IO", "Mem.Alloc", "Exec"];

    pub fn ambient_basics() -> Self {
        Self {
            required_effects: Holds::new(),
            granted_effects: Self::AMBIENT_BASIC_EFFECTS
                .iter()
                .map(|effect| (*effect).to_string())
                .collect(),
            denied_effects: Holds::new(),
            authority: "application default".to_string(),
        }
    }

    /// Project the parsed `authority.holds` rows without teaching an engine
    /// how to parse package policy.
    pub fn from_policy(
        allow: Option<&[String]>,
        deny: Option<&[String]>,
        authority: impl Into<String>,
    ) -> Self {
        let parse = |names: Option<&[String]>| {
            names
                .into_iter()
                .flatten()
                .filter_map(|name| parse_right(name))
                .collect()
        };
        Self {
            required_effects: Holds::new(),
            granted_effects: parse(allow),
            denied_effects: parse(deny),
            authority: authority.into(),
        }
    }

    /// Required effects with no policy verdict at the application boundary.
    /// `Panic` is an internal deny-only stop row, not a positive authority
    /// request. An explicit `Panic` denial is still reported below.
    pub fn undecided_effects(&self) -> Holds {
        self.required_effects
            .iter()
            .filter(|effect| {
                root(effect) != Effect::Panic.name()
                    && answer(&self.granted_effects, &self.denied_effects, effect)
                        == Verdict::Missing
            })
            .cloned()
            .collect()
    }

    /// Required effects that the policy explicitly denies.
    pub fn denied_required_effects(&self) -> Holds {
        self.required_effects
            .iter()
            .filter(|effect| answer(&self.granted_effects, &self.denied_effects, effect) == Verdict::Denied)
            .cloned()
            .collect()
    }

    pub fn is_allowed(&self) -> bool {
        self.undecided_effects().is_empty() && self.denied_required_effects().is_empty()
    }

    /// Render the one-line manifest fix from the complete undecided set.
    /// Denied effects are never turned into an allow suggestion.
    pub fn policy_fix(&self) -> String {
        let undecided = self.undecided_effects();
        let policy_step = if undecided.is_empty() {
            "adjust the denial in `authority.holds.deny`".to_string()
        } else {
            let effects = undecided
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            format!("add `allow: [{effects}]` under `authority.holds` in `package.jet`")
        };
        format!(
            "{policy_step}; otherwise deny effects deliberately, or approve the exact operation once or for the project in an interactive terminal"
        )
    }

    /// Structured refusal shared by the interpreter and JIT adapters. CLI
    /// approval happens before those adapters and updates this same carrier.
    pub fn policy_diagnostic(&self) -> Option<Diagnostic> {
        if self.is_allowed() {
            return None;
        }
        let denied = self.denied_required_effects();
        let undecided = self.undecided_effects();
        let render = |rights: &Holds| {
            if rights.is_empty() {
                "none".to_string()
            } else {
                rights.iter().cloned().collect::<Vec<_>>().join(", ")
            }
        };
        let denied_text = render(&denied);
        let denied_policy_text = render(&self.denied_effects);
        let granted_text = render(&self.granted_effects);
        let undecided_text = render(&undecided);
        let required_text = render(&self.required_effects);
        Some(
            Diagnostic::error(
                "E1803",
                if denied.is_empty() {
                    format!("application authority is undecided for `{undecided_text}`")
                } else {
                    format!("application authority denies `{denied_text}`")
                },
                format!(
                    "required_effects={required_text}; granted_effects={granted_text}; denied_effects={denied_policy_text}; denied_required_effects={denied_text}; undecided_effects={undecided_text}; authority={}",
                    self.authority
                ),
                self.policy_fix(),
                None,
            )
            .with_rights_chain(
                "authority",
                std::iter::empty::<String>(),
                std::iter::once(self.authority.clone()),
                None,
            ),
        )
    }
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
    Structure,
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
            Self::Structure => "structure",
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
            "approx" | "precision" | "precision_demotion" | "rounded" | "wrapping"
            | "saturating" | "checked" => Some(Self::PrecisionDemotion),
            "nondeterministic" | "determinism" => Some(Self::Nondeterministic),
            "structure" => Some(Self::Structure),
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
            Self::Structure => 12,
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
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|candidate| same_fact(candidate, &entry))
        {
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
        let inner = Holds::from(["FS.Read".to_string(), "Net".to_string(), "DB".to_string()]);
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
                "Net", "FS", "IO", "DB", "Time", "Rand", "Env", "Exec", "Log", "GPU", "FFI",
                "Browser", "Secret",
            ]
        );
        assert_eq!(Effect::all().len(), 13);
        assert_eq!(parse_right("Panic").as_deref(), Some("Panic"));
        assert_eq!(parse_right("Mem.Alloc").as_deref(), Some("Mem.Alloc"));
    }

    #[test]
    fn manifestless_application_default_grants_beginner_basics() {
        let authority = ApplicationAuthority::default();
        assert_eq!(
            authority.granted_effects,
            Holds::from([
                "IO".to_string(),
                "Mem.Alloc".to_string(),
                "Exec".to_string(),
            ])
        );
        assert!(authority.denied_effects.is_empty());
        assert_eq!(authority.authority, "application default");
    }

    #[test]
    fn authority_diagnostic_fix_lists_all_undecided_effects() {
        let authority = ApplicationAuthority {
            required_effects: Holds::from([
                "IO".to_string(),
                "Mem.Alloc".to_string(),
                "Exec".to_string(),
            ]),
            granted_effects: Holds::new(),
            denied_effects: Holds::new(),
            authority: "package.jet authority.holds".to_string(),
        };
        let diagnostic = authority.policy_diagnostic().expect("E1803");
        assert!(diagnostic.fix.contains("allow: [Exec, IO, Mem.Alloc]"));
    }

    #[test]
    fn application_authority_does_not_request_a_positive_panic_grant() {
        let mut authority = ApplicationAuthority {
            required_effects: Holds::from(["IO".to_string(), "Panic".to_string()]),
            granted_effects: Holds::from(["IO".to_string()]),
            denied_effects: Holds::new(),
            authority: "application default".to_string(),
        };
        assert!(authority.undecided_effects().is_empty());
        assert!(authority.is_allowed());
        assert!(authority.policy_diagnostic().is_none());

        authority.denied_effects.insert("Panic".to_string());
        let diagnostic = authority.policy_diagnostic().expect("E1803");
        assert!(diagnostic.what.contains("Panic"));
        assert!(diagnostic
            .fix
            .to_ascii_lowercase()
            .contains("adjust the denial"));
        assert!(!diagnostic.fix.contains("allow: [Panic]"));
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
            "Go",
            "Java",
            "DotNet",
            "Fortran",
            "Cobol",
            "Tcl",
            "Lua",
            "Ada",
            "Pascal",
            "Dart",
            "PowerShell",
            "Perl",
            "Ruby",
            "Php",
            "R",
            "Com",
            "Cpp",
            "Py",
            "Octave",
        ] {
            assert!(
                parse_right(root).is_none(),
                "retired spelling parsed: {root}"
            );
            assert!(Effect::parse(root).is_none(), "retired root parsed: {root}");
        }
    }
    #[test]
    fn resource_qualified_rights_keep_tree_boundaries() {
        assert!(covers("FS.Read", "FS.Read:/data/file"));
        assert!(covers("Net.Connect", "Net.Connect:api.example.com"));
        assert!(!covers("FS.Read:/data", "FS.Read:/database"));
        assert!(!covers("Net.Connect:api.example.com", "Net.Connect:api.example.com.evil"));
    }

    #[test]
    fn canonical_authority_tightens_without_widening() {
        let host = Authority::from_rights(["FS.Read", "Net.Connect"]);
        let child = Holds::from(["FS.Read:/data".to_string()]);
        let narrowed = host.tighten(&child).expect("child scope is covered");
        assert!(narrowed.allows("FS.Read:/data/file"));
        assert!(!narrowed.allows("FS.Read:/other"));
        assert!(host.tighten(&Holds::from(["Exec".to_string()])).is_err());
    }
}
