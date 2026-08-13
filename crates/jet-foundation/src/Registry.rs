//! D-META-REG1=A: one registration table.
//!
//! A marker rule, a knowledge plane, a right, and a build fact are rows of the
//! same table, separated only by what they attach to. Reflection, `jet explain`,
//! and the drift guards are written once here and serve all four kinds; nothing
//! downstream keeps a second table or a second guard per kind.
//!
//! D-FACT-LAW1=B puts the law on the row itself. A fact moves toward safety
//! silently; every move away is one written word at the site. So every row
//! states its safe direction and the gate words that move it the other way. A
//! row with no meaningful direction states `None` and names no gate;
//! `law_violations` fails a row that states one without the other.
//!
//! D-FACT-OWN1=A adds one row shape for a fact a prover publishes. The
//! ownership prover is never a plane: it publishes sendability, view
//! provenance, and moved-ness as read-only rows with no plane algebra, so tools
//! and other planes read them like any other fact.
//!
//! D-ONCE-LAW1=A adds the fifth use of the same table: a truth the compiler's
//! own corpus states once. Such a row names its home, everything that renders
//! the truth from that home, and the guard that proves no second copy exists.
//! A registered row with no guard fails `law_violations`, which is the one
//! lint the whole table shares — there is no second guard engine.

use std::sync::LazyLock;

use crate::Diagnostics::{ReportMoment, Severity};
use crate::Policy::{AppliedRule, RuleSite, APPLIED_RULES};

/// What a row attaches to. This is the whole difference between the six uses
/// of the one table, so `RowKind` is read off the target rather than stated
/// twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowTarget {
    /// A rule on written code. The sites are the row's legal attachment points.
    Code(&'static [RuleSite]),
    /// Knowledge about a value.
    Value,
    /// What a scope may do.
    Scope,
    /// What the build knows.
    Build,
    /// D-ONCE-LAW1=A: the compiler's own source. A truth stated once in one
    /// file, rendered from there by everything that needs it.
    Corpus,
    /// D-REPORT-HOME1=A: a typed compile-time diagnostic row.
    Report,
}

/// The six uses of the one table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Marker,
    Plane,
    Right,
    Fact,
    Truth,
    Diagnostic,
}

impl RowKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Marker => "marker",
            Self::Plane => "plane",
            Self::Right => "right",
            Self::Fact => "fact",
            Self::Truth => "truth",
            Self::Diagnostic => "diagnostic",
        }
    }
}

impl RowTarget {
    pub const fn kind(self) -> RowKind {
        match self {
            Self::Code(_) => RowKind::Marker,
            Self::Value => RowKind::Plane,
            Self::Scope => RowKind::Right,
            Self::Build => RowKind::Fact,
            Self::Corpus => RowKind::Truth,
            Self::Report => RowKind::Diagnostic,
        }
    }
}

/// D-ONCE-LAW1=A: how a guard proves there is no second copy.
///
/// There are two ways and no third. A guard either runs both paths and
/// compares the answers, or it counts the places the truth is defined and
/// holds that count. Checking that the one home still exists proves nothing —
/// a second copy passes that check — so it is not a variant and cannot be
/// written down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardProof {
    /// Runs the truth through every renderer and compares the answers.
    DiffsBehavior,
    /// Counts the definition sites in the corpus and refuses a second one.
    CountsSites,
}

impl GuardProof {
    pub const fn name(self) -> &'static str {
        match self {
            Self::DiffsBehavior => "diffs behavior",
            Self::CountsSites => "counts definition sites",
        }
    }
}

/// D-ONCE-LAW1=A: the proof that a truth has one home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guard {
    /// The test function that runs the proof.
    pub test: &'static str,
    /// The file that holds it.
    pub file: &'static str,
    /// What the proof does. See `GuardProof`.
    pub proof: GuardProof,
}

/// D-FACT-LAW1=B / D-FACT-WORD1=A: the direction a row's facts move for free.
/// The law reads "tighten" and "loosen" in every diagnostic and doc; this column
/// says what tightening *is* for this row, because it is a different act per
/// plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeDirection {
    /// Learning more is free. Exactness, flow facts, taint.
    Gain,
    /// Giving up power is free. Rights, package policy, build settings.
    Shrink,
    /// Finishing the job is free. Duty: a bound handle owes `join`.
    Discharge,
    /// This row holds no fact that moves, so it states no direction. A rule on
    /// written code is the ordinary case; so is a read-only prover row.
    None,
}

impl SafeDirection {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gain => "gain",
            Self::Shrink => "shrink",
            Self::Discharge => "discharge",
            Self::None => "none",
        }
    }
}

/// One row of the one table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryRow {
    pub name: &'static str,
    /// What the row attaches to. Also names its kind.
    pub target: RowTarget,
    /// D-TYPE2-FOUND1: whether facts on this plane contribute to type
    /// identity. Obligations are registered too, but compare by subsumption.
    pub identity_bearing: bool,
    /// D-FACT-LAW1=B: which way this row's facts move for free.
    pub safe_direction: SafeDirection,
    /// D-FACT-LAW1=B: the written words that move them the other way.
    pub gates: &'static [&'static str],
    /// D-FACT-OWN1=A: the prover that publishes this row, for a read-only row
    /// that carries no plane algebra. `None` for a declared row.
    pub published_by: Option<&'static str>,
    /// The marker signature, for a row whose target is written code.
    pub rule: Option<&'static AppliedRule>,
    /// D-ONCE-LAW1=A: the one file that owns this truth, repository-relative.
    /// A corpus row names one; every other kind names none.
    pub home: Option<&'static str>,
    /// D-ONCE-LAW1=A: everything that renders the truth from its home. A
    /// corpus row names at least one; every other kind names none.
    pub renderers: &'static [&'static str],
    /// D-ONCE-LAW1=A: the proof that no second copy exists. A corpus row names
    /// one; every other kind names none. A registered corpus row with no guard
    /// fails `law_violations`.
    pub guard: Option<Guard>,
    /// D-REPORT-HOME1=A: the typed diagnostic payload for a report row.
    pub diagnostic: Option<&'static DiagnosticRow>,
    /// The ratified decision this row answers to.
    pub decision: &'static str,
}

impl RegistryRow {
    pub const fn kind(&self) -> RowKind {
        self.target.kind()
    }

    /// True for a read-only row a prover publishes (D-FACT-OWN1=A).
    pub const fn is_prover_supplied(&self) -> bool {
        self.published_by.is_some()
    }

    pub const fn is_identity_bearing(&self) -> bool {
        self.identity_bearing
    }
}

/// One marker row together with the values recovered from a dedicated AST
/// node. Block constructs keep their typed fields; this view lets tools render
/// and reflect them without storing a second `Marker` node.
#[derive(Debug, Clone)]
pub struct MarkerRowAndArgs {
    pub row: &'static RegistryRow,
    pub negated: bool,
    pub args: Vec<MarkerArgument>,
}

#[derive(Debug, Clone)]
pub enum MarkerArgument {
    Expr {
        label: Option<String>,
        value: crate::AST::Expr,
    },
    Ident(String),
    Idents {
        label: Option<String>,
        values: Vec<String>,
    },
    Text(String),
    Policy(Vec<crate::Policy::PolicyDeclaration>),
}

/// Look up one marker application in the one table.
pub fn marker_row_and_args(
    name: &str,
    negated: bool,
    args: Vec<MarkerArgument>,
) -> Option<MarkerRowAndArgs> {
    let row = row(name)?;
    Some(MarkerRowAndArgs { row, negated, args })
}

fn marker_arguments(marker: &crate::AST::Marker) -> Vec<MarkerArgument> {
    marker
        .args
        .iter()
        .enumerate()
        .map(|(index, value)| MarkerArgument::Expr {
            label: marker
                .arg_labels
                .get(index)
                .and_then(|label| label.as_ref())
                .map(|(name, _)| name.clone()),
            value: value.clone(),
        })
        .collect()
}

/// Return the registry row and typed argument values for every dedicated
/// block-construct node. The mapping is kept here so formatter, reflection,
/// and future editor consumers share one accessor rather than matching the
/// `Stmt` variants independently.
pub fn block_marker(
    stmt: &crate::AST::Stmt,
    policies: &[crate::Policy::PolicyDeclaration],
) -> Option<MarkerRowAndArgs> {
    use crate::AST::Stmt;

    let (name, args) = match stmt {
        Stmt::Unsafe {
            audit,
            audit_expr,
            span,
            ..
        } => {
            let mut args = Vec::new();
            if let Some(value) = audit_expr {
                args.push(MarkerArgument::Expr {
                    label: None,
                    value: value.clone(),
                });
            } else if let Some(reason) = audit {
                args.push(MarkerArgument::Text(reason.clone()));
            }
            if let Some(mode) = policies
                .iter()
                .find(|declaration| {
                    declaration.key == crate::Policy::PolicyKey::Unsafe
                        && declaration.target == Some(*span)
                        && matches!(
                            declaration.value,
                            crate::Policy::PolicyValue::Track
                                | crate::Policy::PolicyValue::Skip
                        )
                })
                .map(|declaration| declaration.value.display())
            {
                args.push(MarkerArgument::Expr {
                    label: Some("obligations".to_string()),
                    value: crate::AST::Expr::Ident(mode, *span),
                });
            }
            (crate::Syntax::KW_UNSAFE, args)
        }
        Stmt::Impure {
            reason,
            reason_expr,
            ..
        } => {
            let args = if let Some(value) = reason_expr {
                vec![MarkerArgument::Expr {
                    label: None,
                    value: value.clone(),
                }]
            } else {
                reason
                    .as_ref()
                    .map(|value| vec![MarkerArgument::Text(value.clone())])
                    .unwrap_or_default()
            };
            (crate::Syntax::KW_IMPURE, args)
        }
        Stmt::Reactive { .. } => (crate::Syntax::KW_REACTIVE, Vec::new()),
        Stmt::Shield { .. } => (crate::Syntax::KW_SHIELD, Vec::new()),
        Stmt::Region { name, .. } => (
            crate::Syntax::MARKER_REGION,
            vec![MarkerArgument::Ident(name.clone())],
        ),
        Stmt::Policy { declarations, .. } => (
            crate::Syntax::MARKER_POLICY,
            vec![MarkerArgument::Policy(declarations.clone())],
        ),
        Stmt::Caps { caps, .. } => (
            crate::Syntax::KW_CAPS,
            vec![MarkerArgument::Idents {
                label: None,
                values: caps.iter().map(|(name, _)| name.clone()).collect(),
            }],
        ),
        Stmt::Grant {
            caps, binding, ..
        } => (
            crate::Syntax::KW_GRANT,
            vec![MarkerArgument::Idents {
                label: Some(binding.clone()),
                values: caps.iter().map(|(name, _)| name.clone()).collect(),
            }],
        ),
        Stmt::ContextBlock { fields, .. } => (
            crate::Syntax::CTX_BLOCK,
            fields
                .iter()
                .map(|(name, value, _)| MarkerArgument::Expr {
                    label: Some(name.clone()),
                    value: value.clone(),
                })
                .collect(),
        ),
        Stmt::Live { .. } => (crate::Syntax::MARKER_LIVE, Vec::new()),
        Stmt::AssumeDet { reason_expr, .. } => (
            crate::Syntax::MARKER_NONDETERMINISTIC,
            vec![MarkerArgument::Expr {
                label: None,
                value: reason_expr.clone(),
            }],
        ),
        Stmt::Transact { name, .. } => (
            crate::Syntax::KW_TRANSACT,
            name.iter()
                .cloned()
                .map(MarkerArgument::Ident)
                .collect(),
        ),
        Stmt::Switched { marker, .. } => {
            return marker_row_and_args(&marker.name, marker.negated, marker_arguments(marker));
        }
        _ => return None,
    };
    marker_row_and_args(name, false, args)
}

/// Expression markers use the same row lookup as block markers. A value-tag
/// prefix may resolve to a fact row rather than an applied-rule row; both are
/// still rows in the one registry.
pub fn expression_marker(expr: &crate::AST::Expr) -> Option<MarkerRowAndArgs> {
    match expr {
        crate::AST::Expr::Todo { .. } => {
            marker_row_and_args(crate::Syntax::KW_TODO, false, Vec::new())
        }
        crate::AST::Expr::Tainted(_, Some(name), _) => marker_row_and_args(name, false, Vec::new()),
        _ => None,
    }
}

/// The callable formatter's line break is a row property, not a formatter
/// vocabulary list. The rows below retain the established readable layout;
/// all other callable markers stay beside `fn`.
pub fn callable_marker_needs_line_break(name: &str) -> bool {
    let Some(row) = row(name) else { return false };
    matches!(
        row.name,
        crate::Syntax::MARKER_META
            | crate::Syntax::KW_UNSAFE
            | crate::Syntax::MARKER_FFI
            | crate::Syntax::MARKER_TARGET
            | crate::Syntax::MARKER_WASM_EXPORT
    )
}

/// The raw lexer needs this row predicate to recognize an inline foreign
/// source string. It derives the rule from the registered signature instead of
/// naming a marker in the lexer.
pub fn is_inline_foreign_marker(name: &str) -> bool {
    let Some(rule) = row(name).and_then(|row| row.rule) else {
        return false;
    };
    rule.sites.len() == 1
        && rule.sites[0] == RuleSite::Function
        && rule.signature.params.len() == 1
        && rule.signature.params[0].source_type == "FfiLanguage"
}

/// The lexer keeps arbitrary-precision integers only in the registered type
/// marker whose signature identifies unit-family conversion metadata.
pub fn is_exact_integer_metadata_marker(name: &str) -> bool {
    let Some(rule) = row(name).and_then(|row| row.rule) else {
        return false;
    };
    rule.sites.len() == 1
        && rule.sites[0] == RuleSite::Type
        && rule.signature.params.len() == 3
        && rule.signature.params[0].source_type == "Ident"
        && rule.signature.params[1].source_type == "Value"
        && rule.signature.params[2].source_type == "Ident"
}

/// Whether a diagnostic row is currently produced by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticStatus {
    Active,
    Retired,
    Reserved,
}

impl DiagnosticStatus {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Retired => "retired",
            Self::Reserved => "reserved",
        }
    }
}

/// D-REPORT-HOME1=A: one typed report row. The row owns the protocol fields
/// and the user-facing templates; call sites only supply filled values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticRow {
    pub code: &'static str,
    pub stage: &'static str,
    pub severity: Severity,
    pub moment: ReportMoment,
    pub status: DiagnosticStatus,
    pub meaning: &'static str,
    pub what: &'static str,
    pub why: &'static str,
    pub fix: &'static str,
    pub template_holes: &'static [&'static str],
    pub detail: bool,
    pub structured_fix: Option<StructuredFix>,
}

/// Machine-applicable behavior declared by a diagnostic row.
///
/// The source spelling stays a small, inspectable marker in
/// `Diagnostics.jet`; this typed projection is what emitters consume. Human
/// Fix prose is never parsed to recover one of these facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredFix {
    CryptoMisuse,
    Replace {
        from: &'static str,
        to: &'static str,
    },
    Remove {
        text: &'static str,
    },
    GeneratedMarkerGroup,
    GeneratedMissingArms,
    GeneratedScriptRun,
}

impl StructuredFix {
    pub fn source_marker(self) -> String {
        match self {
            Self::CryptoMisuse => "crypto_misuse".to_string(),
            Self::Replace { from, to } => format!("replace:{from}=>{to}"),
            Self::Remove { text } => format!("remove:{text}"),
            Self::GeneratedMarkerGroup => "generated:marker_group".to_string(),
            Self::GeneratedMissingArms => "generated:missing_arms".to_string(),
            Self::GeneratedScriptRun => "generated:script_run".to_string(),
        }
    }
}

/// The three user-facing messages after a row's named holes are filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDiagnostic {
    pub what: String,
    pub why: String,
    pub fix: String,
}

impl DiagnosticRow {
    /// Fill this row's What/Why/Fix templates from named values.
    ///
    /// A value must name a hole declared by the row, and every declared hole
    /// must be supplied. This keeps emit sites from silently inventing a
    /// second template protocol.
    pub fn render(&self, holes: &[(&str, &str)]) -> RenderedDiagnostic {
        for &(name, _) in holes {
            assert!(
                self.template_holes.iter().any(|hole| *hole == name),
                "diagnostic `{}` has no template hole `{name}`",
                self.code
            );
            assert_eq!(
                holes.iter().filter(|entry| entry.0 == name).count(),
                1,
                "diagnostic `{}` receives template hole `{name}` more than once",
                self.code
            );
        }
        for &name in self.template_holes {
            assert!(
                holes.iter().any(|entry| entry.0 == name),
                "diagnostic `{}` is missing template hole `{name}`",
                self.code
            );
        }
        RenderedDiagnostic {
            what: render_template(self.what, holes),
            why: render_template(self.why, holes),
            fix: render_template(self.fix, holes),
        }
    }
}

/// D-FACT-LAW1=B / D-FACTDECL1=A: the non-code rows are read from Prelude
/// declarations. The source carries the columns that define the law; this
/// type only holds their parsed form for the one registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FactDeclaration {
    /// The identifier written after `fact`. A compiler namespace may publish a
    /// dotted registry name through the `@name` column while keeping the
    /// declaration ordinary Jet source.
    pub source_name: &'static str,
    pub name: &'static str,
    pub target: RowTarget,
    pub safe_direction: SafeDirection,
    pub gates: &'static [&'static str],
    pub published_by: Option<&'static str>,
    pub decision: &'static str,
    /// D-TYPE2-FOUND1=A: identity-bearing plane metadata.
    pub identity_bearing: bool,
}

/// D-FACT-READ1=A: the one member-position reader. The spelling lives in
/// `Syntax`; this enum is the registry's semantic projection, so parser, sema,
/// comptime, and codegen do not grow separate fact-name tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactRead {
    Layout,
    Name,
    Fields,
    Range,
    Dimension,
    States,
    Effects,
    BuildPackageName,
    BuildPackageVersion,
    BuildOS,
    BuildProfile,
    BuildStampGit,
    BuildStampDirty,
    BuildStampToolchain,
    BuildStampAt,
}

/// Resolve a marked member through the registered fact planes.
pub fn fact_read(member: &str) -> Option<FactRead> {
    let (row_name, read) = match member {
        crate::Syntax::COMPILER_FACT_LAYOUT => ("Type.Layout", FactRead::Layout),
        crate::Syntax::COMPILER_FACT_NAME => ("Type.Nominal", FactRead::Name),
        crate::Syntax::COMPILER_FACT_FIELDS => ("Type.Layout", FactRead::Fields),
        crate::Syntax::COMPILER_FACT_RANGE => ("Type.Interval", FactRead::Range),
        crate::Syntax::COMPILER_FACT_DIMENSION => ("Type.Dimension", FactRead::Dimension),
        crate::Syntax::COMPILER_FACT_STATES => ("States", FactRead::States),
        crate::Syntax::COMPILER_FACT_EFFECTS => ("Effects", FactRead::Effects),
        crate::Syntax::COMPILER_BUILD_FACT_PROFILE => ("Build.Profile", FactRead::BuildProfile),
        _ => return None,
    };

    if row(row_name).is_some_and(|registered| registered.kind() == RowKind::Fact) {
        Some(read)
    } else {
        None
    }
}

/// D-CONF-READ1=A / D-CONF-STAMP1=B: resolve one complete `@build.*` path
/// through the same registered fact table used by every other plane.
pub fn build_fact_read(path: &str) -> Option<FactRead> {
    let (row_name, read) = match path {
        crate::Syntax::COMPILER_BUILD_FACT_PACKAGE_NAME => {
            ("Build.Package.Name", FactRead::BuildPackageName)
        }
        crate::Syntax::COMPILER_BUILD_FACT_PACKAGE_VERSION => {
            ("Build.Package.Version", FactRead::BuildPackageVersion)
        }
        crate::Syntax::COMPILER_BUILD_FACT_OS => ("Build.OS", FactRead::BuildOS),
        crate::Syntax::COMPILER_BUILD_FACT_PROFILE_PATH => {
            ("Build.Profile", FactRead::BuildProfile)
        }
        crate::Syntax::COMPILER_BUILD_FACT_STAMP_GIT => {
            ("Build.Stamp.Git", FactRead::BuildStampGit)
        }
        crate::Syntax::COMPILER_BUILD_FACT_STAMP_DIRTY => {
            ("Build.Stamp.Dirty", FactRead::BuildStampDirty)
        }
        crate::Syntax::COMPILER_BUILD_FACT_STAMP_TOOLCHAIN => {
            ("Build.Stamp.Toolchain", FactRead::BuildStampToolchain)
        }
        crate::Syntax::COMPILER_BUILD_FACT_STAMP_AT => {
            ("Build.Stamp.At", FactRead::BuildStampAt)
        }
        _ => return None,
    };
    row(row_name).is_some_and(|registered| registered.kind() == RowKind::Fact)
        .then_some(read)
}

/// The one authority for the non-code registration rows.
pub const FACT_SOURCE: &str = include_str!("../../jet-codegen/src/Prelude/Facts.jet");

/// D-REPORT-HOME1=A: the compile-time row source. Markdown and terminal
/// renderers are projections of this table, never another authority.
pub const DIAGNOSTIC_SOURCE: &str =
    include_str!("../../jet-codegen/src/Prelude/Diagnostics.jet");

static FACT_DECLARATIONS: LazyLock<Vec<FactDeclaration>> = LazyLock::new(read_fact_declarations);
static DIAGNOSTIC_ROWS: LazyLock<Vec<DiagnosticRow>> = LazyLock::new(read_diagnostic_rows);

/// Read every `fact` declaration in `FACT_SOURCE`.
pub fn fact_declarations() -> &'static [FactDeclaration] {
    &FACT_DECLARATIONS
}

/// Every typed diagnostic row, in source order.
pub fn diagnostic_rows() -> &'static [DiagnosticRow] {
    &DIAGNOSTIC_ROWS
}

/// One typed-row lookup used by every diagnostic constructor and renderer.
pub fn diagnostic(code: &str) -> Option<&'static DiagnosticRow> {
    diagnostic_rows().iter().find(|row| row.code == code)
}

/// Diagnostic rows as rows of the shared registration table.
pub fn diagnostic_registry_rows() -> impl Iterator<Item = &'static RegistryRow> {
    rows().iter().filter(|row| row.kind() == RowKind::Diagnostic)
}

fn read_diagnostic_rows() -> Vec<DiagnosticRow> {
    DIAGNOSTIC_SOURCE
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .map(diagnostic_row_from_source)
        .collect()
}

fn diagnostic_row_from_source(line: &str) -> DiagnosticRow {
    let fields: Vec<&str> = line.split('\t').collect();
    assert_eq!(
        fields.len(),
        12,
        "diagnostic row needs 12 tab-separated fields: {line}"
    );
    assert_eq!(fields[0], "diagnostic", "unknown diagnostic row kind: {line}");
    let code = leak(&unescape_source(fields[1]));
    let stage = leak(&unescape_source(fields[2]));
    let severity = match fields[3] {
        "error" => Severity::Error,
        "lint" => Severity::Lint,
        other => panic!("unknown diagnostic severity `{other}` in {line}"),
    };
    let moment = match fields[4] {
        "compile" => ReportMoment::Compile,
        "run" => ReportMoment::Run,
        "test" => ReportMoment::Test,
        "tool" => ReportMoment::Tool,
        other => panic!("unknown diagnostic moment `{other}` in {line}"),
    };
    let status = match fields[5] {
        "active" => DiagnosticStatus::Active,
        "retired" => DiagnosticStatus::Retired,
        "reserved" => DiagnosticStatus::Reserved,
        other => panic!("unknown diagnostic status `{other}` in {line}"),
    };
    let meaning = leak(&unescape_source(fields[6]));
    let what = leak(&unescape_source(fields[7]));
    let why = leak(&unescape_source(fields[8]));
    let fix = leak(&unescape_source(fields[9]));
    let detail = match fields[10] {
        "true" => true,
        "false" => false,
        other => panic!("diagnostic detail flag `{other}` is not bool in {line}"),
    };
    let structured_fix = match fields[11] {
        "-" => None,
        value => {
            let marker = leak(&unescape_source(value));
            Some(structured_fix_from_source(marker, line))
        }
    };
    DiagnosticRow {
        code,
        stage,
        severity,
        moment,
        status,
        meaning,
        what,
        why,
        fix,
        template_holes: template_holes(&[what, why, fix]),
        detail,
        structured_fix,
    }
}

fn structured_fix_from_source(value: &'static str, line: &str) -> StructuredFix {
    if value == "crypto_misuse" {
        return StructuredFix::CryptoMisuse;
    }
    if let Some(text) = value.strip_prefix("remove:") {
        assert!(!text.is_empty(), "empty structured remove fix in {line}");
        return StructuredFix::Remove { text };
    }
    if let Some(replacement) = value.strip_prefix("replace:") {
        let (from, to) = replacement
            .split_once("=>")
            .unwrap_or_else(|| panic!("structured replace fix needs `=>` in {line}"));
        assert!(
            !from.is_empty() && !to.is_empty(),
            "structured replace fix needs non-empty sides in {line}"
        );
        return StructuredFix::Replace {
            from: leak(from),
            to: leak(to),
        };
    }
    if let Some(kind) = value.strip_prefix("generated:") {
        return match kind {
            "marker_group" => StructuredFix::GeneratedMarkerGroup,
            "missing_arms" => StructuredFix::GeneratedMissingArms,
            "script_run" => StructuredFix::GeneratedScriptRun,
            _ => panic!("unknown generated structured fix `{kind}` in {line}"),
        };
    }
    panic!("unknown structured diagnostic fix `{value}` in {line}");
}

fn render_template(template: &str, holes: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' && bytes.get(index + 1) == Some(&b'{') {
            out.push('{');
            index += 2;
            continue;
        }
        if bytes[index] == b'}' && bytes.get(index + 1) == Some(&b'}') {
            out.push('}');
            index += 2;
            continue;
        }
        if bytes[index] == b'{' {
            if let Some(close_offset) = template[index + 1..].find('}') {
                let close = index + 1 + close_offset;
                let name = &template[index + 1..close];
                if !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    let value = holes
                        .iter()
                        .find(|entry| entry.0 == name)
                        .map(|entry| entry.1)
                        .unwrap_or_else(|| panic!("missing diagnostic template hole `{name}`"));
                    out.push_str(value);
                    index = close + 1;
                    continue;
                }
            }
        }
        let character = template[index..]
            .chars()
            .next()
            .expect("template index is on a character boundary");
        out.push(character);
        index += character.len_utf8();
    }
    out
}

/// A row field whose text itself contains backticks is written as a markdown
/// code span (`` … ``) in Diagnostics.jet. The fence is table notation, not
/// product copy — strip it before the template is stored.
fn strip_code_span_fence(value: &str) -> &str {
    value
        .strip_prefix("`` ")
        .and_then(|rest| rest.strip_suffix(" ``"))
        .unwrap_or(value)
}

fn unescape_source(value: &str) -> String {
    let value = strip_code_span_fence(value);
    let mut out = String::with_capacity(value.len());
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            out.push(match character {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            out.push(character);
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

fn template_holes(templates: &[&str]) -> &'static [&'static str] {
    let mut holes = Vec::new();
    for template in templates {
        let bytes = template.as_bytes();
        let mut start = 0;
        while start < bytes.len() {
            let Some(open) = template[start..].find('{') else {
                break;
            };
            let open = start + open;
            let Some(close) = template[open + 1..].find('}') else {
                break;
            };
            let close = open + 1 + close;
            let hole = &template[open + 1..close];
            if !hole.is_empty()
                && hole
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                && !holes.contains(&hole)
            {
                holes.push(leak(hole));
            }
            start = close + 1;
        }
    }
    leak_slice(holes)
}

fn read_fact_declarations() -> Vec<FactDeclaration> {
    FACT_SOURCE
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("fact "))
        .map(fact_declaration)
        .collect()
}

fn fact_declaration(line: &str) -> FactDeclaration {
    let rest = line["fact ".len()..].trim();
    let open = rest
        .find('(')
        .unwrap_or_else(|| panic!("fact declaration without a parameter list: {line}"));
    let close = rest
        .rfind(')')
        .unwrap_or_else(|| panic!("fact declaration without a closing `)`: {line}"));
    let source_name = leak(rest[..open].trim());
    let mut name = source_name;
    let mut target = None;
    let mut safe_direction = None;
    let mut gates = None;
    let mut published_by = None;
    let mut decision = None;
    let mut identity_bearing = false;

    for entry in split_top_level(&rest[open + 1..close]) {
        let (label, value) = entry
            .split_once(':')
            .unwrap_or_else(|| panic!("fact parameter without `:` in {line}: {entry}"));
        let (label, value) = (label.trim(), value.trim());
        match label {
            "@holds" => target = Some(fact_target(value, line)),
            "@safe" => safe_direction = Some(fact_direction(value, line)),
            "@gates" => gates = Some(fact_gates(value, line)),
            "@proved_by" => published_by = Some(leak(value)),
            "@name" => name = leak(&unquote(value, line)),
            "@identity" => identity_bearing = fact_bool(value, line),
            "@decision" => decision = Some(leak(&unquote(value, line))),
            other => panic!("unknown fact column `{other}` in {line}"),
        }
    }

    FactDeclaration {
        source_name,
        name,
        target: target.unwrap_or_else(|| panic!("fact declaration without `@holds`: {line}")),
        safe_direction: safe_direction
            .unwrap_or_else(|| panic!("fact declaration without `@safe`: {line}")),
        gates: gates.unwrap_or_else(|| panic!("fact declaration without `@gates`: {line}")),
        published_by,
        decision: decision.unwrap_or_else(|| panic!("fact declaration without `@decision`: {line}")),
        identity_bearing,
    }
}

fn fact_target(value: &str, line: &str) -> RowTarget {
    match value.strip_prefix('.').unwrap_or(value) {
        "Value" => RowTarget::Value,
        "Scope" => RowTarget::Scope,
        "Build" => RowTarget::Build,
        other => panic!("`{other}` is not a fact target in {line}"),
    }
}

fn fact_direction(value: &str, line: &str) -> SafeDirection {
    match value.strip_prefix('.').unwrap_or(value) {
        "Gain" => SafeDirection::Gain,
        "Shrink" => SafeDirection::Shrink,
        "Discharge" => SafeDirection::Discharge,
        "None" => SafeDirection::None,
        other => panic!("`{other}` is not a safe direction in {line}"),
    }
}

fn fact_bool(value: &str, line: &str) -> bool {
    match value {
        "true" => true,
        "false" => false,
        other => panic!("`{other}` is not a fact boolean in {line}"),
    }
}

fn fact_gates(value: &str, line: &str) -> &'static [&'static str] {
    let inner = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or_else(|| panic!("fact gates must be a list in {line}"));
    leak_slice(
        split_top_level(inner)
            .into_iter()
            .filter(|gate| !gate.is_empty())
            .map(|gate| leak(gate.strip_prefix('.').unwrap_or(gate)))
            .collect(),
    )
}

/// Comma-separated fact columns at nesting depth zero, outside strings.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in text.as_bytes().iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'[' | b'(' | b'{' => depth += 1,
            b']' | b')' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                entries.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        entries.push(last);
    }
    entries
}

fn unquote(value: &str, line: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("fact decision must be quoted in {line}"));
    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for character in inner.chars() {
        match (escaped, character) {
            (true, other) => {
                out.push(other);
                escaped = false;
            }
            (false, '\\') => escaped = true,
            (false, other) => out.push(other),
        }
    }
    out
}

// ponytail: fact rows are read once and live for the process, so source strings
// are leaked instead of threading lifetimes through the registry.
fn leak(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}

fn leak_slice<T>(items: Vec<T>) -> &'static [T] {
    Box::leak(items.into_boxed_slice())
}

fn fact_row(declaration: &FactDeclaration) -> RegistryRow {
    RegistryRow {
        name: declaration.name,
        target: declaration.target,
        identity_bearing: declaration.identity_bearing,
        safe_direction: declaration.safe_direction,
        gates: declaration.gates,
        published_by: declaration.published_by,
        rule: None,
        home: None,
        renderers: &[],
        guard: None,
        diagnostic: None,
        decision: declaration.decision,
    }
}

/// D-ONCE-LAW1=A: one row of a truth the corpus states once. The three columns
/// a marker or a plane leaves empty are the whole difference, so this writes
/// the rest of the shape down once instead of at every row.
const fn truth_row(
    name: &'static str,
    home: &'static str,
    renderers: &'static [&'static str],
    guard: Guard,
    decision: &'static str,
) -> RegistryRow {
    RegistryRow {
        name,
        target: RowTarget::Corpus,
        identity_bearing: false,
        safe_direction: SafeDirection::None,
        gates: &[],
        published_by: None,
        rule: None,
        home: Some(home),
        renderers,
        guard: Some(guard),
        diagnostic: None,
        decision,
    }
}

/// D-META-REG1=A: a marker row states no direction. A rule on written code says
/// what a writer may attach and where; it holds no fact that moves toward or
/// away from safety, so it states `none` and names no gate. The moving facts a
/// marker *writes* belong to the plane, right, or build row that holds them —
/// `#Caps` and `#Grant` are gate words on the `Rights` row, not directions of
/// their own. Stated once here for every marker row, so no row can drift.
fn marker_row(rule: &'static AppliedRule) -> RegistryRow {
    RegistryRow {
        name: rule.name,
        target: RowTarget::Code(rule.sites),
        identity_bearing: false,
        safe_direction: SafeDirection::None,
        gates: &[],
        published_by: None,
        rule: Some(rule),
        home: None,
        renderers: &[],
        guard: None,
        diagnostic: None,
        decision: "D-VERDICT-1455-1",
    }
}

fn diagnostic_registry_row(row: &'static DiagnosticRow) -> RegistryRow {
    RegistryRow {
        name: row.code,
        target: RowTarget::Report,
        identity_bearing: false,
        safe_direction: SafeDirection::None,
        gates: &[],
        published_by: None,
        rule: None,
        home: None,
        renderers: &[],
        guard: None,
        diagnostic: Some(row),
        decision: "D-REPORT-HOME1",
    }
}

/// D-ONCE-LAW1=A: the truths the corpus states once.
///
/// Each row is a place where one meaning used to be written down two or more
/// times and could answer differently in each copy. The row names the file
/// that now owns the meaning, everything that renders it from there, and the
/// test that proves a second copy cannot come back.
const TRUTH_ROWS: &[RegistryRow] = &[
    truth_row(
        "RegistrationTable",
        "crates/jet-foundation/src/Registry.rs",
        &["jet explain", "jet inspect facts", "compile-time reflection"],
        Guard {
            test: "every_row_states_the_one_way_law",
            file: "tests/marker_registry_coverage.rs",
            proof: GuardProof::CountsSites,
        },
        "D-META-REG1",
    ),
    truth_row(
        "JitHostSymbols",
        "crates/jet-jit/src/jit/runtime_host.rs",
        &["host_fns! declarations", "JIT lowering call_host"],
        Guard {
            test: "all_host_symbols_declared_match_all_registered",
            file: "crates/jet-jit/src/jit/runtime_host.rs",
            proof: GuardProof::CountsSites,
        },
        "D-ONCE-LAW1",
    ),
    truth_row(
        "CoreCalls",
        "crates/jet-foundation/src/Syntax/core_calls.rs",
        &[
            "sema/TIR/AOT/comptime projections of every plain core.* call",
            "effect and sink facts for every plain core.* call",
        ],
        Guard {
            test: "core_projection_is_complete_both_directions",
            file: "tests/core_call_table.rs",
            proof: GuardProof::CountsSites,
        },
        "D-ONCE-LAW1",
    ),
    truth_row(
        "BigIntSnip",
        "crates/jet-foundation/src/Numeric.rs",
        &["compile-time evaluation", "JIT encoding", "the Prelude"],
        Guard {
            test: "comptime_bigint_matches_runtime",
            file: "tests/comptime_diff.rs",
            proof: GuardProof::DiffsBehavior,
        },
        "D-ONCE-LAW1",
    ),
    truth_row(
        "Scheduler",
        "crates/jet-codegen/src/Prelude/Scheduler.rs",
        &["AOT programs", "the JIT scheduler host"],
        Guard {
            test: "the_jit_scheduler_is_the_prelude_scheduler",
            file: "crates/jet-codegen/src/SchedulerHost.rs",
            proof: GuardProof::CountsSites,
        },
        "D-ONCE-LAW1",
    ),
    truth_row(
        "IceReport",
        "crates/jet-foundation/src/Diagnostics.rs",
        &["the jet binary panic hook", "the ice! macro", "compile driver reports"],
        Guard {
            test: "no_hand_typed_ice_banner_outside_the_one_home",
            file: "tests/ice_report_single_home.rs",
            proof: GuardProof::CountsSites,
        },
        "D-ONCE-LAW1",
    ),
    truth_row(
        "Retirements",
        "crates/jet-foundation/src/Syntax/retirements.rs",
        &["jet fmt", "jet fix", "the retirement diagnostics"],
        Guard {
            test: "adoption_ratchets_toward_zero",
            file: "tests/retirement_ratchet.rs",
            proof: GuardProof::CountsSites,
        },
        "D-ONCE-RETIRE1",
    ),
    truth_row(
        "JetVocabulary",
        "docs/spec/vocabulary.md",
        &["Markdown docs"],
        Guard {
            test: "vocabulary_page_has_one_definition_and_no_retired_senses",
            file: "tests/truthfulness.rs",
            proof: GuardProof::CountsSites,
        },
        "D-ONCE-WORD1=A",
    ),
];

/// The one table. Marker rows come from the marker registry, fact rows from
/// Prelude declarations, and truths from their one source; nothing else may
/// hold a row.
static REGISTRY: LazyLock<Vec<RegistryRow>> = LazyLock::new(|| {
    APPLIED_RULES
        .iter()
        .map(marker_row)
        .chain(FACT_DECLARATIONS.iter().map(fact_row))
        .chain(TRUTH_ROWS.iter().copied())
        .chain(DIAGNOSTIC_ROWS.iter().map(diagnostic_registry_row))
        .collect()
});

/// Every registered truth, in registration order.
pub fn truths() -> impl Iterator<Item = &'static RegistryRow> {
    rows().iter().filter(|row| row.kind() == RowKind::Truth)
}

/// Every registered row, of every kind.
pub fn rows() -> &'static [RegistryRow] {
    &REGISTRY
}

/// Marker rows from the one registration table. Consumers that need marker
/// vocabulary or reachability must read this projection, not a second list.
pub fn marker_rows() -> impl Iterator<Item = &'static RegistryRow> {
    rows().iter().filter(|row| row.kind() == RowKind::Marker)
}

/// One lookup for every kind. Row names are unique across the table
/// (`law_violations` proves it), so a name is enough.
pub fn row(name: &str) -> Option<&'static RegistryRow> {
    rows().iter().find(|row| row.name == name)
}

/// Resolve a type-plane source name through the Prelude fact declarations.
/// The Rust compiler does not keep a second plane vocabulary: the readable
/// `Facts.jet` rows are the only names that can succeed here.
pub fn type_plane(source_name: &str) -> &'static str {
    FACT_DECLARATIONS
        .iter()
        .find(|declaration| {
            declaration.source_name == source_name
                || declaration.name == source_name
                || declaration
                    .source_name
                    .strip_prefix("Type")
                    .is_some_and(|name| name == source_name)
        })
        .filter(|declaration| declaration.name.starts_with("Type."))
        .map(|declaration| declaration.name)
        .unwrap_or_else(|| panic!("unregistered type plane `{source_name}`"))
}

/// Every Prelude fact row, including type planes and compiler-owned facts.
pub fn fact_rows() -> impl Iterator<Item = &'static RegistryRow> {
    fact_declarations()
        .iter()
        .filter_map(|declaration| row(declaration.name))
}

/// Type-plane rows come from the readable Prelude declarations.
pub fn type_plane_rows() -> impl Iterator<Item = &'static RegistryRow> {
    fact_rows().filter(|row| row.name.starts_with("Type."))
}

/// The closed `FactKind` view is a projection of the registered plane rows.
/// `Type.Interval` is the one source name whose reflection kind is `Range`;
/// every other plane uses its registered leaf name. Adding a plane therefore
/// adds its typed reflection kind through the same source row.
pub fn reflection_kind(name: &str) -> Option<&'static str> {
    let row = row(name)?;
    if row.kind() != RowKind::Plane {
        return None;
    }
    if row.name == "Type.Interval" {
        return Some("Range");
    }
    row.name
        .strip_prefix("Type.")
        .or(Some(row.name))
}

/// The drift guard for the two law columns (D-FACT-LAW1=B), and for the one
/// name space the table keeps. One implementation: the law-zero coverage guard
/// calls this, and no kind gets a second one.
///
/// A row cannot state neither column — both are fields, so the build fails at
/// the row itself. What this reads is everything a compiler cannot: a gate named
/// with no direction to loosen, a prover row that claims plane algebra, a gate
/// word nothing spells, and a name registered twice.
pub fn law_violations() -> Vec<String> {
    let mut violations = check(rows());
    for declaration in fact_declarations() {
        let name = declaration.name;
        let matches: Vec<_> = rows().iter().filter(|row| row.name == name).collect();
        if matches.len() != 1 {
            violations.push(format!(
                "fact `{name}` has {} registry rows; one fact needs one row",
                matches.len()
            ));
            continue;
        }
        let row = matches[0];
        if row.target != declaration.target {
            violations.push(format!(
                "fact `{name}` target drifted from `{}` to `{}`",
                declaration.target.kind().name(),
                row.kind().name(),
            ));
        }
        if row.identity_bearing != declaration.identity_bearing {
            violations.push(format!(
                "type plane `{name}` identity policy drifted from the one table"
            ));
        }
    }
    violations
}

fn check(rows: &[RegistryRow]) -> Vec<String> {
    let mut violations = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for row in rows {
        if seen.contains(&row.name) {
            violations.push(format!(
                "`{}` is registered twice; one table means one row per name",
                row.name
            ));
        } else {
            seen.push(row.name);
        }
        if row.safe_direction == SafeDirection::None && !row.gates.is_empty() {
            violations.push(format!(
                "`{}` ({}) names a gate word but states no safe direction; \
                 a gate loosens a direction, so say which way tightens",
                row.name,
                row.kind().name()
            ));
        }
        for gate in row.gates {
            let spelled = fact_declarations()
                .iter()
                .any(|declaration| declaration.gates.iter().any(|candidate| candidate == gate))
                || rows
                    .iter()
                    .any(|candidate| candidate.name == *gate && candidate.rule.is_some());
            if !spelled {
                violations.push(format!(
                    "`{}` names the gate word `{gate}`, which nothing spells; \
                     a gate is a registered marker or a Prelude gate",
                    row.name
                ));
            }
        }
        if row.is_prover_supplied() && row.safe_direction != SafeDirection::None {
            violations.push(format!(
                "`{}` is published by a prover, so it carries no plane algebra \
                 and must state safe direction `none` (D-FACT-OWN1=A)",
                row.name
            ));
        }
        if matches!(row.target, RowTarget::Code(_)) != row.rule.is_some() {
            violations.push(format!(
                "`{}` attaches to written code exactly when it carries a marker signature",
                row.name
            ));
        }
        if (row.kind() == RowKind::Diagnostic) != row.diagnostic.is_some() {
            violations.push(format!(
                "`{}` is a diagnostic row exactly when it carries a typed diagnostic payload",
                row.name
            ));
        }
        violations.extend(truth_violations(row));
    }
    violations
}

/// D-ONCE-LAW1=A: the three columns a corpus truth must fill and no other kind
/// may fill. A registered truth with no guard is the case the law exists for,
/// so it is named first and in the law's own words.
fn truth_violations(row: &RegistryRow) -> Vec<String> {
    let mut violations = Vec::new();
    if row.kind() == RowKind::Truth {
        if row.guard.is_none() {
            violations.push(format!(
                "`{}` is a registered truth with no guard; a truth that nothing \
                 proves has one home only until someone writes the second copy",
                row.name
            ));
        }
        if row.home.is_none() {
            violations.push(format!(
                "`{}` is a registered truth that names no home; say which file owns it",
                row.name
            ));
        }
        if row.renderers.is_empty() {
            violations.push(format!(
                "`{}` is a registered truth that names no renderer; a truth \
                 nothing reads is dead code, not a truth",
                row.name
            ));
        }
        if row.safe_direction != SafeDirection::None {
            violations.push(format!(
                "`{}` is a corpus truth, so it holds no fact that moves and states \
                 safe direction `none`",
                row.name
            ));
        }
    } else if row.home.is_some() || !row.renderers.is_empty() || row.guard.is_some() {
        violations.push(format!(
            "`{}` ({}) fills a home, renderer, or guard column; those belong to a \
             corpus truth (D-ONCE-LAW1=A)",
            row.name,
            row.kind().name()
        ));
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::{
        diagnostic, diagnostic_registry_rows, diagnostic_rows, fact_declarations, law_violations,
        row, rows, type_plane, type_plane_rows,
        RowKind, RowTarget, SafeDirection,
        StructuredFix,
    };

    #[test]
    fn the_one_table_holds_all_five_kinds() {
        for kind in [
            RowKind::Marker,
            RowKind::Plane,
            RowKind::Right,
            RowKind::Fact,
            RowKind::Truth,
            RowKind::Diagnostic,
        ] {
            assert!(
                rows().iter().any(|row| row.kind() == kind),
                "no {} row in the one table",
                kind.name()
            );
        }
    }

    #[test]
    fn a_target_names_its_kind() {
        assert_eq!(RowTarget::Code(&[]).kind(), RowKind::Marker);
        assert_eq!(RowTarget::Value.kind(), RowKind::Plane);
        assert_eq!(RowTarget::Scope.kind(), RowKind::Right);
        assert_eq!(RowTarget::Build.kind(), RowKind::Fact);
        assert_eq!(RowTarget::Corpus.kind(), RowKind::Truth);
        assert_eq!(RowTarget::Report.kind(), RowKind::Diagnostic);
    }

    #[test]
    fn type_plane_rows_declare_identity_policy() {
        assert!(
            row(type_plane("Interval"))
                .expect("interval plane is registered")
                .is_identity_bearing()
        );
        assert!(
            !row(type_plane("Obligation"))
                .expect("obligation plane is registered")
                .is_identity_bearing()
        );
    }

    #[test]
    fn every_type_plane_has_one_row_from_the_shared_list() {
        let planes: Vec<_> = type_plane_rows().collect();
        assert_eq!(planes.len(), 8);
        for row in planes {
            let declaration = fact_declarations()
                .iter()
                .find(|declaration| declaration.name == row.name)
                .expect("type plane has a Prelude declaration");
            assert_eq!(row.kind(), RowKind::Plane);
            assert_eq!(row.identity_bearing, declaration.identity_bearing);
            assert_eq!(row.decision, declaration.decision);
        }
    }

    #[test]
    fn every_row_obeys_the_one_way_law() {
        assert_eq!(law_violations(), Vec::<String>::new());
    }

    /// The guard has to catch a bad row, not only pass a good table.
    #[test]
    fn the_guard_names_a_row_that_breaks_the_law() {
        use super::{check, RegistryRow};

        let gate_with_no_direction = RegistryRow {
            name: "Wrong",
            target: RowTarget::Value,
            identity_bearing: false,
            safe_direction: SafeDirection::None,
            gates: &["approx"],
            published_by: None,
            rule: None,
            home: None,
            renderers: &[],
            guard: None,
            diagnostic: None,
            decision: "D-TEST",
        };
        assert_eq!(check(&[gate_with_no_direction]).len(), 1);

        let unspelled_gate = RegistryRow {
            gates: &["Trust"],
            safe_direction: SafeDirection::Gain,
            ..gate_with_no_direction
        };
        assert!(check(&[unspelled_gate])[0].contains("nothing spells"));

        let prover_with_algebra = RegistryRow {
            gates: &[],
            safe_direction: SafeDirection::Gain,
            published_by: Some("ownership"),
            ..gate_with_no_direction
        };
        assert!(check(&[prover_with_algebra])[0].contains("no plane algebra"));

        assert_eq!(check(&[gate_with_no_direction, gate_with_no_direction]).len(), 3);
    }

    /// D-ONCE-LAW1=A: the case the law exists for. A truth row that registers
    /// without a guard must fail the same guard every other row answers to.
    #[test]
    fn a_registered_truth_with_no_guard_fails_the_lint() {
        use super::{check, Guard, GuardProof, RegistryRow};

        let guarded = RegistryRow {
            name: "Wrong",
            target: RowTarget::Corpus,
            identity_bearing: false,
            safe_direction: SafeDirection::None,
            gates: &[],
            published_by: None,
            rule: None,
            home: Some("crates/jet-foundation/src/Registry.rs"),
            renderers: &["jet explain"],
            guard: Some(Guard {
                test: "every_row_states_the_one_way_law",
                file: "tests/marker_registry_coverage.rs",
                proof: GuardProof::CountsSites,
            }),
            diagnostic: None,
            decision: "D-TEST",
        };
        assert_eq!(check(&[guarded]), Vec::<String>::new());

        let unguarded = RegistryRow { guard: None, ..guarded };
        assert!(check(&[unguarded])[0].contains("no guard"));

        let homeless = RegistryRow { home: None, ..guarded };
        assert!(check(&[homeless])[0].contains("names no home"));

        let unread = RegistryRow { renderers: &[], ..guarded };
        assert!(check(&[unread])[0].contains("names no renderer"));

        // The three columns belong to a truth row and to nothing else.
        let plane_with_a_home = RegistryRow {
            target: RowTarget::Value,
            ..guarded
        };
        assert!(check(&[plane_with_a_home])[0].contains("corpus truth"));
    }

    /// D-ONCE-LAW1=A: every shipped truth row is complete, and the say-once
    /// closures are registered so the table is not born empty.
    #[test]
    fn every_shipped_truth_is_complete() {
        use super::truths;

        let names: Vec<&str> = truths().map(|row| row.name).collect();
        assert!(names.len() >= 7, "the registry is born non-empty: {names:?}");
        for row in truths() {
            assert!(row.home.is_some(), "`{}` names no home", row.name);
            assert!(!row.renderers.is_empty(), "`{}` names no renderer", row.name);
            assert!(row.guard.is_some(), "`{}` names no guard", row.name);
        }
    }

    #[test]
    fn a_prover_row_is_read_only() {
        let sendability = row("Sendability").expect("the prover publishes Sendability");
        assert!(sendability.is_prover_supplied());
        assert_eq!(sendability.safe_direction, SafeDirection::None);
        assert!(sendability.gates.is_empty());
    }

    #[test]
    fn diagnostic_rows_are_typed_and_have_one_source() {
        assert!(diagnostic_rows().len() >= 700);
        assert_eq!(diagnostic("E0102").expect("E0102 row").severity, crate::Diagnostics::Severity::Error);
        assert_eq!(diagnostic("L2001").expect("L2001 row").severity, crate::Diagnostics::Severity::Lint);
        for row in diagnostic_rows() {
            assert!(!row.code.is_empty());
            assert!(!row.what.is_empty());
            assert!(!row.why.is_empty());
            assert!(!row.fix.is_empty());
            assert!(diagnostic(row.code).is_some());
        }
        assert_eq!(
            diagnostic_registry_rows().count(),
            diagnostic_rows().len(),
            "every typed row must be in the shared registration table"
        );
    }

    #[test]
    fn row_templates_and_structured_fixes_are_typed() {
        let layout = diagnostic("E0959").expect("E0959 row");
        let rendered = layout.render(&[("type", "Point"), ("member", "size")]);
        assert!(rendered.what.contains("Point.size"));

        let crypto = diagnostic("E2702").expect("E2702 row");
        assert_eq!(
            crypto.structured_fix,
            Some(StructuredFix::CryptoMisuse)
        );
        assert_eq!(
            diagnostic("E0999").and_then(|row| row.structured_fix),
            Some(StructuredFix::GeneratedMarkerGroup)
        );
        let rendered = crypto.render(&[("why", "the bound is 32 bytes"), ("fix", "pass 32 bytes")]);
        assert_eq!(rendered.why, "the bound is 32 bytes");
        assert_eq!(rendered.fix, "pass 32 bytes");

        for row in diagnostic_rows() {
            if let Some(fix) = row.structured_fix {
                assert!(!fix.source_marker().is_empty(), "{} has an empty typed fix", row.code);
            }
        }
    }
}
