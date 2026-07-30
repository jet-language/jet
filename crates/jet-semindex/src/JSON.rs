//! Stable JSON encoding for `SemIndex` (no external crates — I6 path deps only).

use jet_foundation::JSON::json_escape;

use crate::Build::{SymDef, SymKind, SymRef};
use crate::Types::{
    BypassFact, BypassKind, CallEdge, DefinitionFact, EffectFact, InstanceFact, MemberFact, MemberKind, MemberOrigin, OutputFact, SemIndex,
    SourceSpan, SymbolDef, SymbolKind, SymbolRef, TypeDossier, ViewProjectionFact,
    ViewProvenanceFact, ViewSourceFact, ViewSourcePathFact,
};

fn json_instance(value: &InstanceFact) -> String {
    let arguments = value.arguments.iter().map(|value| json_str(value)).collect::<Vec<_>>().join(",");
    let applications = value.applications.iter().map(|application| format!("{{\"name\":{},\"module_path\":{},\"semantic_identity\":{},\"span\":{}}}", json_str(&application.name), json_str(&application.module_path), json_str(&application.semantic_identity), json_span(application.span))).collect::<Vec<_>>().join(",");
    let members = value.exported_members.iter().map(|value| json_str(value)).collect::<Vec<_>>().join(",");
    format!("{{\"name\":{},\"module_path\":{},\"fingerprint\":{},\"full_key\":{},\"template_definition_id\":{},\"template_span\":{},\"arguments\":[{}],\"applications\":[{}],\"exported_members\":[{}]}}",
        json_str(&value.name), json_str(&value.module_path), json_str(&value.fingerprint), json_str(&value.full_key_hex), json_str(&value.template_definition_id), json_span(value.template_span), arguments, applications, members)
}

fn json_output(value: &OutputFact) -> String {
    let params = value.entry.params.iter().map(|value| json_str(value)).collect::<Vec<_>>().join(",");
    let effects = value.entry.effects.iter().map(|value| json_str(value)).collect::<Vec<_>>().join(",");
    let return_type = value.entry.return_type.as_ref().map_or_else(|| "null".to_string(), |value| json_str(value));
    format!(
        "{{\"binding\":{},\"kind\":{},\"name\":{},\"module_path\":{},\"span\":{},\"entry\":{{\"identity\":{},\"name\":{},\"module_path\":{},\"definition_span\":{},\"reference_span\":{},\"params\":[{}],\"return_type\":{},\"authority\":{},\"effects\":[{}]}}}}",
        json_str(&value.binding), json_str(&value.kind), json_str(&value.name),
        json_str(&value.module_path), json_span(value.span),
        json_str(&value.entry.identity), json_str(&value.entry.name),
        json_str(&value.entry.module_path), json_span(value.entry.definition_span),
        json_span(value.entry.reference_span), params, return_type,
        json_str(&value.entry.authority), effects,
    )
}

fn json_definition_fact(f: &DefinitionFact) -> String {
    format!("{{\"stable_id\":{},\"signature_id\":{},\"content_id\":{},\"human_identity\":{},\"name\":{},\"kind\":{},\"module\":{},\"span\":{}}}", json_str(&f.stable_id), json_str(&f.signature_id), json_str(&f.content_id), json_str(&f.human_identity), json_str(&f.name), json_str(&f.kind), json_str(&f.module_path), json_span(f.span))
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

fn json_span(span: SourceSpan) -> String {
    format!("{{\"start\":{},\"end\":{}}}", span.start, span.end)
}

fn json_view_provenance(provenance: &ViewProvenanceFact) -> String {
    let sources = provenance
        .sources
        .iter()
        .map(|source_path| {
            let source = match &source_path.source {
                ViewSourceFact::Receiver => "{\"kind\":\"receiver\"}".to_string(),
                ViewSourceFact::Parameter(index) => {
                    format!("{{\"kind\":\"parameter\",\"index\":{index}}}")
                }
                ViewSourceFact::Static { module_path, name } => format!(
                    "{{\"kind\":\"static\",\"module\":{},\"name\":{}}}",
                    json_str(module_path),
                    json_str(name),
                ),
            };
            let projections = source_path
                .projections
                .iter()
                .map(|projection| match projection {
                    ViewProjectionFact::Field(name) => {
                        format!("{{\"kind\":\"field\",\"name\":{}}}", json_str(name))
                    }
                    ViewProjectionFact::Index => "{\"kind\":\"index\"}".to_string(),
                    ViewProjectionFact::Range => "{\"kind\":\"range\"}".to_string(),
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{\"source\":{source},\"projections\":[{projections}]}}")
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"output_path\":[{}],\"sources\":[{sources}],\"mutable\":{}}}",
        provenance
            .output_path
            .iter()
            .map(|part| json_str(part))
            .collect::<Vec<_>>()
            .join(","),
        provenance.mutable,
    )
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
                ps.join(","), ret_json
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
        SymbolKind::Type => "{\"kind\":\"type\"}".to_string(),
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
    let view_json = format!(
        "[{}]",
        d.view_provenance
            .iter()
            .map(json_view_provenance)
            .collect::<Vec<_>>()
            .join(",")
    );
    format!(
        "{{\"identity\":{},\"name\":{},\"module\":{},\"span\":{},\"detail\":{},\"view_provenance\":{}}}",
        json_str(&d.identity),
        json_str(&d.name),
        json_str(&d.module_path),
        json_span(d.def_span),
        json_kind(&d.kind),
        view_json,
    )
}

fn json_ref(r: &SymbolRef) -> String {
    let scope_json = match &r.scope_identity {
        Some(scope) => json_str(scope),
        None => "null".to_string(),
    };
    let target_json = match &r.target {
        Some(target) => format!(
            "{{\"module\":{},\"kind\":{},\"semantic_identity\":{},\"span\":{}}}",
            json_str(&target.module_path),
            json_str(&target.kind),
            target.semantic_identity.as_ref().map_or_else(|| "null".to_string(), |identity| json_str(identity)),
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
    let provenance = e
        .provenance
        .iter()
        .map(|origin| {
            let path = origin
                .call_path
                .iter()
                .map(|part| json_str(part))
                .collect::<Vec<_>>()
                .join(",");
            let spans = origin
                .spans
                .iter()
                .map(|span| json_span(*span))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"effect\":{},\"call_path\":[{}],\"spans\":[{}]}}",
                json_str(&origin.effect),
                path,
                spans
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"function\":{},\"direct\":[{}],\"callees\":[{}],\"inferred\":[{}],\"maximal\":{},\"provenance\":[{}]}}",
        json_str(&e.function),
        direct.join(","),
        callees.join(","),
        inferred.join(","),
        if e.maximal { "true" } else { "false" },
        provenance
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
        let instances: Vec<String> = self.instances().iter().map(json_instance).collect();
        let outputs: Vec<String> = self.outputs().iter().map(json_output).collect();
        format!(
            "{{\"schema_version\":{},\"definitions\":[{}],\"definition_facts\":[{}],\"instances\":[{}],\"outputs\":[{}],\"references\":[{}],\"calls\":[{}],\"effects\":[{}],\"members\":[{}]}}",
            self.schema_version(),
            defs.join(","),
            definition_facts.join(","),
            instances.join(","),
            outputs.join(","),
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

pub(crate) fn convert_defs(
    defs: &[SymDef],
    view_provenance: &std::collections::HashMap<String, jet_foundation::AST::ViewProvenanceMap>,
) -> Vec<SymbolDef> {
    defs.iter()
        .map(|d| SymbolDef {
            identity: d.identity.clone(),
            name: d.name.clone(),
            module_path: d.module_path.clone(),
            def_span: d.def_span.into(),
            kind: convert_kind(&d.kind),
            view_provenance: view_provenance
                .get(&d.identity)
                .map(|map| {
                    map.iter()
                        .map(|(path, provenance)| convert_view_provenance(path, provenance))
                        .collect()
                })
                .unwrap_or_default(),
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
        SymKind::Function { params, ret, .. } => SymbolKind::Function {
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
        SymKind::Type => SymbolKind::Type,
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

fn convert_view_provenance(
    output_path: &[String],
    provenance: &jet_foundation::AST::ViewProvenance,
) -> ViewProvenanceFact {
    use jet_foundation::AST::{ViewSource, ViewSourceProjection};

    ViewProvenanceFact {
        output_path: output_path.to_vec(),
        sources: provenance
            .sources
            .iter()
            .map(|source_path| ViewSourcePathFact {
                source: match &source_path.source {
                    ViewSource::Receiver => ViewSourceFact::Receiver,
                    ViewSource::Parameter(index) => ViewSourceFact::Parameter(*index),
                    ViewSource::Static { module_path, name } => ViewSourceFact::Static {
                        module_path: module_path.clone(),
                        name: name.clone(),
                    },
                },
                projections: source_path
                    .projections
                    .iter()
                    .map(|projection| match projection {
                        ViewSourceProjection::Field(name) => {
                            ViewProjectionFact::Field(name.clone())
                        }
                        ViewSourceProjection::Index => ViewProjectionFact::Index,
                        ViewSourceProjection::Range => ViewProjectionFact::Range,
                    })
                    .collect(),
            })
            .collect(),
        mutable: provenance.mutable,
    }
}

pub(crate) fn convert_effects(facts: &jet_sema::SemIndexEffectFacts) -> Vec<EffectFact> {
    fn witness(
        function: &str,
        effect: &str,
        facts: &jet_sema::SemIndexEffectFacts,
        seen: &mut std::collections::BTreeSet<String>,
    ) -> Option<(Vec<String>, Vec<SourceSpan>)> {
        if !seen.insert(function.to_string()) {
            return None;
        }
        let summary = facts.summaries.get(function)?;
        if summary.direct.contains(effect) {
            let spans = summary
                .direct_spans
                .get(effect)
                .copied()
                .map(SourceSpan::from)
                .into_iter()
                .collect();
            return Some((vec![function.to_string()], spans));
        }
        if summary.maximal {
            let spans = summary
                .maximal_span
                .map(SourceSpan::from)
                .into_iter()
                .collect();
            return Some((vec![function.to_string()], spans));
        }
        for callee in &summary.edges {
            if !facts
                .solved
                .get(callee)
                .is_some_and(|row| row.contains(effect))
            {
                continue;
            }
            let mut branch_seen = seen.clone();
            if let Some((mut path, mut spans)) = witness(callee, effect, facts, &mut branch_seen) {
                let call_span = summary
                    .memory
                    .calls
                    .iter()
                    .find(|call| call.callee == *callee)
                    .map(|call| SourceSpan::from(call.span));
                path.insert(0, function.to_string());
                if let Some(span) = call_span {
                    spans.insert(0, span);
                }
                return Some((path, spans));
            }
        }
        None
    }

    let mut out = Vec::new();
    for (function, summary) in &facts.summaries {
        let direct: Vec<String> = summary.direct.iter().cloned().collect();
        let callees: Vec<String> = summary.edges.iter().cloned().collect();
        let inferred: Vec<String> = facts
            .solved
            .get(function)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        let provenance = inferred
            .iter()
            .filter_map(|effect| {
                witness(
                    function,
                    effect,
                    facts,
                    &mut std::collections::BTreeSet::new(),
                )
                .map(|(call_path, spans)| crate::Types::EffectProvenance {
                    effect: effect.clone(),
                    call_path,
                    spans,
                })
            })
            .collect();
        out.push(EffectFact {
            function: function.clone(),
            direct,
            callees,
            inferred,
            maximal: summary.maximal,
            provenance,
        });
    }
    out.sort_by(|a, b| a.function.cmp(&b.function));
    out
}
