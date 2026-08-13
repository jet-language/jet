//! Readable Core prelude registry (D-NAME-ALIAS1=A).
//!
//! The source file is the one human-readable home for ambient names. This
//! module only parses its declarations into compiler facts; the facts do not
//! implement any runtime behavior.

use std::sync::LazyLock;

pub const SOURCE: &str = include_str!("../../jet-codegen/src/Prelude/core/prelude.jet");

/// The only epoch in which the current readable prelude is introduced.
///
/// A later prelude addition must move this value with the edition/epoch that
/// introduces it. The compiler can then keep older packages on their old
/// ambient set while the migration lint points at the new name.
pub const PRELUDE_EPOCH: &str = "2026";

/// Existing edition migration lint used for a prelude addition that is newer
/// than the package being checked. L0510 remains the local shadow warning.
pub const PRELUDE_MIGRATION_LINT: &str = "L2001";

/// D-CORE-PRELUDE1=A: the closed membership test for every ambient name.
pub const POLICY_CRITERIA: [&str; 7] = [
    "measured frequency",
    "total and safe",
    "names that never carry semantics",
    "no better home",
    "first-hour coverage",
    "one fixed set",
    "collision-conscious names",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    /// A name whose behavior remains in the embedded Prelude/CoreLib kernel.
    Builtin,
    /// A readable alias into the canonical Core tree.
    Core {
        module: &'static str,
        item: &'static str,
    },
    /// An ambient name whose existing comptime gate remains in force.
    Comptime,
    /// A type re-export from the canonical Core tree.
    Type,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entry {
    pub name: &'static str,
    pub target: Target,
}

static ENTRIES: LazyLock<Vec<Entry>> = LazyLock::new(|| parse(SOURCE));

pub fn entries() -> &'static [Entry] {
    ENTRIES.as_slice()
}

pub fn entry(name: &str) -> Option<&'static Entry> {
    entries().iter().find(|entry| entry.name == name)
}

pub fn names() -> impl Iterator<Item = &'static str> {
    entries().iter().map(|entry| entry.name)
}

/// A prelude addition is admitted only when the package edition moves forward.
pub fn addition_is_epoch_boundary(previous: &str, introduced: &str) -> bool {
    jet_foundation_epoch_year(introduced) > jet_foundation_epoch_year(previous)
}

/// Return the registered migration lint for an ambient name that is newer
/// than the package edition. The sema shadow warning remains L0510; this hook
/// is the edition gate for growing the fixed set.
pub fn migration_lint_for(
    name: &str,
    introduced: &str,
    package_edition: &str,
) -> Option<&'static str> {
    (entry(name).is_some()
        && jet_foundation_epoch_year(package_edition) < jet_foundation_epoch_year(introduced))
        .then_some(PRELUDE_MIGRATION_LINT)
}

/// Every name in the current registry belongs to the current prelude epoch.
/// Keeping this fact next to the parser makes an unannotated future addition
/// visible in review instead of silently changing an older package.
pub fn introduced_epoch(name: &str) -> Option<&'static str> {
    entry(name).map(|_| PRELUDE_EPOCH)
}

fn jet_foundation_epoch_year(edition: &str) -> u32 {
    edition.trim().parse().unwrap_or(2026)
}

fn parse(source: &'static str) -> Vec<Entry> {
    let mut entries = Vec::new();
    for raw_line in source.lines() {
        let line = raw_line.split("//").next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("pub fn ") {
            if let Some(name) = rest.split(['(', ' ', '{']).next().filter(|name| !name.is_empty()) {
                entries.push(Entry {
                    name,
                    target: Target::Builtin,
                });
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("pub use ") else {
            continue;
        };
        let Some((module, members)) = rest.split_once(".[") else {
            continue;
        };
        let Some(members) = members.strip_suffix(']') else {
            continue;
        };
        for member in members.split(',').map(str::trim).filter(|member| !member.is_empty()) {
            let mut names = member.split(" as ").map(str::trim);
            let original = names.next().unwrap_or_default();
            let local = names.next().unwrap_or(original);
            if original.is_empty() || local.is_empty() {
                continue;
            }
            let target = if original.chars().next().is_some_and(char::is_uppercase) {
                Target::Type
            } else if module == "core.comptime" {
                Target::Comptime
            } else {
                Target::Core {
                    module,
                    item: original,
                }
            };
            entries.push(Entry { name: local, target });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::{
        addition_is_epoch_boundary, entry, entries, introduced_epoch, migration_lint_for, names,
        Target, POLICY_CRITERIA, PRELUDE_EPOCH, PRELUDE_MIGRATION_LINT, SOURCE,
    };

    #[test]
    fn source_is_the_complete_ambient_registry() {
        assert_eq!(entries().len(), 19);
        assert_eq!(entry("print").map(|entry| entry.target), Some(Target::Builtin));
        assert_eq!(entry("assert_eq").map(|entry| entry.target), Some(Target::Builtin));
        assert_eq!(
            entry("read_file").map(|entry| entry.target),
            Some(Target::Core {
                module: "core.files",
                item: "read",
            })
        );
        assert_eq!(entry("embed_file").map(|entry| entry.target), Some(Target::Comptime));
        assert_eq!(entry("Clock").map(|entry| entry.target), Some(Target::Type));
        assert!(names().any(|name| name == "file_exists"));
    }

    #[test]
    fn policy_is_the_ratified_seven_part_gate() {
        assert_eq!(POLICY_CRITERIA.len(), 7);
        assert!(POLICY_CRITERIA.iter().all(|criterion| !criterion.is_empty()));
        assert!(addition_is_epoch_boundary("2026", "2027"));
        assert!(!addition_is_epoch_boundary("2026", "2026"));
    }

    #[test]
    fn every_current_name_has_an_epoch_and_old_packages_get_the_migration_lint() {
        for name in names() {
            assert_eq!(introduced_epoch(name), Some(PRELUDE_EPOCH));
        }
        assert_eq!(
            migration_lint_for("Path", "2027", "2026"),
            Some(PRELUDE_MIGRATION_LINT)
        );
        assert_eq!(migration_lint_for("Path", "2026", "2026"), None);
        assert_eq!(migration_lint_for("not_prelude", "2027", "2026"), None);
    }

    #[test]
    fn review_checklist_has_no_implicit_conversion_or_partial_entry() {
        assert!(!SOURCE.contains("impl "));
        assert!(!SOURCE.contains("convert"));
        assert!(!SOURCE.contains("into("));

        let total = [
            "print", "panic", "require", "assert", "assert_eq", "eprint", "file_exists",
        ];
        let result = ["input", "read_file", "write_file"];
        for entry in entries() {
            let covered = total.contains(&entry.name)
                || result.contains(&entry.name)
                || matches!(entry.target, Target::Type | Target::Comptime);
            assert!(covered, "unreviewed prelude entry: {}", entry.name);
        }
    }
}
