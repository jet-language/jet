//! D-BUILDNORM1=A — canonical, span/comment-free serialization of a parsed
//! program, for the content-addressed build cache (Tower #85).
//!
//! The build cache keys on `SHA256(canonical_bytes(bundle) + profile + version)`.
//! `canonical_bytes` turns the *pre-sema* [`ProgramBundle`] (post-parse,
//! post-import-resolution, before any sema/codegen) into a deterministic byte
//! string with these properties:
//!
//! - **whitespace-insensitive** and **comment-insensitive**: neither whitespace
//!   nor comments survive parsing (comments are never stored on any AST node),
//!   and byte-offset spans are stripped here — so reformatting or adding a
//!   comment does not change the key.
//! - **identifier-name-sensitive**: identifiers are kept exactly as written, so
//!   renaming a local changes the key.
//! - **child-order-sensitive**: children are serialized in source order, so
//!   reordering statements or operands changes the key.
//! - **whole-program**: every module in the bundle (the entry file *and* every
//!   imported module the loader pulled in) contributes, so a change in an
//!   imported module's source changes the key too. This is what makes a cache
//!   *hit* sound: an identical key means an identical parsed program across the
//!   entire import closure.
//!
//! ## Why this is sound under I2/I3
//!
//! A cache hit replays a previously-validated result, it does not bypass
//! validation. The key captures every input that determines the generated Rust:
//! the full parsed program (this module), the build profile, and the toolchain
//! version (folded in by [`ast_cache_key`]'s `jet_version` salt). A hit is only
//! ever *served* from an entry that a prior build *stored*, and storage only
//! happens for a build that ran the whole front end (sema + codegen) to success.
//! So a hit means: this exact program, under this exact toolchain and profile,
//! was already type-checked and compiled once — skipping the pipeline again is
//! replaying that verified compile, never letting unchecked code through.
//!
//! Inputs that live *outside* the parsed AST are handled by the caller, not
//! here: `embed_file`/`embed_bytes` bytes and the enclosing `pkg.jet` policy are
//! folded into the cache decision in `CmdCompile` (embed builds bypass the
//! cache; the manifest fingerprint rides the `jet_version` salt), so an
//! external-file or manifest change can never be masked by an identical AST.

use crate::AST::ProgramBundle;

/// Deterministic, span-free, comment-free serialization of the parsed program.
/// See the module docs for the exact contract.
pub fn canonical_bytes(bundle: &ProgramBundle) -> Vec<u8> {
    use std::fmt::Write;
    let mut s = String::new();
    // Which module is the program entry point is part of the program's meaning.
    let _ = write!(s, "entry:{}\u{1}", bundle.entry);
    // Modules in loader order (deterministic for a given import graph). Only the
    // AST *content* of each module is serialized — never its path, on-disk
    // display string, or raw source text (all whitespace-/location-dependent).
    for m in &bundle.modules {
        s.push_str(&m.alias);
        s.push('\u{1}');
        // Visibility + web-target markers change what codegen emits, so they are
        // part of the program's meaning. All are small deterministic values.
        let _ = write!(s, "{:?}", m.pub_file);
        s.push('\u{1}');
        let _ = write!(s, "{:?}", m.no_prelude);
        s.push('\u{1}');
        let _ = write!(s, "{:?}", m.web_target_ceiling);
        s.push('\u{1}');
        let _ = write!(s, "{:?}", m.html_path);
        s.push('\u{1}');
        let _ = write!(s, "{:?}", m.imports);
        s.push('\u{1}');
        let _ = write!(s, "{:?}", m.items);
        s.push('\u{2}');
    }
    strip_spans(&s).into_bytes()
}

/// Canonical bytes for an AST fragment. Debug output is used only as the
/// exhaustive structural encoder; all source-coordinate spans are removed.
pub fn canonical_fragment<T: std::fmt::Debug>(value: &T) -> Vec<u8> {
    strip_spans(&format!("{value:?}")).into_bytes()
}

/// The build-cache key for a parsed program: `SHA256(canonical_bytes + 0 +
/// profile_tag + 0 + jet_version)`, as 64 lowercase hex chars.
///
/// SHA-256 throughout, matching `Lock::LockEnvelope::output_hash`
/// (D-JPK-CACHE1=A / D-CASTORE1=A) so the local build cache and the hangar/lock
/// `output-hash` field are the same mechanism computed the same way — the
/// Epoch-6 substitution protocol can feed this value straight in by prefixing
/// `sha256-` (the lock's spelling); the raw-hex form here is the cache-directory
/// name (`~/.cache/jet/build/<key>/bin`).
///
/// `jet_version` is a toolchain-identity salt: a codegen change (new compiler
/// version) must not serve a stale binary for an identical AST. The caller
/// passes `Manifest::COMPILER_VERSION` (optionally combined with an enclosing
/// `pkg.jet` fingerprint) here.
pub fn ast_cache_key(bundle: &ProgramBundle, profile_tag: &str, jet_version: &str) -> String {
    let mut data = canonical_bytes(bundle);
    data.push(0);
    data.extend_from_slice(profile_tag.as_bytes());
    data.push(0);
    data.extend_from_slice(jet_version.as_bytes());
    crate::SHA256::sha256_hex(&data)
}

/// Remove every `Span { start: N, end: M }` rendering from a `Debug` string, so
/// that a byte-offset shift (from whitespace or a comment earlier in the file)
/// does not change the output — while never touching text *inside* a `Debug`
/// string or char literal. That exception matters for soundness: a string
/// literal whose contents happen to read `Span { start: 1, end: 2 }` must still
/// contribute its exact bytes to the key, so two programs differing only inside
/// such a literal produce different keys.
fn strip_spans(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    // Start of the pending run of bytes to copy verbatim.
    let mut copy_from = 0usize;
    let mut i = 0usize;
    const MARK: &[u8] = b"Span { start: ";
    while i < bytes.len() {
        match bytes[i] {
            // A Debug string ("…") or char ('…') literal: copy it through
            // untouched, honoring `\\` / `\"` / `\'` escapes so an embedded
            // quote never ends it early. Multibyte UTF-8 inside the literal is
            // preserved because we only ever slice the original `&str`.
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    if c == b'\\' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    if c == q {
                        break;
                    }
                }
            }
            // A span rendering outside any literal: flush the verbatim run,
            // emit the placeholder, and skip through the closing `}` (a `Span`
            // Debug never contains a nested `}`).
            b'S' if bytes[i..].starts_with(MARK) => {
                out.push_str(&input[copy_from..i]);
                out.push_str("Span");
                i += MARK.len();
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1; // consume the closing `}`
                }
                copy_from = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(&input[copy_from..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_spans_removes_span_renderings() {
        let a = "Ident(\"x\", Span { start: 0, end: 1 })";
        let b = "Ident(\"x\", Span { start: 40, end: 41 })";
        assert_eq!(strip_spans(a), strip_spans(b));
        assert_eq!(strip_spans(a), "Ident(\"x\", Span)");
    }

    #[test]
    fn strip_spans_keeps_span_lookalike_inside_string_literal() {
        // A string literal whose contents mimic a Span rendering must NOT be
        // stripped — otherwise two distinct programs would collide.
        let a = "Str([Lit(\"Span { start: 1, end: 2 }\")], Span { start: 5, end: 9 })";
        let b = "Str([Lit(\"Span { start: 7, end: 3 }\")], Span { start: 5, end: 9 })";
        assert_ne!(
            strip_spans(a),
            strip_spans(b),
            "literal contents must survive span-stripping"
        );
        // …but the trailing real span is still stripped from both.
        assert!(strip_spans(a).ends_with("], Span)"));
    }

    #[test]
    fn strip_spans_keeps_char_literal_with_quote() {
        // A char literal `'"'` contains a quote that must not be mistaken for a
        // string-literal boundary.
        let s = "Char('\"', Span { start: 0, end: 3 })";
        assert_eq!(strip_spans(s), "Char('\"', Span)");
    }
}
