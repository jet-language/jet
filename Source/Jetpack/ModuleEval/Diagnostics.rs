//! The diagnostic constructors for module evaluation (E0966–E0978) plus the
//! merge-conflict (§6) wrapper (`merge_error_to_diagnostic`). Each carries its
//! pinned what/why/fix copy (I4).

use std::path::Path;

use crate::Diagnostics::{Diagnostic, Span};

use super::super::Merge::MergeError;

pub(super) fn bad_source_ref(ref_text: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E0968",
        format!("`{ref_text}` isn't a `provider@target` source ref"),
        "a named source resolves to an upstream written as `provider@target` (U6) — `github@owner/repo/rev`, `path@../local`, `nixpkgs@channel`".to_string(),
        "write the ref as `provider@target`, e.g. `github@NixOS/nixpkgs/nixos-24.05`".to_string(),
        span,
    )
}

/// E0969: an `imports:` directive must be `find("<dir>")` with a literal path.
pub(super) fn bad_import_directive(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0969",
        "an `imports:` directive must be `find(\"<dir>\")`".to_string(),
        "imports auto-discover a directory of modules (U4); the only directive is `find` with a single string-literal path, e.g. `find(\"./modules\")`".to_string(),
        "write `imports: find(\"./modules\")`".to_string(),
        Some(span),
    )
}

/// E0970: `imports: find("<dir>")` points at a directory that doesn't exist.
pub(super) fn find_dir_missing(dir: &Path, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0970",
        format!("`find` can't read the directory `{}`", dir.display()),
        "`imports: find(\"<dir>\")` walks that directory for `.jet` modules (U4); it must exist relative to this file".to_string(),
        "create the directory, or fix the path so it points at your modules folder".to_string(),
        Some(span),
    )
}

/// E0971: a discovered module imports — forbidden by the liftability law (U4):
/// modules contribute to the merged whole, they don't import each other.
pub(super) fn discovered_module_imports(file: &Path) -> Diagnostic {
    Diagnostic::error(
        "E0971",
        format!(
            "the discovered module `{}` has its own `imports:`",
            file.display()
        ),
        "a module found by `find(…)` may not import (the liftability law, U4) — modules only contribute to the merged whole, they never import each other; nesting `find` would make composition explode".to_string(),
        "remove the `imports:` from this module; declare all `find(…)` directives in the top-level env.jet".to_string(),
        None,
    )
}

pub(super) fn not_a_namespace_literal(expected: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        format!("a module contribution must be a `{expected}` literal"),
        format!(
            "a contribution's value describes its namespace with a typed struct literal, e.g. `env.dev: {expected}.{{…}}`"
        ),
        format!("wrap the value in `{expected}.{{…}}`"),
        Some(span),
    )
}

pub(super) fn wrong_namespace_type(expected: &str, got: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        format!("expected a `{expected}` literal here, found `{got}`"),
        format!("a contribution to this namespace must use the matching type `{expected}`"),
        format!("change `{got}.{{…}}` to `{expected}.{{…}}`"),
        Some(span),
    )
}

pub(super) fn packages_not_a_list(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        "the `packages` field must be a list literal".to_string(),
        "`packages: [ … ]` lists the packages this contribution adds, using the Pkg sugar (U6)"
            .to_string(),
        "write `packages: [ … ]`".to_string(),
        Some(span),
    )
}

/// E0972: an unknown field on a `System` / `Image` / `Service` record (U11/U14).
pub(super) fn unknown_record_field(ty: &str, field: &str, known: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0972",
        format!("`{field}` isn't a field of `{ty}`"),
        format!("a `{ty}` has a fixed set of fields: {known}"),
        format!("remove `{field}`, or use one of {known}"),
        Some(span),
    )
}

/// E0973: a `target:` (or cross-compile platform) names an unknown platform (U13).
pub(super) fn unknown_platform(os: &str, arch: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0973",
        format!("`{os}.{arch}` isn't a platform Jet knows"),
        "U13: a `target` is a typed platform value, not a piece of quoted text — it must be `linux.x64` or `linux.arm64`".to_string(),
        "write `target: linux.x64` or `target: linux.arm64`".to_string(),
        Some(span),
    )
}

/// E0974: a `System` with no `target` field (U11).
pub(super) fn missing_system_target(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0974",
        "this `System` has no `target`".to_string(),
        "U11: every machine names the platform it runs on with a typed `target` value".to_string(),
        "add `target: linux.x64` (or `linux.arm64`)".to_string(),
        Some(span),
    )
}

/// E0975: a `Service` record's `enable` field is not a yes/no value (U12).
pub(super) fn service_enable_not_bool(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0975",
        "a service's `enable` must be `true` or `false`".to_string(),
        "U12: a `Service` turns on or off with a yes/no `enable` flag".to_string(),
        "write `enable: true` or `enable: false`".to_string(),
        Some(span),
    )
}

/// E0975: a `Service` record with no `enable` field (U12).
pub(super) fn service_missing_enable(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0975",
        format!("the service `{name}` has no `enable`"),
        "U12: every `Service` says whether it is on with a required `enable` flag, then any further settings".to_string(),
        format!("add `enable: true` (or `false`) to `{name}`"),
        Some(span),
    )
}

/// E0976: an `Image` `format:` that isn't `iso` / `qcow` / `raw` (U14).
pub(super) fn image_bad_format(word: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0976",
        format!("`{word}` isn't a disk-image format"),
        "U14: an image is built as one of three formats — `iso`, `qcow`, or `raw`".to_string(),
        "write `format: iso`, `format: qcow`, or `format: raw`".to_string(),
        Some(span),
    )
}

/// E0977: an `Image` with no `from:` field (U14).
pub(super) fn image_missing_from(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0977",
        "this `Image` has no `from`".to_string(),
        "U14: an image is built from a system — `from: system.<name>` names which one".to_string(),
        "add `from: system.<name>`, e.g. `from: system.halcyon`".to_string(),
        Some(span),
    )
}

/// E0977: an `Image` restates a field it inherits from its system (U14).
pub(super) fn image_restated_field(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0977",
        format!("an image doesn't restate `{field}`"),
        "U14: `packages`, `services`, and `options` are inherited from the system the image is built from — they are written once on the system, never on the image".to_string(),
        format!("remove `{field}` from the image; set it on the system instead. (Only an explicit `target:` may be restated, for cross-compiling.)"),
        Some(span),
    )
}

/// E0978: an `Image` `from:` references a system that no contribution defines (U14).
pub(super) fn image_from_unknown_system(image: &str, system: &str, known: &[String]) -> Diagnostic {
    let hint = if known.is_empty() {
        "no `system.<name>:` contribution is defined".to_string()
    } else {
        format!("known systems: {}", known.join(", "))
    };
    Diagnostic::error(
        "E0978",
        format!("the image `{image}` is built from an unknown system `{system}`"),
        "U14: `from: system.<name>` must name a `System` defined by some module contribution; the image inherits that system's target, packages, services, and options".to_string(),
        format!("define `system.{system}: {{ … }}`, or point `from:` at an existing system ({hint})"),
        None,
    )
}

/// Wrap a merge conflict (§6) as a user-facing diagnostic (I4).
pub fn merge_error_to_diagnostic(err: &MergeError) -> Diagnostic {
    match err {
        MergeError::SourceConflict { name, a, b } => Diagnostic::error(
            "E0967",
            format!("source `{name}` is declared with two different refs"),
            format!("one module declares `{name}` as `{a}`, another as `{b}` — sources merge by name, so the refs must agree"),
            "make every declaration of this source agree, or rename one of them".to_string(),
            None,
        ),
        MergeError::ScalarConflict { key, values } => Diagnostic::error(
            "E0967",
            format!("`{key}` got conflicting values: {}", values.join(", ")),
            "scalar settings merge to one value; without a priority marker, modules contributing different values can't be reconciled".to_string(),
            "make every module agree on this value, or remove the conflicting contribution".to_string(),
            None,
        ),
    }
}
