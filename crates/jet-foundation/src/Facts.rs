//! Shared compile-time fact model (D-FACTMODEL1=A).

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FactKind {
    Effect,
    State,
    Tag,
}

impl FactKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Effect => "Effect",
            Self::State => "State",
            Self::Tag => "Tag",
        }
    }
}

/// Canonical closed effect roots. Effect parsing, rule arguments, fact
/// registration, diagnostics, and reflection all consume this list.
pub const EFFECT_ROOTS: &[&str] = &[
    "Net", "FS", "IO", "DB", "Time", "Rand", "Env", "Exec", "Log", "GPU", "Go", "Java",
    "DotNet", "Fortran", "Cobol", "Tcl", "Lua", "Ada", "Pascal", "Dart", "PowerShell",
    "Perl", "Ruby", "Php", "R", "Com", "Browser", "Secret",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactDeclaration {
    pub kind: FactKind,
    pub name: String,
    pub members: BTreeSet<String>,
    /// Destinations rejected by this fact. Used by tag facts; empty for other
    /// families.
    pub deny: BTreeSet<String>,
    /// Sources that introduce this fact. Used by tag facts; empty for other
    /// families.
    pub from: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FactRegistry {
    declarations: BTreeMap<(FactKind, String), FactDeclaration>,
}

impl FactRegistry {
    pub fn declare(
        &mut self,
        kind: FactKind,
        name: impl Into<String>,
        members: impl IntoIterator<Item = String>,
    ) {
        let name = name.into();
        self.declare_with_rules(kind, name, members, std::iter::empty(), std::iter::empty());
    }

    pub fn declare_with_rules(
        &mut self,
        kind: FactKind,
        name: impl Into<String>,
        members: impl IntoIterator<Item = String>,
        deny: impl IntoIterator<Item = String>,
        from: impl IntoIterator<Item = String>,
    ) {
        let name = name.into();
        self.declarations.insert(
            (kind, name.clone()),
            FactDeclaration {
                kind,
                name,
                members: members.into_iter().collect(),
                deny: deny.into_iter().collect(),
                from: from.into_iter().collect(),
            },
        );
    }

    pub fn get(&self, kind: FactKind, name: &str) -> Option<&FactDeclaration> {
        self.declarations.get(&(kind, name.to_string()))
    }

    pub fn declare_member(
        &mut self,
        kind: FactKind,
        name: impl Into<String>,
        member: impl Into<String>,
    ) {
        let name = name.into();
        let member = member.into();
        self.declarations
            .entry((kind, name.clone()))
            .or_insert_with(|| FactDeclaration {
                kind,
                name,
                members: BTreeSet::new(),
                deny: BTreeSet::new(),
                from: BTreeSet::new(),
            })
            .members
            .insert(member);
    }

    pub fn contains(&self, kind: FactKind, name: &str) -> bool {
        self.declarations.contains_key(&(kind, name.to_string()))
    }

    pub fn iter(&self) -> impl Iterator<Item = &FactDeclaration> {
        self.declarations.values()
    }

    pub fn iter_kind(&self, kind: FactKind) -> impl Iterator<Item = &FactDeclaration> {
        self.declarations.values().filter(move |declaration| declaration.kind == kind)
    }

    pub fn member(&self, kind: FactKind, declaration: &str, member: &str) -> bool {
        self.get(kind, declaration)
            .is_some_and(|fact| fact.members.contains(member))
    }
}

/// One subsumption rule for every hierarchical fact family.
pub fn fact_covers(bound: &str, fact: &str) -> bool {
    fact == bound
        || fact
            .strip_prefix(bound)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::{fact_covers, FactKind, FactRegistry};

    #[test]
    fn subsumption_uses_segment_boundaries() {
        assert!(fact_covers("FS", "FS"));
        assert!(fact_covers("FS", "FS.Read"));
        assert!(!fact_covers("FS", "FStore"));
        assert!(!fact_covers("FS.Read", "FS"));
    }

    #[test]
    fn fact_families_have_distinct_namespaces() {
        let mut facts = FactRegistry::default();
        facts.declare(FactKind::Effect, "Secret", std::iter::empty());
        facts.declare_with_rules(
            FactKind::Tag,
            "Secret",
            std::iter::empty(),
            ["Log".to_string()],
            std::iter::empty(),
        );

        assert_eq!(facts.get(FactKind::Effect, "Secret").unwrap().kind, FactKind::Effect);
        assert_eq!(facts.get(FactKind::Tag, "Secret").unwrap().deny.len(), 1);
        assert_eq!(facts.iter().count(), 2);
    }
}
