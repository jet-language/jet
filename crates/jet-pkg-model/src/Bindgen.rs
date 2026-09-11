//! Deterministic helpers for generated binding responses and foreign-adaptation
//! plans.
//!
//! Low-level binders parse foreign declarations. This module owns the
//! data-driven decision that selects native arity or a checked adaptation.
//! A name that merely looks like a count is never enough evidence to remove an
//! argument or claim a borrowed view.

use crate::ForeignBridge::{ForeignBoundaryContract, IdentityBuilder, FOREIGN_BOUNDARY_SCHEMA};
use std::fmt;

#[cfg(test)]
use crate::AST::{
    BinderRuntime, BinderStatus, BindingStubKind, ForeignLanguage, FOREIGN_BINDERS,
};

/// Shared response protocol used by package-model helpers.
///
/// Generated binders use this protocol for machine-readable responses while
/// retaining a human-readable explanation for the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecoderProtocol {
    /// Supervised workers return `{ok, value}` and use the shared status map.
    StandardEnvelope,
    /// The embedded Lua VM returns the encoded function value directly.
    LuaRawJson,
}

/// Emit the one response decoder used by one generated binding.
pub(crate) fn render_decode_response(error: &str, protocol: DecoderProtocol) -> String {
    match protocol {
        DecoderProtocol::StandardEnvelope => format!(
            r#"fn decode_response(raw: String, code: Int) DataTree !{error} -> {{
    if code == {{
        1 -> {{ return Err({error}.NotRunning) }}
        2 -> {{ return Err({error}.Timeout) }}
        3 -> {{ return Err({error}.Cancelled) }}
        5 -> {{ return Err({error}.Limit) }}
        0 -> {{}}
        else -> {{ return Err({error}.Protocol) }}
    }}
    response := json.parse(raw) ?? return Err({error}.Protocol)
    succeeded := (response.field("ok") ?? DataTree.Bool(false)).bool() ?? false
    if !succeeded -> return Err({error}.CommandFailed)
    return Ok(response.field("value") ?? DataTree.Null)
}}

"#,
        ),
        DecoderProtocol::LuaRawJson => format!(
            r#"fn decode_response(raw: String, code: Int) DataTree !{error} -> {{
    decode_status(code)
    value := json.parse(raw) ?? return Err({error}.Protocol)
    return Ok(value)
}}

"#,
        ),
    }
}

/// Emit the Lua status adapter shared by response and table-view calls.
pub(crate) fn render_lua_decode_status(error: &str) -> String {
    format!(
        r#"fn decode_status(code: Int) Bool !{error} -> {{
    if code == 1 -> return Err({error}.NotRunning)
    if code == 2 -> return Err({error}.Timeout)
    if code == 3 -> return Err({error}.Cancelled)
    if code == 5 -> return Err({error}.Limit)
    if code == 4 -> return Err({error}.CommandFailed)
    if code == 6 -> return Err({error}.Protocol)
    if code != 0 -> return Err({error}.Protocol)
    return Ok(true)
}}

"#,
    )
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct RendererDescriptor {
    pub(crate) language: ForeignLanguage,
    pub(crate) runtime: BinderRuntime,
    pub(crate) stub_kind: BindingStubKind,
    pub(crate) protocol: DecoderProtocol,
    pub(crate) render_probe: fn() -> String,
}

/// One entry per active generated response binder. The canonical foreign
/// descriptor table remains the support denominator; this table owns only the
/// renderer probe and its response protocol.
#[cfg(test)]
const GENERATOR_REGISTRY: &[RendererDescriptor] = &[
    RendererDescriptor {
        language: ForeignLanguage::Lua,
        runtime: BinderRuntime::EmbeddedLua,
        stub_kind: BindingStubKind::LuaScript,
        protocol: DecoderProtocol::LuaRawJson,
        render_probe: crate::LuaBind::render_probe,
    },
    RendererDescriptor {
        language: ForeignLanguage::PowerShell,
        runtime: BinderRuntime::SupervisedPowerShell,
        stub_kind: BindingStubKind::PowerShellScript,
        protocol: DecoderProtocol::StandardEnvelope,
        render_probe: crate::PowerShellBind::render_probe,
    },
    RendererDescriptor {
        language: ForeignLanguage::Perl,
        runtime: BinderRuntime::SupervisedPerl,
        stub_kind: BindingStubKind::PerlScript,
        protocol: DecoderProtocol::StandardEnvelope,
        render_probe: crate::PerlBind::render_probe,
    },
    RendererDescriptor {
        language: ForeignLanguage::Ruby,
        runtime: BinderRuntime::SupervisedRuby,
        stub_kind: BindingStubKind::RubyScript,
        protocol: DecoderProtocol::StandardEnvelope,
        render_probe: crate::RubyBind::render_probe,
    },
    RendererDescriptor {
        language: ForeignLanguage::Php,
        runtime: BinderRuntime::SupervisedPhpPool,
        stub_kind: BindingStubKind::PhpScript,
        protocol: DecoderProtocol::StandardEnvelope,
        render_probe: crate::PhpBind::render_probe,
    },
    RendererDescriptor {
        language: ForeignLanguage::R,
        runtime: BinderRuntime::SupervisedR,
        stub_kind: BindingStubKind::RScript,
        protocol: DecoderProtocol::StandardEnvelope,
        render_probe: crate::RBind::render_probe,
    },
    RendererDescriptor {
        language: ForeignLanguage::Octave,
        runtime: BinderRuntime::SupervisedOctave,
        stub_kind: BindingStubKind::OctaveScript,
        protocol: DecoderProtocol::StandardEnvelope,
        render_probe: crate::OctaveBind::render_probe,
    },
];

/// Find the response renderer for one canonical binder descriptor. The
/// descriptor table is the denominator; the generator registry supplies the
/// renderer-owned probe and protocol without a second match list.
#[cfg(test)]
fn renderer_for(descriptor: &crate::AST::BinderDescriptor) -> Option<RendererDescriptor> {
    if descriptor.status != BinderStatus::Active {
        return None;
    }
    GENERATOR_REGISTRY
        .iter()
        .find(|renderer| {
            renderer.language == descriptor.language && renderer.stub_kind == descriptor.stub_kind
        })
        .copied()
}

#[cfg(test)]
fn canonical_for(renderer: &RendererDescriptor) -> Option<&'static crate::AST::BinderDescriptor> {
    let mut matches = FOREIGN_BINDERS.iter().filter(|descriptor| {
        descriptor.status == BinderStatus::Active
            && descriptor.language == renderer.language
            && descriptor.stub_kind == renderer.stub_kind
    });
    let descriptor = matches.next()?;
    matches.next().is_none().then_some(descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_each_message_renderer_once() {
        let registered = GENERATOR_REGISTRY
            .iter()
            .filter_map(canonical_for)
            .collect::<Vec<_>>();
        assert_eq!(
            registered.len(),
            GENERATOR_REGISTRY.len(),
            "every generator registry entry must match exactly one canonical active binder"
        );
        let renderers = GENERATOR_REGISTRY.to_vec();
        assert_eq!(
            FOREIGN_BINDERS
                .iter()
                .filter_map(renderer_for)
                .count(),
            GENERATOR_REGISTRY.len(),
            "canonical active binder registry and generator registry disagree"
        );
        let languages = renderers
            .iter()
            .map(|renderer| renderer.language)
            .collect::<Vec<_>>();
        let mut unique = languages.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), languages.len());

        for renderer in &renderers {
            let descriptor = FOREIGN_BINDERS
                .iter()
                .find(|descriptor| descriptor.language == renderer.language)
                .expect("renderer language is missing from FOREIGN_BINDERS");
            assert_eq!(descriptor.status, BinderStatus::Active);
            assert_eq!(descriptor.runtime, renderer.runtime);
            assert_eq!(descriptor.stub_kind, renderer.stub_kind);
            let source = (renderer.render_probe)();
            assert_eq!(source.matches("fn decode_response(").count(), 1);
            let (tokens, diagnostics) = crate::Lexer::lex_generated(&source);
            assert!(
                diagnostics.is_empty(),
                "{} renderer probe has lexer diagnostics: {diagnostics:#?}",
                renderer.language.root()
            );
            let parse_ok = std::thread::Builder::new()
                .stack_size(8 * 1024 * 1024)
                .spawn(move || crate::Parser::parse(&tokens).is_ok())
                .expect("generated parser thread should start")
                .join()
                .expect("generated parser thread should finish");
            assert!(
                parse_ok,
                "{} renderer probe does not parse:\n{source}",
                renderer.language.root()
            );
            let response_externs = source
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    let rest = line.strip_prefix("fn ")?;
                    let (name, _) = rest.split_once('(')?;
                    (line.contains("input: String")
                        && line.contains("deadline_ms: Int) String ="))
                    .then(|| name.to_string())
                })
                .collect::<Vec<_>>();
            assert!(!response_externs.is_empty());
            for name in response_externs {
                let marker = format!("pub fn {name}");
                let start = source
                    .find(&format!("{marker}("))
                    .or_else(|| source.find(&format!("{marker}<")))
                    .expect("every response extern has a generated wrapper");
                let body = &source[start..];
                let body = body
                    .split_once("\n}\n")
                    .map(|(body, _)| body)
                    .unwrap_or(body);
                assert_eq!(
                    body.matches(&format!("raw :: abi.{name}("))
                        .count(),
                    1,
                    "{} operation `{name}` does not sequence its raw call",
                    renderer.language.root()
                );
                assert_eq!(
                    body.matches("code :: abi.take_error()").count(),
                    1,
                    "{} operation `{name}` does not capture its status after the raw call",
                    renderer.language.root()
                );
                assert_eq!(
                    body.matches("decode_response(raw, code)").count(),
                    1,
                    "{} operation `{name}` bypasses decode_response",
                    renderer.language.root()
                );
                assert!(
                    !body.contains("json.parse(raw)"),
                    "{} operation `{name}` owns envelope parsing",
                    renderer.language.root()
                );
                assert!(
                    !body.contains("response.field(\"ok\")"),
                    "{} operation `{name}` owns envelope status decoding",
                    renderer.language.root()
                );
            }
            if renderer.language == ForeignLanguage::Lua {
                let view_externs = source
                    .lines()
                    .filter_map(|line| {
                        let line = line.trim();
                        let rest = line.strip_prefix("fn ")?;
                        let (name, _) = rest.split_once('(')?;
                        (name.ends_with("_view")
                            && line.contains("deadline_ms: Int) Int ="))
                            .then(|| name.to_string())
                    })
                    .collect::<Vec<_>>();
                assert!(!view_externs.is_empty(), "Lua probe lacks view externs");
                for name in view_externs {
                    let marker = format!("pub fn {name}(");
                    let start = source
                        .find(&marker)
                        .expect("every Lua view extern has a generated wrapper");
                    let body = &source[start..];
                    let body = body
                        .split_once("\n}\n")
                        .map(|(body, _)| body)
                        .unwrap_or(body);
                    assert!(
                        body.contains("decode_status(abi.take_error())"),
                        "Lua view `{name}` bypasses decode_status"
                    );
                }
            }
            assert!(
                source.contains("#Import module c."),
                "{} lacks the raw extern path",
                renderer.language.root()
            );
            assert!(
                source.contains("#Error"),
                "{} lacks a typed error domain",
                renderer.language.root()
            );
            assert_eq!(source.matches("json.parse(raw)").count(), 1);
            match renderer.protocol {
                DecoderProtocol::StandardEnvelope => {
                    assert_eq!(source.matches("response.field(\"ok\")").count(), 1);
                    assert!(!source.contains("decode_status("));
                }
                DecoderProtocol::LuaRawJson => {
                    assert_eq!(source.matches("response.field(\"ok\")").count(), 0);
                    assert_eq!(source.matches("fn decode_status(").count(), 1);
                    assert!(source.contains("if code == 4 -> return Err(LuaError.CommandFailed)"));
                    assert!(source.contains("if code == 6 -> return Err(LuaError.Protocol)"));
                    assert_eq!(
                        source.matches("abi.take_error()").count(),
                        source.matches("decode_response(raw, code)").count()
                            + source.matches("decode_status(abi.take_error())").count(),
                        "Lua status result bypasses a named decoder"
                    );
                    assert!(source.contains("decode_status(abi.take_error())"));
                }
            }
        }
    }
}
/// The requested generated surface. `Automatic` is a checked facade; `Native`
/// retains the foreign declaration's arity while keeping boundary checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindingShape {
    Automatic,
    Native,
}

impl BindingShape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Native => "native",
        }
    }

    pub fn parse(value: &str) -> Result<Self, BindingPlanError> {
        match value {
            "automatic" | "auto" => Ok(Self::Automatic),
            "native" => Ok(Self::Native),
            other => Err(BindingPlanError::InvalidInput(format!(
                "unknown binding shape `{other}`; use `automatic` or `native`"
            ))),
        }
    }
}

impl fmt::Display for BindingShape {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

/// Project-level binding drift policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindingPolicy {
    Automatic,
    Frozen,
}

impl BindingPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Frozen => "frozen",
        }
    }

    pub fn parse(value: &str) -> Result<Self, BindingPlanError> {
        match value {
            "automatic" | "auto" => Ok(Self::Automatic),
            "frozen" => Ok(Self::Frozen),
            other => Err(BindingPlanError::InvalidInput(format!(
                "unknown binding policy `{other}`; use `automatic` or `frozen`"
            ))),
        }
    }
}

impl fmt::Display for BindingPolicy {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

/// The unit carried by a foreign pointer/count pair.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CountUnit {
    Bytes,
    Elements(String),
    Unknown,
}

impl CountUnit {
    pub fn known(&self) -> bool {
        match self {
            Self::Bytes => true,
            Self::Elements(element) => !element.trim().is_empty(),
            Self::Unknown => false,
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::Bytes => "bytes".to_string(),
            Self::Elements(element) => format!("elements:{element}"),
            Self::Unknown => "unknown".to_string(),
        }
    }
}

/// Whether a count describes the complete extent or only a prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CountMeaning {
    FullExtent,
    Prefix,
    Unknown,
}

impl CountMeaning {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullExtent => "full-extent",
            Self::Prefix => "prefix",
            Self::Unknown => "unknown",
        }
    }
}

/// Retention evidence for a pointer crossing the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PointerRetention {
    BorrowedForCall,
    MayRetain,
    Unknown,
}

impl PointerRetention {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BorrowedForCall => "borrowed-for-call",
            Self::MayRetain => "may-retain",
            Self::Unknown => "unknown",
        }
    }
}

/// Canonical facts needed before a pointer/count adaptation can remove the
/// native count argument.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerCountFact {
    pub pointer_parameter: String,
    pub count_parameter: String,
    pub unit: CountUnit,
    pub meaning: CountMeaning,
    pub native_width_bits: u16,
    pub retention: PointerRetention,
}

impl PointerCountFact {
    pub fn new(
        pointer_parameter: impl Into<String>,
        count_parameter: impl Into<String>,
        unit: CountUnit,
        meaning: CountMeaning,
        native_width_bits: u16,
        retention: PointerRetention,
    ) -> Self {
        Self {
            pointer_parameter: pointer_parameter.into(),
            count_parameter: count_parameter.into(),
            unit,
            meaning,
            native_width_bits,
            retention,
        }
    }

    fn validate_names(&self) -> Result<(), BindingPlanError> {
        if self.pointer_parameter.trim().is_empty() || self.count_parameter.trim().is_empty() {
            return Err(BindingPlanError::InvalidInput(
                "pointer/count evidence needs both parameter names".to_string(),
            ));
        }
        if self.pointer_parameter == self.count_parameter {
            return Err(BindingPlanError::InvalidInput(
                "pointer/count evidence names the same parameter twice".to_string(),
            ));
        }
        Ok(())
    }
}

/// One parsed foreign operation supplied to the planner by a language binder.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingOperation {
    pub name: String,
    pub native_signature: String,
    pub result_type: String,
    pub pointer_count: Option<PointerCountFact>,
    pub nullable_return: bool,
    pub borrowed_view: bool,
    pub status_out: bool,
    pub partial_success: bool,
    pub fallible_close: bool,
    pub callback_transport: String,
    pub effects: String,
    pub ownership: String,
    pub failure_mapping: String,
    pub copies: String,
    pub placement: String,
    pub cleanup: String,
}

impl BindingOperation {
    pub fn new(
        name: impl Into<String>,
        native_signature: impl Into<String>,
        result_type: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            native_signature: native_signature.into(),
            result_type: result_type.into(),
            pointer_count: None,
            nullable_return: false,
            borrowed_view: false,
            status_out: false,
            partial_success: false,
            fallible_close: false,
            callback_transport: "none".to_string(),
            effects: "foreign".to_string(),
            ownership: "signature-declared".to_string(),
            failure_mapping: "preserve".to_string(),
            copies: "none".to_string(),
            placement: "caller".to_string(),
            cleanup: "none".to_string(),
        }
    }

    pub fn with_pointer_count(mut self, fact: PointerCountFact) -> Self {
        self.pointer_count = Some(fact);
        self
    }

    pub fn requires_protocol_evidence(&self) -> bool {
        self.nullable_return
            || self.borrowed_view
            || self.status_out
            || self.partial_success
            || self.fallible_close
            || self.callback_transport != "none"
    }
}

/// Exact inputs pinned by a generated binding plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingInputs {
    pub target: String,
    pub generator: String,
    pub artifact: String,
    pub dependencies: Vec<String>,
    pub compiler_flags: Vec<String>,
}

impl BindingInputs {
    pub fn new(
        target: impl Into<String>,
        generator: impl Into<String>,
        artifact: impl Into<String>,
    ) -> Self {
        Self {
            target: target.into(),
            generator: generator.into(),
            artifact: artifact.into(),
            dependencies: Vec::new(),
            compiler_flags: Vec::new(),
        }
    }

    pub fn with_dependencies<I, S>(mut self, dependencies: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.dependencies = sorted_strings(dependencies);
        self
    }

    pub fn with_compiler_flags<I, S>(mut self, flags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.compiler_flags = sorted_strings(flags);
        self
    }

    fn validate(&self) -> Result<(), BindingPlanError> {
        for (name, value) in [
            ("target", self.target.as_str()),
            ("generator", self.generator.as_str()),
            ("artifact", self.artifact.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(BindingPlanError::InvalidInput(format!(
                    "binding input `{name}` is empty"
                )));
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut identity = IdentityBuilder::new(FOREIGN_BOUNDARY_SCHEMA);
        identity.field("target", self.target.as_bytes());
        identity.field("generator", self.generator.as_bytes());
        identity.field("artifact", self.artifact.as_bytes());
        identity.field("dependencies", self.dependencies.join("\n").as_bytes());
        identity.field("compiler-flags", self.compiler_flags.join("\n").as_bytes());
        identity.finish()
    }
}

/// A stable, reviewable description of what an adaptation preserves.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlanSemantics {
    pub ownership: String,
    pub failure_mapping: String,
    pub copies: String,
    pub placement: String,
    pub effects: String,
    pub cleanup: String,
}

/// A resolved generated binding plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingPlan {
    pub operation: BindingOperation,
    pub library: String,
    pub shape: BindingShape,
    pub boundary_digest: String,
    pub input_digest: String,
    pub artifact: String,
    pub checked_obligations: Vec<String>,
    pub semantics: PlanSemantics,
    pub digest: String,
}

impl BindingPlan {
    /// Resolve one operation against the canonical boundary contract.
    ///
    /// Every automatic adaptation requires a complete artifact identity and
    /// guarantee-level evidence for the affected obligations. Unknown evidence
    /// produces one actionable refusal instead of a best-effort projection.
    pub fn resolve(
        contract: &ForeignBoundaryContract,
        operation: BindingOperation,
        inputs: BindingInputs,
        shape: BindingShape,
    ) -> Result<Self, BindingPlanError> {
        contract
            .validate()
            .map_err(BindingPlanError::Contract)?;
        inputs.validate()?;
        if operation.name.trim().is_empty() {
            return Err(BindingPlanError::InvalidInput(
                "binding operation needs a name".to_string(),
            ));
        }
        if let Some(pointer_count) = &operation.pointer_count {
            pointer_count.validate_names()?;
        }

        let mut required = vec!["abi", "layout", "width", "alignment", "target"];
        if operation.pointer_count.is_some() {
            required.extend(["ownership", "lifetime"]);
        }
        if operation.requires_protocol_evidence() {
            required.extend(["errors", "cleanup"]);
        }
        if operation.nullable_return {
            required.push("nullability");
        }
        if operation.fallible_close {
            required.push("cleanup");
        }
        required.sort_unstable();
        required.dedup();

        for obligation in &required {
            let admitted = if shape == BindingShape::Native {
                contract.has_complete_artifact_coverage()
                    && contract
                        .obligation(obligation)
                        .is_some_and(|row| !row.basis.is_unknown())
            } else {
                contract.permits_guarantee(obligation)
            };
            if !admitted {
                return Err(BindingPlanError::Refused(BindingRefusal::missing(
                    operation.name.clone(),
                    obligation,
                )));
            }
        }
        if let Some(pointer_count) = &operation.pointer_count {
            if pointer_count.native_width_bits == 0
                || pointer_count.native_width_bits > 64
                || pointer_count.native_width_bits % 8 != 0
            {
                return Err(BindingPlanError::Refused(BindingRefusal::new(
                    operation.name.clone(),
                    "width",
                    "The native count width is not checked. Record the target ABI width before adapting this pointer/count pair.",
                )));
            }
            if shape == BindingShape::Automatic {
                if !pointer_count.unit.known() || pointer_count.meaning == CountMeaning::Unknown {
                    return Err(BindingPlanError::Refused(BindingRefusal::new(
                        operation.name.clone(),
                        "count-unit",
                        "The pointer/count unit or meaning is unknown. Record bytes-versus-elements and full-extent-versus-prefix evidence, or select `--shape native`.",
                    )));
                }
                if pointer_count.retention != PointerRetention::BorrowedForCall {
                    return Err(BindingPlanError::Refused(BindingRefusal::new(
                        operation.name.clone(),
                        "lifetime",
                        "The foreign implementation may retain the pointer. Supply a checked borrowed-for-call contract or select `--shape native`; no copy or retry is inserted.",
                    )));
                }
            }
        }

        let mut checked_obligations = required
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        checked_obligations.sort();
        checked_obligations.dedup();
        let semantics = PlanSemantics {
            ownership: operation.ownership.clone(),
            failure_mapping: operation.failure_mapping.clone(),
            copies: operation.copies.clone(),
            placement: operation.placement.clone(),
            effects: operation.effects.clone(),
            cleanup: operation.cleanup.clone(),
        };
        let boundary_digest = contract.digest();
        let input_digest = inputs.digest();
        let digest = plan_digest(
            &operation,
            &contract.library,
            shape,
            &boundary_digest,
            &input_digest,
            &checked_obligations,
            &semantics,
        );
        Ok(Self {
            operation,
            library: contract.library.clone(),
            shape,
            boundary_digest,
            input_digest,
            artifact: inputs.artifact.clone(),
            checked_obligations,
            semantics,
            digest,
        })
    }

    pub fn candidate_digest(&self) -> &str {
        &self.digest
    }

    pub fn is_native(&self) -> bool {
        self.shape == BindingShape::Native
    }

    pub fn checked_count(
        &self,
        view_len: usize,
        requested: Option<u128>,
    ) -> Result<u128, BindingPlanError> {
        let Some(pointer_count) = &self.operation.pointer_count else {
            return Err(BindingPlanError::InvalidInput(
                "checked count requested for an operation without pointer/count evidence"
                    .to_string(),
            ));
        };
        let count = requested.unwrap_or(view_len as u128);
        if count > view_len as u128 {
            return Err(BindingPlanError::Refused(BindingRefusal::new(
                self.operation.name.clone(),
                "extent",
                "The requested count exceeds the view extent; shorten the count or pass the complete view.",
            )));
        }
        let maximum = (1u128 << pointer_count.native_width_bits) - 1;
        if count > maximum {
            return Err(BindingPlanError::Refused(BindingRefusal::new(
                self.operation.name.clone(),
                "width",
                "The requested count does not fit the pinned native width; no narrowing conversion is performed.",
            )));
        }
        Ok(count)
    }

    /// Render the generated facade and its explicit native call seam.
    pub fn render_facade(&self) -> String {
        let mut source = format!(
            "// jet-ffi-plan={}\n// jet-ffi-shape={}\n",
            self.digest, self.shape
        );
        source.push_str(&format!(
            "// jet-ffi-boundary={} input={}\n",
            self.boundary_digest, self.input_digest
        ));
        source.push_str(&format!(
            "// ownership={} failure={} copies={} placement={} effects={} cleanup={}\n",
            self.semantics.ownership,
            self.semantics.failure_mapping,
            self.semantics.copies,
            self.semantics.placement,
            self.semantics.effects,
            self.semantics.cleanup
        ));
        if self.operation.callback_transport != "none" {
            source.push_str(&format!(
                "// jet-ffi-callback-transport={}\n// jet-ffi-callback-identity={}\n// jet-ffi-callback-plan={}\n",
                self.operation.callback_transport,
                self.operation.name,
                self.digest
            ));
        }
        if self.shape == BindingShape::Automatic {
            if let Some(pointer_count) = &self.operation.pointer_count {
                source.push_str(&format!(
                    "#Import module c.{} {{\n    fn __jet_ffi_native_{}({}: &[U8], {}: U64) {} = \"{}\"\n}}\n\n",
                    self.library,
                    self.operation.name,
                    pointer_count.pointer_parameter,
                    pointer_count.count_parameter,
                    self.operation.result_type,
                    self.operation.name
                ));
                source.push_str(&format!(
                    "pub fn {}({}: &[U8]) {} -> {{\n    return __jet_ffi_native_{}({}, {}.len())\n}}\n",
                    self.operation.name,
                    pointer_count.pointer_parameter,
                    self.operation.result_type,
                    self.operation.name,
                    pointer_count.pointer_parameter,
                    pointer_count.pointer_parameter
                ));
            } else {
                source.push_str(&format!(
                    "// automatic facade preserves native signature: {}\n",
                    self.operation.native_signature
                ));
            }
        } else {
            source.push_str(&format!(
                "// native facade preserves arity: {}\n",
                self.operation.native_signature
            ));
        }
        source
    }

    /// Explain the exact diff that would be written by a mutating command.
    pub fn explain(&self) -> String {
        let count_unit = self
            .operation
            .pointer_count
            .as_ref()
            .map(|pointer_count| pointer_count.unit.as_str())
            .unwrap_or_else(|| "none".to_string());
        let ownership = &self.semantics.ownership;
        let failure = &self.semantics.failure_mapping;
        format!(
            "operation: {}\nlibrary: {}\nshape: {}\ncount units: {}\nownership: {}\nfailure mapping: {}\ncopies: {}\nplacement: {}\neffects: {}\ncleanup: {}\nartifact: {}\ninput digest: {}\nboundary evidence: {}\ncandidate digest: {}\n",
            self.operation.name,
            self.library,
            self.shape,
            count_unit,
            ownership,
            failure,
            self.semantics.copies,
            self.semantics.placement,
            self.semantics.effects,
            self.semantics.cleanup,
            self.artifact,
            self.input_digest,
            self.boundary_digest,
            self.digest
        )
    }

    pub fn frozen_drift(&self, recorded_digest: &str) -> Result<(), BindingPlanError> {
        if self.digest == recorded_digest {
            Ok(())
        } else {
            Err(BindingPlanError::FrozenDrift {
                recorded: recorded_digest.to_string(),
                candidate: self.digest.clone(),
            })
        }
    }
}

/// Resolve a plan without making callers spell the associated type.
pub fn resolve_plan(
    contract: &ForeignBoundaryContract,
    operation: BindingOperation,
    inputs: BindingInputs,
    shape: BindingShape,
) -> Result<BindingPlan, BindingPlanError> {
    BindingPlan::resolve(contract, operation, inputs, shape)
}

/// A single refusal keeps the command actionable and avoids a cascade of
/// secondary diagnostics caused by one missing canonical fact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingRefusal {
    pub operation: String,
    pub obligation: String,
    pub message: String,
}

impl BindingRefusal {
    fn new(operation: String, obligation: impl Into<String>, message: &str) -> Self {
        Self {
            operation,
            obligation: obligation.into(),
            message: message.to_string(),
        }
    }

    fn missing(operation: String, obligation: &str) -> Self {
        Self::new(
            operation,
            obligation,
            &format!(
                "Canonical foreign evidence for `{obligation}` is missing or not guarantee-level. Record the exact artifact evidence, then retry; no guessed adaptation is emitted."
            ),
        )
    }
}

impl fmt::Display for BindingRefusal {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "binding `{}` refused adaptation at `{}`: {}",
            self.operation, self.obligation, self.message
        )
    }
}

/// Errors from plan resolution, extent checks, and frozen-plan checks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindingPlanError {
    InvalidInput(String),
    Contract(String),
    Refused(BindingRefusal),
    FrozenDrift { recorded: String, candidate: String },
}

impl fmt::Display for BindingPlanError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => output.write_str(message),
            Self::Contract(message) => write!(output, "invalid foreign boundary contract: {message}"),
            Self::Refused(refusal) => refusal.fmt(output),
            Self::FrozenDrift {
                recorded,
                candidate,
            } => write!(
                output,
                "frozen binding plan drifted: recorded={recorded} candidate={candidate}; rerun with `--update --accept {candidate}`"
            ),
        }
    }
}

impl std::error::Error for BindingPlanError {}

fn sorted_strings<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut values = values
        .into_iter()
        .map(Into::into)
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn plan_digest(
    operation: &BindingOperation,
    library: &str,
    shape: BindingShape,
    boundary_digest: &str,
    input_digest: &str,
    checked_obligations: &[String],
    semantics: &PlanSemantics,
) -> String {
    let mut identity = IdentityBuilder::new(FOREIGN_BOUNDARY_SCHEMA);
    identity.field("library", library.as_bytes());
    identity.field("operation", operation.name.as_bytes());
    identity.field("native-signature", operation.native_signature.as_bytes());
    identity.field("result", operation.result_type.as_bytes());
    identity.field("shape", shape.as_str().as_bytes());
    identity.field("boundary", boundary_digest.as_bytes());
    identity.field("inputs", input_digest.as_bytes());
    identity.field("obligations", checked_obligations.join("\n").as_bytes());
    identity.field("ownership", semantics.ownership.as_bytes());
    identity.field("failure", semantics.failure_mapping.as_bytes());
    identity.field("copies", semantics.copies.as_bytes());
    identity.field("placement", semantics.placement.as_bytes());
    identity.field("effects", semantics.effects.as_bytes());
    identity.field("cleanup", semantics.cleanup.as_bytes());
    if let Some(pointer_count) = &operation.pointer_count {
        identity.field("pointer", pointer_count.pointer_parameter.as_bytes());
        identity.field("count", pointer_count.count_parameter.as_bytes());
        identity.field("unit", pointer_count.unit.as_str().as_bytes());
        identity.field("meaning", pointer_count.meaning.as_str().as_bytes());
        identity.field("width", pointer_count.native_width_bits.to_string().as_bytes());
        identity.field("retention", pointer_count.retention.as_str().as_bytes());
    }
    identity.finish()
}
