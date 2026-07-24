//! Thread-local package edition for sema, comptime, and codegen parity (D-REL3).

use std::cell::RefCell;

pub fn edition_year(edition: &str) -> u32 {
    edition.trim().parse().unwrap_or(2026)
}

pub fn edition_at_least(edition: &str, baseline: &str) -> bool {
    edition_year(edition) >= edition_year(baseline)
}

thread_local! {
    static PACKAGE_EDITION: RefCell<String> = RefCell::new("2026".to_string());
}

pub fn with_package_edition<R>(edition: &str, f: impl FnOnce() -> R) -> R {
    PACKAGE_EDITION.with(|cell| {
        let prev = cell.replace(edition.to_string());
        let out = f();
        *cell.borrow_mut() = prev;
        out
    })
}

pub fn package_edition() -> String {
    PACKAGE_EDITION.with(|cell| cell.borrow().clone())
}

pub fn package_edition_at_least(baseline: &str) -> bool {
    edition_at_least(&package_edition(), baseline)
}
