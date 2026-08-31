//! Shared generated-binding response decoders and renderer registry.
//!
//! The registry is deliberately next to the binders.  It is the denominator
//! for renderer tests, so adding an active message binder without adding its
//! generated-source proof cannot look complete by accident.

#[cfg(test)]
use crate::AST::ForeignLanguage;
#[cfg(test)]
use crate::AST::{BinderRuntime, BinderStatus, BindingStubKind, FOREIGN_BINDERS};

/// Wire shape consumed by a generated `decode_response` helper.
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
    if code == 1 -> return Err({error}.NotRunning)
    if code == 2 -> return Err({error}.Timeout)
    if code == 3 -> return Err({error}.Cancelled)
    if code == 5 -> return Err({error}.Limit)
    if code != 0 -> return Err({error}.Protocol)
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
            assert!(
                crate::Parser::parse(&tokens).is_ok(),
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
                let marker = format!("pub fn {name}(");
                let start = source
                    .find(&marker)
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
                source.contains("#Extern module c."),
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
