//! Stable JSON encoding for `SemIndex` (no external crates — I6 path deps only).

use jet_foundation::Diagnostics::Span;

use crate::Build::{SymDef, SymKind, SymRef};
use crate::Types::{
    BypassFact, BypassKind, CallEdge, DefinitionFact, EffectFact, MemberFact, MemberKind, MemberOrigin, SemIndex,
    SourceSpan, SymbolDef, SymbolKind, SymbolRef, TypeDossier,
};

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

fn json_definition_fact(f: &DefinitionFact) -> String {
    format!("{{\"stable_id\":{},\"signature_id\":{},\"content_id\":{},\"human_identity\":{},\"name\":{},\"kind\":{},\"module\":{},\"span\":{}}}", json_str(&f.stable_id), json_str(&f.signature_id), json_str(&f.content_id), json_str(&f.human_identity), json_str(&f.name), json_str(&f.kind), json_str(&f.module_path), json_span(f.span))
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
    let target_json = match &r.target {
        Some(target) => format!(
            "{{\"module\":{},\"kind\":{},\"span\":{}}}",
            json_str(&target.module_path),
            json_str(&target.kind),
            json_span(target.def_span)
        ),
        None => "null".to_string(),
    };
    format!(
        "{{\"name\":{},\"module\":{},\"scope_identity\":{},\"target\":{},\"span\":{}}}",
        json_str(&r.name),
        json_str(&r.module_path),
        scope_json,
        target_json,
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

fn json_member_kind(kind: &MemberKind) -> &'static str {
    match kind {
        MemberKind::Field => "field",
        MemberKind::Variant => "variant",
        MemberKind::Method => "method",
    }
}

fn json_origin(origin: &MemberOrigin) -> String {
    match origin {
        MemberOrigin::TypeBody => "{\"kind\":\"type_body\"}".to_string(),
        MemberOrigin::InherentImpl => "{\"kind\":\"inherent_impl\"}".to_string(),
        MemberOrigin::TraitImpl { trait_name } => format!(
            "{{\"kind\":\"trait_impl\",\"trait\":{}}}",
            json_str(trait_name)
        ),
        MemberOrigin::TraitRequirement { trait_name } => format!(
            "{{\"kind\":\"trait_requirement\",\"trait\":{}}}",
            json_str(trait_name)
        ),
    }
}

fn json_member(m: &MemberFact) -> String {
    format!(
        "{{\"owner\":{},\"name\":{},\"identity\":{},\"kind\":{},\"origin\":{},\"signature\":{},\"module\":{},\"span\":{}}}",
        json_str(&m.owner),
        json_str(&m.name),
        json_str(&m.identity),
        json_str(json_member_kind(&m.kind)),
        json_origin(&m.origin),
        json_str(&m.signature),
        json_str(&m.module_path),
        json_span(m.span)
    )
}

impl SemIndex {
    /// Stable JSON document for tests and `jet inspect semindex --json`.
    pub fn to_json(&self) -> String {
        let defs: Vec<String> = self.definitions().iter().map(json_def).collect();
        let refs: Vec<String> = self.references().iter().map(json_ref).collect();
        let calls: Vec<String> = self.call_edges().iter().map(json_call).collect();
        let effects: Vec<String> = self.effects().iter().map(json_effect).collect();
        let members: Vec<String> = self.members().iter().map(json_member).collect();
        let definition_facts: Vec<String> = self.definition_facts().iter().map(json_definition_fact).collect();
        format!(
            "{{\"schema_version\":{},\"definitions\":[{}],\"definition_facts\":[{}],\"references\":[{}],\"calls\":[{}],\"effects\":[{}],\"members\":[{}]}}",
            self.schema_version(),
            defs.join(","),
            definition_facts.join(","),
            refs.join(","),
            calls.join(","),
            effects.join(","),
            members.join(",")
        )
    }
}

impl TypeDossier {
    pub fn to_json(&self) -> String {
        let def_json = match &self.definition {
            Some(def) => json_def(def),
            None => "null".to_string(),
        };
        let members: Vec<String> = self.members.iter().map(json_member).collect();
        let refs: Vec<String> = self.references.iter().map(json_ref).collect();
        let bypasses: Vec<String> = self.bypass_facts.iter().map(json_bypass).collect();
        format!(
            "{{\"schema_version\":{},\"target\":{},\"definition\":{},\"members\":[{}],\"references\":[{}],\"bypass_facts\":[{}]}}",
            self.schema_version,
            json_str(&self.target),
            def_json,
            members.join(","),
            refs.join(","),
            bypasses.join(",")
        )
    }

    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "dossier `{}` (schema v{})\n",
            self.target, self.schema_version
        ));
        match &self.definition {
            Some(def) => {
                out.push_str(&format!(
                    "summary\n  defined: {}:{}..{}\n",
                    def.module_path, def.def_span.start, def.def_span.end
                ));
            }
            None => out.push_str("summary\n  defined: not found\n"),
        }
        out.push_str("members\n");
        if self.members.is_empty() {
            out.push_str("  none\n");
        }
        for m in &self.members {
            out.push_str(&format!(
                "  {} {} ({}) @ {}:{}..{}\n",
                json_member_kind(&m.kind),
                m.signature,
                origin_text(&m.origin),
                m.module_path,
                m.span.start,
                m.span.end
            ));
        }
        out.push_str(&format!("references\n  count: {}\n", self.references.len()));
        for r in &self.references {
            out.push_str(&format!(
                "  {}:{}..{}\n",
                r.module_path, r.span.start, r.span.end
            ));
        }
        // D-LINTPOLICY1=A: every spelled bypass in the program, named and
        // recorded — the override law's audit clause made visible.
        out.push_str(&format!(
            "bypass facts\n  count: {}\n",
            self.bypass_facts.len()
        ));
        for b in &self.bypass_facts {
            let detail = if b.detail.is_empty() {
                "(no reason given)".to_string()
            } else {
                b.detail.clone()
            };
            out.push_str(&format!(
                "  {} at `{}` ({}) @ {}:{}..{}\n",
                bypass_kind_text(b.kind),
                b.site,
                detail,
                b.module_path,
                b.span.start,
                b.span.end
            ));
        }
        out
    }
}

fn bypass_kind_text(kind: BypassKind) -> &'static str {
    match kind {
        BypassKind::UnsafeRegion => "#Unsafe region",
        BypassKind::UnsafeFn => "#Unsafe fn",
        BypassKind::ExplicitDrop => ".drop(reason)",
        BypassKind::LintAllow => "#[allow(lint)]",
    }
}

fn json_bypass(b: &BypassFact) -> String {
    format!(
        "{{\"kind\":{},\"site\":{},\"detail\":{},\"module_path\":{},\"span\":{}}}",
        json_str(b.kind.as_str()),
        json_str(&b.site),
        json_str(&b.detail),
        json_str(&b.module_path),
        json_span(b.span)
    )
}

fn origin_text(origin: &MemberOrigin) -> String {
    match origin {
        MemberOrigin::TypeBody => "type body".to_string(),
        MemberOrigin::InherentImpl => "impl".to_string(),
        MemberOrigin::TraitImpl { trait_name } => format!("impl {trait_name}"),
        MemberOrigin::TraitRequirement { trait_name } => format!("trait {trait_name}"),
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
            target: r.target.clone(),
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
