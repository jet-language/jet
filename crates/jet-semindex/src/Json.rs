//! Stable JSON encoding for `SemIndex` (no external crates — I6 path deps only).

use jet_foundation::Diagnostics::Span;

use crate::Build::{SymDef, SymKind, SymRef};
use crate::Types::{CallEdge, EffectFact, SemIndex, SourceSpan, SymbolDef, SymbolKind, SymbolRef};

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

fn json_span(span: SourceSpan) -> String {
    format!("{{\"start\":{},\"end\":{}}}", span.start, span.end)
}

#[allow(dead_code)]
fn json_span_raw(span: Span) -> String {
    json_span(span.into())
}

fn json_kind(kind: &SymbolKind) -> String {
    match kind {
        SymbolKind::Module => "{\"kind\":\"module\"}".to_string(),
        SymbolKind::Function { params, ret } => {
            let ps: Vec<String> = params
                .iter()
                .map(|(n, t)| format!("{{\"name\":{},\"type\":{}}}", json_str(n), json_str(t)))
                .collect();
            let ret_json = match ret {
                Some(t) => json_str(t),
                None => "null".to_string(),
            };
            format!(
                "{{\"kind\":\"function\",\"params\":[{}],\"ret\":{}}}",
                ps.join(","),
                ret_json
            )
        }
        SymbolKind::Struct { fields } => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(n, t)| format!("{{\"name\":{},\"type\":{}}}", json_str(n), json_str(t)))
                .collect();
            format!("{{\"kind\":\"struct\",\"fields\":[{}]}}", fs.join(","))
        }
        SymbolKind::Enum { variants } => {
            let vs: Vec<String> = variants.iter().map(|v| json_str(v)).collect();
            format!("{{\"kind\":\"enum\",\"variants\":[{}]}}", vs.join(","))
        }
        SymbolKind::Trait => "{\"kind\":\"trait\"}".to_string(),
        SymbolKind::Tag => "{\"kind\":\"tag\"}".to_string(),
        SymbolKind::Const => "{\"kind\":\"const\"}".to_string(),
        SymbolKind::EnumVariant { parent } => format!(
            "{{\"kind\":\"enum_variant\",\"parent\":{}}}",
            json_str(parent)
        ),
        SymbolKind::Field { ty, parent } => format!(
            "{{\"kind\":\"field\",\"type\":{},\"parent\":{}}}",
            json_str(ty),
            json_str(parent)
        ),
        SymbolKind::Local { mutable, ty } => {
            let ty_json = match ty {
                Some(t) => json_str(t),
                None => "null".to_string(),
            };
            format!(
                "{{\"kind\":\"local\",\"mutable\":{},\"type\":{}}}",
                if *mutable { "true" } else { "false" },
                ty_json
            )
        }
        SymbolKind::Param { ty } => format!("{{\"kind\":\"param\",\"type\":{}}}", json_str(ty)),
    }
}

fn json_def(d: &SymbolDef) -> String {
    format!(
        "{{\"identity\":{},\"name\":{},\"module\":{},\"span\":{},\"detail\":{}}}",
        json_str(&d.identity),
        json_str(&d.name),
        json_str(&d.module_path),
        json_span(d.def_span),
        json_kind(&d.kind)
    )
}

fn json_ref(r: &SymbolRef) -> String {
    let scope_json = match &r.scope_identity {
        Some(scope) => json_str(scope),
        None => "null".to_string(),
    };
    format!(
        "{{\"name\":{},\"module\":{},\"scope_identity\":{},\"span\":{}}}",
        json_str(&r.name),
        json_str(&r.module_path),
        scope_json,
        json_span(r.span)
    )
}

fn json_call(c: &CallEdge) -> String {
    format!(
        "{{\"caller\":{},\"callee\":{},\"module\":{},\"span\":{}}}",
        json_str(&c.caller),
        json_str(&c.callee),
        json_str(&c.module_path),
        json_span(c.call_span)
    )
}

fn json_effect(e: &EffectFact) -> String {
    let direct: Vec<String> = e.direct.iter().map(|s| json_str(s)).collect();
    let callees: Vec<String> = e.callees.iter().map(|s| json_str(s)).collect();
    let inferred: Vec<String> = e.inferred.iter().map(|s| json_str(s)).collect();
    format!(
        "{{\"function\":{},\"direct\":[{}],\"callees\":[{}],\"inferred\":[{}],\"maximal\":{}}}",
        json_str(&e.function),
        direct.join(","),
        callees.join(","),
        inferred.join(","),
        if e.maximal { "true" } else { "false" }
    )
}

impl SemIndex {
    /// Stable JSON document for tests and `jet semindex --json`.
    pub fn to_json(&self) -> String {
        let defs: Vec<String> = self.definitions().iter().map(json_def).collect();
        let refs: Vec<String> = self.references().iter().map(json_ref).collect();
        let calls: Vec<String> = self.call_edges().iter().map(json_call).collect();
        let effects: Vec<String> = self.effects().iter().map(json_effect).collect();
        format!(
            "{{\"schema_version\":{},\"definitions\":[{}],\"references\":[{}],\"calls\":[{}],\"effects\":[{}]}}",
            self.schema_version(),
            defs.join(","),
            refs.join(","),
            calls.join(","),
            effects.join(",")
        )
    }
}

#[allow(dead_code)]
pub(crate) fn lsp_sym_kind_json(kind: &SymKind) -> String {
    match kind {
        SymKind::Module => "{\"kind\":\"module\"}".to_string(),
        SymKind::Function { params, ret } => {
            let ps: Vec<String> = params
                .iter()
                .map(|(n, t)| {
                    format!(
                        "{{\"name\":{},\"type\":{}}}",
                        json_str(n),
                        json_str(&t.name())
                    )
                })
                .collect();
            let ret_json = match ret {
                Some(t) => json_str(&t.name()),
                None => "null".to_string(),
            };
            format!(
                "{{\"kind\":\"function\",\"params\":[{}],\"ret\":{}}}",
                ps.join(","),
                ret_json
            )
        }
        SymKind::Struct { fields } => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(n, t)| {
                    format!(
                        "{{\"name\":{},\"type\":{}}}",
                        json_str(n),
                        json_str(&t.name())
                    )
                })
                .collect();
            format!("{{\"kind\":\"struct\",\"fields\":[{}]}}", fs.join(","))
        }
        SymKind::Enum { variants } => {
            let vs: Vec<String> = variants.iter().map(|v| json_str(v)).collect();
            format!("{{\"kind\":\"enum\",\"variants\":[{}]}}", vs.join(","))
        }
        SymKind::Trait => "{\"kind\":\"trait\"}".to_string(),
        SymKind::Tag => "{\"kind\":\"tag\"}".to_string(),
        SymKind::Const => "{\"kind\":\"const\"}".to_string(),
        SymKind::EnumVariant { parent } => format!(
            "{{\"kind\":\"enum_variant\",\"parent\":{}}}",
            json_str(parent)
        ),
        SymKind::Field { ty, parent } => format!(
            "{{\"kind\":\"field\",\"type\":{},\"parent\":{}}}",
            json_str(&ty.name()),
            json_str(parent)
        ),
        SymKind::Local { mutable, ty } => {
            let ty_json = match ty {
                Some(t) => json_str(&t.name()),
                None => "null".to_string(),
            };
            format!(
                "{{\"kind\":\"local\",\"mutable\":{},\"type\":{}}}",
                if *mutable { "true" } else { "false" },
                ty_json
            )
        }
        SymKind::Param { ty } => {
            format!("{{\"kind\":\"param\",\"type\":{}}}", json_str(&ty.name()))
        }
    }
}

pub(crate) fn convert_defs(defs: &[SymDef]) -> Vec<SymbolDef> {
    defs.iter()
        .map(|d| SymbolDef {
            identity: d.identity.clone(),
            name: d.name.clone(),
            module_path: d.module_path.clone(),
            def_span: d.def_span.into(),
            kind: convert_kind(&d.kind),
        })
        .collect()
}

pub(crate) fn convert_refs(refs: &[SymRef]) -> Vec<SymbolRef> {
    refs.iter()
        .map(|r| SymbolRef {
            name: r.name.clone(),
            module_path: r.module_path.clone(),
            scope_identity: r.scope_identity.clone(),
            span: r.span.into(),
        })
        .collect()
}

fn convert_kind(kind: &SymKind) -> SymbolKind {
    match kind {
        SymKind::Module => SymbolKind::Module,
        SymKind::Function { params, ret } => SymbolKind::Function {
            params: params.iter().map(|(n, t)| (n.clone(), t.name())).collect(),
            ret: ret.as_ref().map(|t| t.name()),
        },
        SymKind::Struct { fields } => SymbolKind::Struct {
            fields: fields.iter().map(|(n, t)| (n.clone(), t.name())).collect(),
        },
        SymKind::Enum { variants } => SymbolKind::Enum {
            variants: variants.clone(),
        },
        SymKind::Trait => SymbolKind::Trait,
        SymKind::Tag => SymbolKind::Tag,
        SymKind::Const => SymbolKind::Const,
        SymKind::EnumVariant { parent } => SymbolKind::EnumVariant {
            parent: parent.clone(),
        },
        SymKind::Field { ty, parent } => SymbolKind::Field {
            ty: ty.name(),
            parent: parent.clone(),
        },
        SymKind::Local { mutable, ty } => SymbolKind::Local {
            mutable: *mutable,
            ty: ty.as_ref().map(|t| t.name()),
        },
        SymKind::Param { ty } => SymbolKind::Param { ty: ty.name() },
    }
}

pub(crate) fn convert_effects(facts: &jet_sema::SemIndexEffectFacts) -> Vec<EffectFact> {
    let mut out = Vec::new();
    for (function, summary) in &facts.summaries {
        let direct: Vec<String> = summary.direct.iter().cloned().collect();
        let callees: Vec<String> = summary.edges.iter().cloned().collect();
        let inferred: Vec<String> = facts
            .solved
            .get(function)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        out.push(EffectFact {
            function: function.clone(),
            direct,
            callees,
            inferred,
            maximal: summary.maximal,
        });
    }
    out.sort_by(|a, b| a.function.cmp(&b.function));
    out
}
