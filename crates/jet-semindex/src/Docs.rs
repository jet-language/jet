//! Stable documentation projection from the checked semantic graph.
//!
//! This module owns the producer-side shape of `jet doc`.  The command only
//! checks a bundle, asks this module for a projection, and chooses an output
//! format.  It does not walk the AST a second time.

use crate::Types::SemIndex;
use jet_foundation::AST::{Item, ProgramBundle};
use jet_foundation::Diagnostics::Span;
use std::collections::BTreeSet;
use std::path::Path;

pub const DOC_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocSource {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub link: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocItem {
    pub kind: String,
    pub name: String,
    pub qualified_name: String,
    pub module: String,
    pub public: bool,
    pub package_public: bool,
    pub signature: String,
    /// Effective failure carrier from the checked callable fact. `None` is
    /// intentional for non-callable documentation items.
    pub failure_contract: Option<String>,
    /// Provenance of the effective failure carrier: implicit, explicit,
    /// converted, or proven unreachable.
    pub failure_source: Option<String>,
    pub summary: String,
    pub examples: Vec<String>,
    pub markers: Vec<String>,
    pub source: DocSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocModule {
    pub name: String,
    pub path: String,
    pub public: bool,
    pub summary: String,
    pub source: DocSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocImpl {
    pub kind: String,
    pub type_name: String,
    pub trait_name: Option<String>,
    pub module: String,
    pub public: bool,
    pub methods: Vec<String>,
    pub source: DocSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocExpectation {
    pub expression: String,
    pub expected: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocTest {
    pub module: String,
    pub line: usize,
    pub setup: Vec<String>,
    pub expectations: Vec<DocExpectation>,
    pub source: DocSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocGraph {
    pub schema_version: u32,
    pub root: String,
    pub modules: Vec<DocModule>,
    pub items: Vec<DocItem>,
    pub impls: Vec<DocImpl>,
    pub doctests: Vec<DocTest>,
}

/// Build the documentation projection from one checked bundle and its
/// semindex.  The index argument is deliberately part of the public seam: a
/// future consumer can enrich this projection from semantic facts without
/// making the CLI own a second graph walk.
pub fn build_doc_graph(bundle: &ProgramBundle, index: &SemIndex) -> DocGraph {
    let mut protocol_names = BTreeSet::new();
    for module in &bundle.modules {
        collect_protocol_names(&module.items, &mut protocol_names);
    }

    let mut graph = DocGraph {
        schema_version: DOC_SCHEMA_VERSION,
        root: stable_root(&bundle.project_root),
        modules: Vec::new(),
        items: Vec::new(),
        impls: Vec::new(),
        doctests: Vec::new(),
    };

    for module in &bundle.modules {
        let module_path = stable_module_path(&bundle.project_root, &module.path, &module.display);
        let module_name = module_name(&module_path, &module.alias);
        let module_source = source_for(
            &bundle.project_root,
            &module.path,
            &module.display,
            &module.source,
            Span::new(0, 0),
        );
        graph.modules.push(DocModule {
            name: module_name.clone(),
            path: module_path.clone(),
            public: module.pub_file,
            summary: leading_summary(&module.source),
            source: module_source,
        });
        graph
            .doctests
            .extend(discover_doctests(
                &bundle.project_root,
                &module.path,
                &module.display,
                &module.source,
                &module_path,
            ));
        collect_items(
            &module.items,
            &module_path,
            "",
            module.pub_file,
            &module.path,
            &module.display,
            &module.source,
            &bundle.project_root,
            &protocol_names,
            &mut graph.items,
            &mut graph.impls,
        );
    }

    enrich_callable_contracts(&mut graph.items, index);

    graph.modules.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.name.cmp(&right.name))
    });
    graph.items.sort_by(|left, right| {
        left.module
            .cmp(&right.module)
            .then(left.qualified_name.cmp(&right.qualified_name))
            .then(kind_rank(&left.kind).cmp(&kind_rank(&right.kind)))
            .then(left.source.start.cmp(&right.source.start))
    });
    graph.impls.sort_by(|left, right| {
        left.module
            .cmp(&right.module)
            .then(left.type_name.cmp(&right.type_name))
            .then(left.trait_name.cmp(&right.trait_name))
            .then(left.source.start.cmp(&right.source.start))
    });
    graph.doctests.sort_by(|left, right| {
        left.module
            .cmp(&right.module)
            .then(left.line.cmp(&right.line))
    });
    graph
}

impl DocGraph {
    pub fn undocumented_public(&self) -> Vec<&DocItem> {
        self.items
            .iter()
            .filter(|item| item.public && item.summary.is_empty())
            .collect()
    }

    /// Stable machine-readable output.  Field order is explicit because this
    /// is also the checked artifact used by fixture tests.
    pub fn to_json(&self) -> String {
        let modules = self
            .modules
            .iter()
            .map(DocModule::to_json)
            .collect::<Vec<_>>()
            .join(",");
        let items = self
            .items
            .iter()
            .map(DocItem::to_json)
            .collect::<Vec<_>>()
            .join(",");
        let impls = self
            .impls
            .iter()
            .map(DocImpl::to_json)
            .collect::<Vec<_>>()
            .join(",");
        let doctests = self
            .doctests
            .iter()
            .map(DocTest::to_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema_version\":{},\"root\":{},\"modules\":[{}],\"items\":[{}],\"impls\":[{}],\"doctests\":[{}]}}",
            self.schema_version,
            json_string(&self.root),
            modules,
            items,
            impls,
            doctests
        )
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::from("# Jet Documentation\n\n");
        out.push_str("Generated from the checked semantic graph.\n\n");
        out.push_str("## Modules\n\n");
        for module in &self.modules {
            out.push_str(&format!(
                "- `{}` — `{}` ([source]({}))\n",
                module.name,
                module.path,
                docs_link(&module.source.link)
            ));
            if !module.summary.is_empty() {
                out.push_str(&format!("  {}\n", module.summary));
            }
        }
        out.push_str("\n## API\n\n");
        for item in self.items.iter().filter(|item| item.public) {
            out.push_str(&format!(
                "### `{}`\n\n{}\n\n",
                item.qualified_name,
                if item.summary.is_empty() {
                    "Undocumented public item."
                } else {
                    item.summary.as_str()
                }
            ));
            out.push_str(&format!("```jet\n{}\n```\n\n", item.signature));
            if !item.examples.is_empty() {
                out.push_str("Examples:\n\n");
                for example in &item.examples {
                    out.push_str(&format!("- `{}`\n", example));
                }
                out.push('\n');
            }
            out.push_str(&format!("[Source]({})\n\n", docs_link(&item.source.link)));
        }
        if !self.impls.is_empty() {
            out.push_str("## Implementations\n\n");
            for implementation in &self.impls {
                let trait_name = implementation
                    .trait_name
                    .as_deref()
                    .unwrap_or("inherent");
                out.push_str(&format!(
                    "- `{}` for `{}` in `{}` ([source]({}))\n",
                    trait_name,
                    implementation.type_name,
                    implementation.module,
                    docs_link(&implementation.source.link)
                ));
            }
            out.push('\n');
        }
        if !self.doctests.is_empty() {
            out.push_str("## Doctests\n\n");
            for doctest in &self.doctests {
                out.push_str(&format!(
                    "### `{}` line {}\n\n",
                    doctest.module, doctest.line
                ));
                out.push_str("```jet\n");
                for line in &doctest.setup {
                    out.push_str(line);
                    out.push('\n');
                }
                for expectation in &doctest.expectations {
                    out.push_str(&format!(
                        "{} // => {}\n",
                        expectation.expression, expectation.expected
                    ));
                }
                out.push_str("```\n\n");
                out.push_str(&format!("[Source]({})\n\n", docs_link(&doctest.source.link)));
            }
        }
        out
    }

    pub fn to_html(&self) -> String {
        let mut out = String::from(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Jet Documentation</title></head><body><h1>Jet Documentation</h1>",
        );
        out.push_str("<p>Generated from the checked semantic graph.</p><h2>Modules</h2><ul>");
        for module in &self.modules {
            out.push_str(&format!(
                "<li><code>{}</code> — <code>{}</code> (<a href=\"{}\">source</a>)",
                html_escape(&module.name),
                html_escape(&module.path),
                html_escape(&docs_link(&module.source.link))
            ));
            if !module.summary.is_empty() {
                out.push_str(&format!("<p>{}</p>", html_escape(&module.summary)));
            }
            out.push_str("</li>");
        }
        out.push_str("</ul><h2>API</h2>");
        for item in self.items.iter().filter(|item| item.public) {
            out.push_str(&format!(
                "<article><h3 id=\"{}\"><code>{}</code></h3><p>{}</p><pre><code>{}</code></pre>",
                html_escape(&item.qualified_name),
                html_escape(&item.qualified_name),
                html_escape(if item.summary.is_empty() {
                    "Undocumented public item."
                } else {
                    item.summary.as_str()
                }),
                html_escape(&item.signature)
            ));
            if !item.examples.is_empty() {
                out.push_str("<h4>Examples</h4><ul>");
                for example in &item.examples {
                    out.push_str(&format!("<li><code>{}</code></li>", html_escape(example)));
                }
                out.push_str("</ul>");
            }
            out.push_str(&format!(
                "<p><a href=\"{}\">Source</a></p></article>",
                html_escape(&docs_link(&item.source.link))
            ));
        }
        if !self.impls.is_empty() {
            out.push_str("<h2>Implementations</h2><ul>");
            for implementation in &self.impls {
                out.push_str(&format!(
                    "<li><code>{}</code> for <code>{}</code> (<a href=\"{}\">source</a>)</li>",
                    html_escape(
                        implementation
                            .trait_name
                            .as_deref()
                            .unwrap_or("inherent")
                    ),
                    html_escape(&implementation.type_name),
                    html_escape(&docs_link(&implementation.source.link))
                ));
            }
            out.push_str("</ul>");
        }
        if !self.doctests.is_empty() {
            out.push_str("<h2>Doctests</h2>");
            for doctest in &self.doctests {
                out.push_str(&format!(
                    "<article><h3><code>{}</code> line {}</h3><pre><code>",
                    html_escape(&doctest.module),
                    doctest.line
                ));
                for line in &doctest.setup {
                    out.push_str(&html_escape(line));
                    out.push('\n');
                }
                for expectation in &doctest.expectations {
                    out.push_str(&format!(
                        "{} // =&gt; {}\n",
                        html_escape(&expectation.expression),
                        html_escape(&expectation.expected)
                    ));
                }
                out.push_str(&format!(
                    "</code></pre><a href=\"{}\">Source</a></article>",
                    html_escape(&docs_link(&doctest.source.link))
                ));
            }
        }
        out.push_str("</body></html>\n");
        out
    }
}

impl DocSource {
    fn to_json(&self) -> String {
        format!(
            "{{\"path\":{},\"start\":{},\"end\":{},\"line\":{},\"column\":{},\"link\":{}}}",
            json_string(&self.path),
            self.start,
            self.end,
            self.line,
            self.column,
            json_string(&self.link)
        )
    }
}

impl DocModule {
    fn to_json(&self) -> String {
        format!(
            "{{\"name\":{},\"path\":{},\"public\":{},\"summary\":{},\"source\":{}}}",
            json_string(&self.name),
            json_string(&self.path),
            self.public,
            json_string(&self.summary),
            self.source.to_json()
        )
    }
}

impl DocItem {
    fn to_json(&self) -> String {
        format!(
            "{{\"kind\":{},\"name\":{},\"qualified_name\":{},\"module\":{},\"public\":{},\"package_public\":{},\"signature\":{},\"failure_contract\":{},\"failure_source\":{},\"summary\":{},\"examples\":{},\"markers\":{},\"source\":{}}}",
            json_string(&self.kind),
            json_string(&self.name),
            json_string(&self.qualified_name),
            json_string(&self.module),
            self.public,
            self.package_public,
            json_string(&self.signature),
            optional_json_string(self.failure_contract.as_deref()),
            optional_json_string(self.failure_source.as_deref()),
            json_string(&self.summary),
            json_array(&self.examples),
            json_array(&self.markers),
            self.source.to_json()
        )
    }
}

impl DocImpl {
    fn to_json(&self) -> String {
        format!(
            "{{\"kind\":{},\"type_name\":{},\"trait_name\":{},\"module\":{},\"public\":{},\"methods\":{},\"source\":{}}}",
            json_string(&self.kind),
            json_string(&self.type_name),
            self.trait_name
                .as_deref()
                .map_or_else(|| "null".to_string(), json_string),
            json_string(&self.module),
            self.public,
            json_array(&self.methods),
            self.source.to_json()
        )
    }
}

impl DocTest {
    fn to_json(&self) -> String {
        let expectations = self
            .expectations
            .iter()
            .map(|expectation| {
                format!(
                    "{{\"expression\":{},\"expected\":{}}}",
                    json_string(&expectation.expression),
                    json_string(&expectation.expected)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"module\":{},\"line\":{},\"setup\":{},\"expectations\":[{}],\"source\":{}}}",
            json_string(&self.module),
            self.line,
            json_array(&self.setup),
            expectations,
            self.source.to_json()
        )
    }
}

fn collect_protocol_names(items: &[Item], names: &mut BTreeSet<String>) {
    for item in items {
        match item {
            Item::ProtocolDecl(protocol) => {
                names.insert(protocol.name.clone());
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_protocol_names(body, names);
                }
            }
            Item::GenericModule(module) => collect_protocol_names(&module.body, names),
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_items(
    items: &[Item],
    module_path: &str,
    parent: &str,
    inherited_public: bool,
    file_path: &Path,
    display_path: &str,
    source: &str,
    root: &Path,
    protocol_names: &BTreeSet<String>,
    output: &mut Vec<DocItem>,
    impls: &mut Vec<DocImpl>,
) {
    for item in items {
        match item {
            Item::Func(function) => add_function(
                function,
                module_path,
                parent,
                inherited_public,
                file_path,
                display_path,
                source,
                root,
                output,
            ),
            Item::Struct(definition) => {
                add_item(
                    output,
                    "struct",
                    &definition.name,
                    parent,
                    module_path,
                    definition.is_pub || inherited_public,
                    definition.is_package_pub,
                    definition.span,
                    definition.name_span,
                    &definition.type_markers,
                    file_path,
                    display_path,
                    source,
                    root,
                );
                let owner = qualified(parent, &definition.name);
                for field in &definition.fields {
                    add_item(
                        output,
                        "field",
                        &field.name,
                        &owner,
                        module_path,
                        field.is_pub,
                        field.is_package_pub,
                        field.name_span,
                        field.name_span,
                        &field.serde_markers,
                        file_path,
                        display_path,
                        source,
                        root,
                    );
                }
                for method in &definition.methods {
                    add_function(
                        method,
                        module_path,
                        &owner,
                        definition.is_pub || inherited_public,
                        file_path,
                        display_path,
                        source,
                        root,
                        output,
                    );
                }
                for trait_impl in &definition.trait_impls {
                    add_nested_impl(
                        trait_impl.trait_name.as_str(),
                        &definition.name,
                        &trait_impl.methods,
                        module_path,
                        definition.is_pub || inherited_public,
                        definition.span,
                        file_path,
                        display_path,
                        source,
                        root,
                        output,
                        impls,
                    );
                }
            }
            Item::Enum(definition) => {
                add_item(
                    output,
                    "enum",
                    &definition.name,
                    parent,
                    module_path,
                    definition.is_pub || inherited_public,
                    definition.is_package_pub,
                    definition.span,
                    definition.name_span,
                    &[],
                    file_path,
                    display_path,
                    source,
                    root,
                );
                let owner = qualified(parent, &definition.name);
                for variant in &definition.variants {
                    add_item(
                        output,
                        "variant",
                        &variant.name,
                        &owner,
                        module_path,
                        definition.is_pub || inherited_public,
                        false,
                        variant.name_span,
                        variant.name_span,
                        &variant.serde_markers,
                        file_path,
                        display_path,
                        source,
                        root,
                    );
                }
                for method in &definition.methods {
                    add_function(
                        method,
                        module_path,
                        &owner,
                        definition.is_pub || inherited_public,
                        file_path,
                        display_path,
                        source,
                        root,
                        output,
                    );
                }
                for trait_impl in &definition.trait_impls {
                    add_nested_impl(
                        trait_impl.trait_name.as_str(),
                        &definition.name,
                        &trait_impl.methods,
                        module_path,
                        definition.is_pub || inherited_public,
                        definition.span,
                        file_path,
                        display_path,
                        source,
                        root,
                        output,
                        impls,
                    );
                }
            }
            Item::Distinct(definition) => add_item(
                output,
                "distinct",
                &definition.name,
                parent,
                module_path,
                definition.is_pub || inherited_public,
                definition.is_package_pub,
                definition.span,
                definition.name_span,
                &definition.type_markers,
                file_path,
                display_path,
                source,
                root,
            ),
            Item::TypeAlias(definition) => add_item(
                output,
                "type_alias",
                &definition.name,
                parent,
                module_path,
                definition.is_pub || inherited_public,
                definition.is_package_pub,
                definition.span,
                definition.name_span,
                &[],
                file_path,
                display_path,
                source,
                root,
            ),
            Item::UnitFamily(definition) => add_item(
                output,
                "unit_family",
                &definition.family,
                parent,
                module_path,
                definition.is_pub || inherited_public,
                definition.is_package_pub,
                definition.span,
                definition.family_span,
                &[],
                file_path,
                display_path,
                source,
                root,
            ),
            Item::Trait(definition) => {
                add_item(
                    output,
                    "trait",
                    &definition.name,
                    parent,
                    module_path,
                    definition.is_pub || inherited_public,
                    definition.is_package_pub,
                    definition.span,
                    definition.name_span,
                    &[],
                    file_path,
                    display_path,
                    source,
                    root,
                );
                let owner = qualified(parent, &definition.name);
                for method in &definition.methods {
                    add_item(
                        output,
                        "function",
                        &method.name,
                        &owner,
                        module_path,
                        definition.is_pub || inherited_public,
                        false,
                        method.span,
                        method.name_span,
                        &[],
                        file_path,
                        display_path,
                        source,
                        root,
                    );
                }
            }
            Item::Tag(definition) => add_item(
                output,
                "tag",
                &definition.name,
                parent,
                module_path,
                definition.is_pub || inherited_public,
                definition.is_package_pub,
                definition.span,
                definition.name_span,
                &[],
                file_path,
                display_path,
                source,
                root,
            ),
            Item::ProtocolDecl(definition) => add_item(
                output,
                "protocol",
                &definition.name,
                parent,
                module_path,
                definition.is_pub || inherited_public,
                definition.is_package_pub,
                definition.span,
                definition.name_span,
                &[],
                file_path,
                display_path,
                source,
                root,
            ),
            Item::EffectDecl(definition) => add_item(
                output,
                "effect",
                &definition.name,
                parent,
                module_path,
                false,
                false,
                definition.span,
                definition.name_span,
                &[],
                file_path,
                display_path,
                source,
                root,
            ),
            Item::Impl(definition) => {
                let trait_name = definition.trait_name.clone();
                let protocol_handle = definition
                    .type_name
                    .rsplit_once('.')
                    .is_some_and(|(_, endpoint)| matches!(endpoint, "Client" | "Server"));
                let kind = if protocol_handle
                    || trait_name
                        .as_deref()
                        .is_some_and(|name| protocol_names.contains(name))
                {
                    "protocol_impl"
                } else if trait_name.is_some() {
                    "trait_impl"
                } else {
                    "inherent_impl"
                };
                let source_span = definition.span;
                let mut methods = definition
                    .methods
                    .iter()
                    .map(|method| method.name.clone())
                    .collect::<Vec<_>>();
                methods.sort();
                impls.push(DocImpl {
                    kind: kind.to_string(),
                    type_name: definition.type_name.clone(),
                    trait_name: trait_name.clone(),
                    module: module_path.to_string(),
                    public: true,
                    methods,
                    source: source_for(root, file_path, display_path, source, source_span),
                });
                let owner = qualified(parent, &definition.type_name);
                for method in &definition.methods {
                    add_function(
                        method,
                        module_path,
                        &owner,
                        inherited_public,
                        file_path,
                        display_path,
                        source,
                        root,
                        output,
                    );
                }
            }
            Item::CodeModule(module) => {
                add_item(
                    output,
                    "module",
                    &module.name,
                    parent,
                    module_path,
                    module.is_pub || inherited_public,
                    module.is_package_pub,
                    module.span,
                    module.name_span,
                    &[],
                    file_path,
                    display_path,
                    source,
                    root,
                );
                if let Some(body) = &module.body {
                    collect_items(
                        body,
                        module_path,
                        &qualified(parent, &module.name),
                        module.is_pub || inherited_public,
                        file_path,
                        display_path,
                        source,
                        root,
                        protocol_names,
                        output,
                        impls,
                    );
                }
            }
            Item::GenericModule(module) => {
                add_item(
                    output,
                    "module",
                    &module.name,
                    parent,
                    module_path,
                    module.is_pub || inherited_public,
                    module.is_package_pub,
                    module.span,
                    module.name_span,
                    &[],
                    file_path,
                    display_path,
                    source,
                    root,
                );
                collect_items(
                    &module.body,
                    module_path,
                    &qualified(parent, &module.name),
                    module.is_pub || inherited_public,
                    file_path,
                    display_path,
                    source,
                    root,
                    protocol_names,
                    output,
                    impls,
                );
            }
            Item::ModuleAlias(definition) => add_item(
                output,
                "module_alias",
                &definition.name,
                parent,
                module_path,
                definition.is_pub || inherited_public,
                definition.is_package_pub,
                definition.span,
                definition.name_span,
                &[],
                file_path,
                display_path,
                source,
                root,
            ),
            Item::MarkerDecl(definition) => add_item(
                output,
                "marker",
                &definition.name,
                parent,
                module_path,
                true,
                false,
                definition.span,
                definition.name_span,
                &[],
                file_path,
                display_path,
                source,
                root,
            ),
            Item::FactDecl(definition) => add_item(
                output,
                "fact",
                &definition.name,
                parent,
                module_path,
                true,
                false,
                definition.span,
                definition.name_span,
                &[],
                file_path,
                display_path,
                source,
                root,
            ),
            Item::Const(definition) => add_item(
                output,
                "const",
                &definition.name,
                parent,
                module_path,
                false,
                false,
                definition.span,
                definition.name_span,
                &[],
                file_path,
                display_path,
                source,
                root,
            ),
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn add_nested_impl(
    trait_name: &str,
    type_name: &str,
    methods: &[jet_foundation::AST::Func],
    module_path: &str,
    public: bool,
    span: Span,
    file_path: &Path,
    display_path: &str,
    source: &str,
    root: &Path,
    output: &mut Vec<DocItem>,
    impls: &mut Vec<DocImpl>,
) {
    let mut method_names = methods
        .iter()
        .map(|method| method.name.clone())
        .collect::<Vec<_>>();
    method_names.sort();
    impls.push(DocImpl {
        kind: "trait_impl".to_string(),
        type_name: type_name.to_string(),
        trait_name: Some(trait_name.to_string()),
        module: module_path.to_string(),
        public,
        methods: method_names,
        source: source_for(root, file_path, display_path, source, span),
    });
    let owner = type_name.to_string();
    for method in methods {
        add_function(
            method,
            module_path,
            &owner,
            public,
            file_path,
            display_path,
            source,
            root,
            output,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn add_function(
    function: &jet_foundation::AST::Func,
    module_path: &str,
    parent: &str,
    inherited_public: bool,
    file_path: &Path,
    display_path: &str,
    source: &str,
    root: &Path,
    output: &mut Vec<DocItem>,
) {
    add_item(
        output,
        "function",
        &function.name,
        parent,
        module_path,
        function.is_pub || inherited_public,
        function.is_package_pub,
        function.span,
        function.name_span,
        &function.markers,
        file_path,
        display_path,
        source,
        root,
    );
}

#[allow(clippy::too_many_arguments)]
fn add_item(
    output: &mut Vec<DocItem>,
    kind: &str,
    name: &str,
    parent: &str,
    module_path: &str,
    public: bool,
    package_public: bool,
    span: Span,
    name_span: Span,
    markers: &[jet_foundation::AST::Marker],
    file_path: &Path,
    display_path: &str,
    source: &str,
    root: &Path,
) {
    let (summary, examples) = source_docs(source, name_span.start);
    output.push(DocItem {
        kind: kind.to_string(),
        name: name.to_string(),
        qualified_name: qualified(parent, name),
        module: module_path.to_string(),
        public,
        package_public,
        signature: declaration_text(source, span),
        failure_contract: None,
        failure_source: None,
        summary,
        examples,
        markers: markers.iter().map(|marker| marker.name.clone()).collect(),
        source: source_for(root, file_path, display_path, source, span),
    });
}

fn enrich_callable_contracts(items: &mut [DocItem], index: &SemIndex) {
    for item in items.iter_mut().filter(|item| item.kind == "function") {
        let Some(definition) = index.definitions().iter().find(|definition| {
            matches!(
                &definition.kind,
                crate::Types::SymbolKind::Function { .. }
            ) && definition.name == item.name
                && item.source.start <= definition.def_span.start
                && definition.def_span.end <= item.source.end
        }) else {
            continue;
        };
        let Some(signature) = &definition.callable_signature else {
            continue;
        };
        item.failure_contract = Some(signature.failure_contract.clone());
        item.failure_source = Some(signature.failure_source.clone());
        item.signature.push_str("\nfailure: ");
        item.signature.push_str(&signature.failure_contract);
        item.signature.push_str(" (");
        item.signature.push_str(&signature.failure_source);
        item.signature.push(')');
    }
}

fn discover_doctests(
    root: &Path,
    file_path: &Path,
    display_path: &str,
    source: &str,
    module: &str,
) -> Vec<DocTest> {
    let mut tests = Vec::new();
    let mut in_fence = false;
    let mut setup = Vec::new();
    let mut expectations = Vec::new();
    let mut fence_line = 0;
    for (index, raw_line) in source.lines().enumerate() {
        let line_number = index + 1;
        let Some(doc_line) = raw_line.trim().strip_prefix("///") else {
            if in_fence {
                in_fence = false;
                setup.clear();
                expectations.clear();
            }
            continue;
        };
        let doc_line = doc_line.trim();
        if !in_fence {
            if doc_line.starts_with("```") && doctest_fence_is_jet(doc_line) {
                in_fence = true;
                fence_line = line_number;
                setup.clear();
                expectations.clear();
            }
            continue;
        }
        if doc_line == "```" {
            let offset = line_offset(source, fence_line);
            tests.push(DocTest {
                module: module.to_string(),
                line: fence_line,
                setup: std::mem::take(&mut setup),
                expectations: std::mem::take(&mut expectations),
                source: source_for(
                    root,
                    file_path,
                    display_path,
                    source,
                    Span::new(offset, offset + raw_line.len()),
                ),
            });
            in_fence = false;
            continue;
        }
        if let Some(marker) = find_doctest_expect_marker(doc_line) {
            expectations.push(DocExpectation {
                expression: doc_line[..marker].trim().to_string(),
                expected: doc_line[marker + DOC_EXPECT_MARKER.len()..].trim().to_string(),
            });
        } else if !doc_line.is_empty() {
            setup.push(doc_line.to_string());
        }
    }
    tests
}

const DOC_EXPECT_MARKER: &str = "// =>";

fn doctest_fence_is_jet(fence: &str) -> bool {
    let language = fence.trim_start_matches('`').trim();
    language.is_empty() || language.eq_ignore_ascii_case("jet")
}

fn find_doctest_expect_marker(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '"' {
            in_string = true;
        } else if line[index..].starts_with(DOC_EXPECT_MARKER) {
            return Some(index);
        }
    }
    None
}

fn source_docs(source: &str, def_start: usize) -> (String, Vec<String>) {
    let prefix = &source[..def_start.min(source.len())];
    let before_declaration = prefix.rsplit_once('\n').map_or("", |(before, _)| before);
    let mut lines = Vec::new();
    for line in before_declaration.lines().rev() {
        let line = line.trim();
        let Some(doc) = line.strip_prefix("///") else {
            break;
        };
        lines.push(doc.trim().to_string());
    }
    lines.reverse();
    let mut summary = String::new();
    let mut examples = Vec::new();
    for line in lines {
        if let Some(example) = line.strip_prefix("Example:") {
            examples.push(example.trim().to_string());
        } else if summary.is_empty() && !line.is_empty() && !line.starts_with("```") {
            summary = line;
        }
    }
    (summary, examples)
}

fn leading_summary(source: &str) -> String {
    let mut summary = String::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(doc) = line.strip_prefix("///") else {
            if !line.is_empty() {
                break;
            }
            continue;
        };
        let doc = doc.trim();
        if summary.is_empty() && !doc.is_empty() && !doc.starts_with("```") {
            summary = doc.to_string();
        }
    }
    summary
}

fn source_for(root: &Path, file_path: &Path, display_path: &str, source: &str, span: Span) -> DocSource {
    let path = stable_module_path(root, file_path, display_path);
    let start = span.start.min(source.len());
    let end = span.end.min(source.len()).max(start);
    let (line, column) = line_column(source, start);
    DocSource {
        path: path.clone(),
        start,
        end,
        line,
        column,
        link: format!("{}#L{}", path, line),
    }
}

fn declaration_text(source: &str, span: Span) -> String {
    let start = span.start.min(source.len());
    let end = span.end.min(source.len()).max(start);
    let raw = &source[start..end];
    let raw = raw.split('{').next().unwrap_or(raw);
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn stable_root(_root: &Path) -> String {
    ".".to_string()
}

fn stable_module_path(root: &Path, path: &Path, display: &str) -> String {
    path.strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .filter(|relative| relative != ".")
        .unwrap_or_else(|| display.replace('\\', "/"))
}

fn module_name(path: &str, alias: &str) -> String {
    if !alias.is_empty() {
        return alias.to_string();
    }
    Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |name| name.to_string_lossy().into_owned())
}

fn qualified(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}::{name}")
    }
}

fn kind_rank(kind: &str) -> u8 {
    match kind {
        "module" => 0,
        "marker" | "fact" => 1,
        "protocol" | "trait" | "tag" | "effect" => 2,
        "struct" | "enum" | "distinct" | "type_alias" | "unit_family" => 3,
        "field" | "variant" => 4,
        "function" => 5,
        _ => 6,
    }
}

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.len() + 1, |(_, line)| line.len() + 1);
    (line, column)
}

fn line_offset(source: &str, line: usize) -> usize {
    source
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum()
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", jet_foundation::JSON::json_escape(value))
}

fn optional_json_string(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_string(), json_string)
}

fn json_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_string(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn docs_link(link: &str) -> String {
    format!("../{link}")
}
