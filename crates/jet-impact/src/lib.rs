//! D-IMPACT1: blast-radius queries over the stable semantic-index API.

#![allow(non_snake_case)]
#![deny(warnings)]

use jet_semindex::{CallEdge, SemIndex, SymbolDef, SymbolRef};
use std::collections::{HashSet, VecDeque};

/// One edge in the impact graph with transitive depth from the queried symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactEdge {
    pub depth: usize,
    pub caller: String,
    pub callee: String,
    pub module_path: String,
    pub span_start: usize,
    pub span_end: usize,
}

/// One reference site with transitive depth (always 1 for direct references).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactRef {
    pub depth: usize,
    pub name: String,
    pub module_path: String,
    pub span_start: usize,
    pub span_end: usize,
}

/// Blast-radius report for one symbol in a checked program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactReport {
    pub symbol: String,
    pub found: bool,
    pub depth_limit: usize,
    pub definition_module: Option<String>,
    pub references: Vec<ImpactRef>,
    pub call_sites: Vec<ImpactEdge>,
    pub upstream_callers: Vec<ImpactEdge>,
    pub downstream_callees: Vec<ImpactEdge>,
}

impl ImpactReport {
    pub fn analyze(index: &SemIndex, symbol: &str, depth_limit: usize) -> Self {
        let resolved = resolve_symbol(index, symbol);
        let found = resolved.is_some();
        let definition_module = resolved.map(|d| d.module_path.clone());

        let references = direct_references(index, symbol);
        let call_sites = direct_call_sites(index, symbol);
        let upstream_callers = if found {
            transitive_callers(index, symbol, depth_limit)
        } else {
            Vec::new()
        };
        let downstream_callees = if found {
            transitive_callees(index, symbol, depth_limit)
        } else {
            Vec::new()
        };

        ImpactReport {
            symbol: symbol.to_string(),
            found,
            depth_limit,
            definition_module,
            references,
            call_sites,
            upstream_callers,
            downstream_callees,
        }
    }

    pub fn to_json(&self) -> String {
        fn edge_json(e: &ImpactEdge) -> String {
            format!(
                "{{\"depth\":{},\"caller\":{},\"callee\":{},\"module_path\":{},\"span\":{{\"start\":{},\"end\":{}}}}}",
                e.depth,
                json_str(&e.caller),
                json_str(&e.callee),
                json_str(&e.module_path),
                e.span_start,
                e.span_end
            )
        }
        fn ref_json(r: &ImpactRef) -> String {
            format!(
                "{{\"depth\":{},\"name\":{},\"module_path\":{},\"span\":{{\"start\":{},\"end\":{}}}}}",
                r.depth,
                json_str(&r.name),
                json_str(&r.module_path),
                r.span_start,
                r.span_end
            )
        }
        let refs: Vec<String> = self.references.iter().map(ref_json).collect();
        let sites: Vec<String> = self.call_sites.iter().map(edge_json).collect();
        let up: Vec<String> = self.upstream_callers.iter().map(edge_json).collect();
        let down: Vec<String> = self.downstream_callees.iter().map(edge_json).collect();
        let def_mod = match &self.definition_module {
            Some(m) => json_str(m),
            None => "null".to_string(),
        };
        format!(
            "{{\"symbol\":{},\"found\":{},\"depth_limit\":{},\"definition_module\":{},\"references\":[{}],\"call_sites\":[{}],\"upstream_callers\":[{}],\"downstream_callees\":[{}]}}",
            json_str(&self.symbol),
            self.found,
            self.depth_limit,
            def_mod,
            refs.join(","),
            sites.join(","),
            up.join(","),
            down.join(",")
        )
    }

    pub fn render_text(&self) -> String {
        let mut out = String::new();
        if !self.found {
            out.push_str(&format!("symbol `{}` not found in index\n", self.symbol));
            if !self.references.is_empty() || !self.call_sites.is_empty() {
                out.push_str("partial matches from name references/calls:\n");
            } else {
                return out;
            }
        } else {
            out.push_str(&format!("blast radius for `{}`", self.symbol));
            if let Some(m) = &self.definition_module {
                out.push_str(&format!(" (defined in {m})"));
            }
            out.push('\n');
        }
        out.push_str(&format!("  references:        {}\n", self.references.len()));
        out.push_str(&format!("  direct call sites: {}\n", self.call_sites.len()));
        out.push_str(&format!(
            "  upstream callers:  {} (depth <= {})\n",
            self.upstream_callers.len(),
            self.depth_limit
        ));
        out.push_str(&format!(
            "  downstream callees:{} (depth <= {})\n",
            self.downstream_callees.len(),
            self.depth_limit
        ));
        for r in &self.references {
            out.push_str(&format!(
                "  ref  {}:{}..{}\n",
                r.module_path, r.span_start, r.span_end
            ));
        }
        for e in &self.call_sites {
            out.push_str(&format!(
                "  call {} -> {} @ {}:{}..{}\n",
                e.caller, e.callee, e.module_path, e.span_start, e.span_end
            ));
        }
        for e in &self.upstream_callers {
            out.push_str(&format!(
                "  upstream[d{}] {} -> {} @ {}\n",
                e.depth, e.caller, e.callee, e.module_path
            ));
        }
        for e in &self.downstream_callees {
            out.push_str(&format!(
                "  downstream[d{}] {} -> {} @ {}\n",
                e.depth, e.caller, e.callee, e.module_path
            ));
        }
        out
    }
}

fn resolve_symbol<'a>(index: &'a SemIndex, query: &str) -> Option<&'a SymbolDef> {
    if let Some((module, name)) = query.rsplit_once("::") {
        return index.definitions().iter().find(|d| {
            d.name == name && (d.module_path == module || d.module_path.ends_with(module))
        });
    }
    index.lookup(query)
}

fn direct_references(index: &SemIndex, symbol: &str) -> Vec<ImpactRef> {
    index
        .references_to(symbol)
        .into_iter()
        .map(|r| ref_to_impact(r, 1))
        .collect()
}

fn direct_call_sites(index: &SemIndex, symbol: &str) -> Vec<ImpactEdge> {
    index
        .call_sites(symbol)
        .into_iter()
        .map(|e| edge_to_impact(e, 1))
        .collect()
}

fn transitive_callers(index: &SemIndex, callee: &str, depth_limit: usize) -> Vec<ImpactEdge> {
    let mut out = Vec::new();
    let mut seen_edges: HashSet<(String, String)> = HashSet::new();
    let mut frontier: VecDeque<(String, usize)> = VecDeque::new();
    frontier.push_back((callee.to_string(), 0));

    while let Some((current, depth)) = frontier.pop_front() {
        if depth >= depth_limit {
            continue;
        }
        for edge in index.call_sites(&current) {
            let key = (edge.caller.clone(), edge.callee.clone());
            if seen_edges.insert(key) {
                let next_depth = depth + 1;
                out.push(edge_to_impact(edge, next_depth));
                frontier.push_back((edge.caller.clone(), next_depth));
            }
        }
    }
    out
}

fn transitive_callees(index: &SemIndex, caller: &str, depth_limit: usize) -> Vec<ImpactEdge> {
    let mut out = Vec::new();
    let mut seen_edges: HashSet<(String, String)> = HashSet::new();
    let mut frontier: VecDeque<(String, usize)> = VecDeque::new();
    frontier.push_back((caller.to_string(), 0));

    while let Some((current, depth)) = frontier.pop_front() {
        if depth >= depth_limit {
            continue;
        }
        for edge in index.call_edges() {
            if edge.caller != current {
                continue;
            }
            let key = (edge.caller.clone(), edge.callee.clone());
            if seen_edges.insert(key) {
                let next_depth = depth + 1;
                out.push(edge_to_impact(edge, next_depth));
                frontier.push_back((edge.callee.clone(), next_depth));
            }
        }
    }
    out
}

fn ref_to_impact(r: &SymbolRef, depth: usize) -> ImpactRef {
    ImpactRef {
        depth,
        name: r.name.clone(),
        module_path: r.module_path.clone(),
        span_start: r.span.start,
        span_end: r.span.end,
    }
}

fn edge_to_impact(e: &CallEdge, depth: usize) -> ImpactEdge {
    ImpactEdge {
        depth,
        caller: e.caller.clone(),
        callee: e.callee.clone(),
        module_path: e.module_path.clone(),
        span_start: e.call_span.start,
        span_end: e.call_span.end,
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
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
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jet_semindex::open;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/features")
            .join(name)
    }

    #[test]
    fn impact_finds_report_callers() {
        let idx = open(&fixture("effects/effects.jet")).expect("effects indexes");
        let report = ImpactReport::analyze(&idx, "report", 3);
        assert!(report.found);
        assert!(!report.call_sites.is_empty() || !report.upstream_callers.is_empty());
        assert!(report.upstream_callers.iter().any(|e| e.caller == "run"));
    }

    #[test]
    fn impact_json_shape() {
        let idx = open(&fixture("effects/effects.jet")).expect("effects indexes");
        let report = ImpactReport::analyze(&idx, "square", 2);
        let json = report.to_json();
        assert!(json.contains("\"symbol\":\"square\""));
        assert!(json.contains("\"references\""));
        assert!(json.contains("\"downstream_callees\""));
    }
}
