use super::alloc_ptrs::{e3101, ptr_type};
use super::serde_diags::unknown_core_item;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::{json_ty, Checker};
use crate::Syntax;
use crate::AST::{Expr, Type};
impl<'a> Checker<'a> {
    pub(crate) fn infer_core_field(
        &mut self,
        alias: &str,
        module: &str,
        name: &str,
        alias_span: Span,
        span: Span,
    ) -> Option<Type> {
        // Direct type imports (`use core.encoding.[Dyn, DataEvent]`)
        // bind the type name as an alias to the owning Core module. Resolve
        // its constructors here instead of treating `Dyn.Text` as a
        // module item lookup.
        if module == "core.encoding"
            && alias == "Dyn"
            && matches!(name, "Null" | "Bool" | "Int" | "Float" | "Text" | "Bytes" | "DateTime" | "Array" | "Object")
        {
            self.record_import_alias_reference(alias, alias_span);
            return Some(json_ty());
        }
        // A grouped public Core type import names the source enum, not its
        // module. Query its declaring registry before the module-item route.
        if let Some(item) = self.core_item_imports.get(alias) {
            if let Some(owner) = self.core_source_enum_owner_module(module, item) {
                let has_variant = self.modules.is_some_and(|states|
                    states[owner].registry.enum_variants(item)
                        .is_some_and(|variants| variants.contains_key(name)));
                if has_variant {
                    let identity = self.canonical_nominal_name(owner, item);
                    self.record_import_alias_reference(alias, alias_span);
                    return Some(Type::Named(identity));
                }
            }
        }
        // Native enums retain their catalog leaf identity, not a source-module
        // nominal synthesized for an imported alias. Borrow the actual case
        // names instead of cloning a registry under that synthetic identity.
        if let Some(item) = self.core_item_imports.get(alias) {
            if self.core_source_enum_owner_module(module, item).is_none() {
                let variants = super::core_types::core_native_enum_variant_names(module, item);
                if variants.is_some_and(|variants| variants.contains(&name)) {
                    let identity = item.clone();
                    self.record_import_alias_reference(alias, alias_span);
                    return Some(Type::Named(identity));
                }
            }
        }

        if matches!(jet_foundation::CoreModuleExports::core_leaf_kind(module, name),
            Some(jet_foundation::CoreModuleExports::CoreLeafKind::Enum(_)))
        {
            if let Some(owner) = self.core_source_enum_owner_module(module, name) {
                return Some(Type::Named(self.canonical_nominal_name(owner, name)));
            }
        }
        let result = match (module, name) {
            ("core.math", "pi" | "e" | "tau" | "infinity" | "nan") => Some(Type::Float),
            // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): `mem.Arena`, `mem.Bump`,
            // `mem.Pool`, `mem.Fixed` — accessed as a field on the `core.mem` alias,
            // then `.new()` is called on the sentinel type to construct the allocator.
            ("core.mem", "Arena") => Some(Type::Named(Syntax::MEM_ARENA.to_string())),
            ("core.mem", "Bump") => Some(Type::Named(Syntax::MEM_BUMP.to_string())),
            ("core.mem", "Pool") => Some(Type::Named(Syntax::MEM_POOL.to_string())),
            ("core.mem", "Fixed") => Some(Type::Named(Syntax::MEM_FIXED.to_string())),
            ("core.mem", "AllocError") => Some(Type::Named(Syntax::TYPE_ALLOC_ERROR.to_string())),
            ("core.compute.solve", "Solver") => self
                .core_source_owner_module(module, name)
                .map(|owner| Type::Named(self.canonical_nominal_name(owner, name))),
            // D-GAME1/2/3 + D-WD10: static sentinels for `game.Scene.new`,
            // `game.Replay.record` and `game.Backend.headless`.
            ("core.game", "Scene") => Some(Type::Named("GameSceneType".to_string())),
            ("core.game", "Replay") => Some(Type::Named("GameReplayType".to_string())),
            ("core.game", "Backend") => Some(Type::Named("GameBackendType".to_string())),
            ("core.net.tls", "ClientConfig") => {
                Some(Type::Named("TLSClientConfigType".to_string()))
            }
            ("core.http.client", "Client") => Some(Type::Named("HTTPClientType".to_string())),
            ("core.http.client", "Proxy") => Some(Type::Named("HTTPProxy".to_string())),
            ("core.http.client", "RedirectPolicy") => {
                Some(Type::Named("HTTPRedirectPolicy".to_string()))
            }
            ("core.http.client", "CookieJar") => Some(Type::Named("HTTPCookieJar".to_string())),
            ("core.net.tls", "RootCertificates") => {
                Some(Type::Named("TLSRootCertificatesType".to_string()))
            }
            ("core.net.tls", "ClientIdentity") => {
                Some(Type::Named("TLSClientIdentityType".to_string()))
            }
            ("core.net.tls", "TLSVersion") => Some(Type::Named("TLSVersion".to_string())),
            // D-FIDELITY-API1=A: `core.perf.Perf` static API sentinel.
            ("core.perf", "Perf") => Some(Type::Named("Perf".to_string())),
            // D-ENCSTREAM-SURFACE1=A: shared encoding values are module fields on
            // `core.encoding`, then unit/payload constructors attach as Field/Call.
            ("core.encoding", "DataEvent") => Some(Type::Named("DataEvent".to_string())),
            ("core.encoding", "EncodingLimits") => Some(Type::Named("EncodingLimits".to_string())),
            ("core.encoding", "EncodingError") => Some(Type::Named("EncodingError".to_string())),
            ("core.encoding", "EncodingCause") => Some(Type::Named("EncodingCause".to_string())),
            ("core.encoding", "EncodingErrorKind") => {
                Some(Type::Named("EncodingErrorKind".to_string()))
            }
            ("core.encoding", "EncodingFormat") => {
                Some(Type::Named("EncodingFormat".to_string()))
            }
            ("core.encoding.json", "JSONReader" | "JSONWriter") => {
                Some(Type::Named(name.to_string()))
            }
            ("core.encoding.jsonl", "JSONLReader" | "JSONLWriter") => {
                Some(Type::Named(name.to_string()))
            }
            ("core.encoding.csv", "CSVReader" | "CSVWriter" | "CSVRow") => {
                Some(Type::Named(name.to_string()))
            }
            ("core.encoding.xml", "XMLReader" | "XMLWriter") => Some(Type::Named(name.to_string())),
            ("core.encoding.cbor", "CBORReader" | "CBORWriter") => {
                Some(Type::Named(name.to_string()))
            }
            _ => {
                self.diags.push(unknown_core_item(module, name, span));
                let _ = alias_span;
                None
            }
        };
        if result.is_some() {
            self.record_import_alias_reference(alias, alias_span);
        }
        result
    }

    /// S58 (E2-M13): `alias.Ptr<T>.from_addr(addr)`. Gated by `use core.mem`
    /// (E3102) and an enclosing `#Unsafe` block (E3101). Returns `Ptr<T>`.
    pub(crate) fn infer_ptr_from_addr(
        &mut self,
        alias: &str,
        alias_span: Span,
        elem: &Type,
        addr: &mut Expr,
        span: Span,
    ) -> Option<Type> {
        // E3102: the discovery gate — the alias must be a `core.mem` import.
        let is_mem = self
            .core_imports
            .get(alias)
            .map(|m| m == Syntax::CORE_MEM_MODULE)
            .unwrap_or(false);
        if !is_mem {
            self.diags.push(self.e3102(alias, alias_span));
            self.infer(addr);
            return None;
        }
        // E3101: pointer construction is a low-level operation; it needs the
        // audit gate.
        if Syntax::core_mem_requires_audit(Syntax::MEM_FROM_ADDR) && !self.in_unsafe {
            self.diags.push(e3101(Syntax::MEM_FROM_ADDR, span));
        }
        // The address is a plain Int.
        if let Some(t) = self.infer(addr) {
            if !matches!(&t, Type::Int | Type::InlineRange { .. }) {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "`{}` needs an Int address, not {}",
                        Syntax::MEM_FROM_ADDR,
                        t.show()
                    ),
                    "a pointer is built from a numeric machine address".to_string(),
                    "pass an Int, e.g. from `mem.address_of(x)`".to_string(),
                    Some(addr.span()),
                ));
            }
        }
        Some(ptr_type(elem.clone()))
    }

    /// E3102: a `core.mem` item was named without `use core.mem`.
    pub(crate) fn e3102(&self, alias: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            "E3102",
            format!("`{}` is part of the low-level tier", Syntax::TYPE_PTR),
            format!(
                "naming `{}`, `{}`, or an allocator needs the discovery gate",
                Syntax::TYPE_PTR,
                Syntax::MEM_VOLATILE_READ
            ),
            format!(
                "add `use {};` and call through `{}.…`",
                Syntax::CORE_MEM_MODULE,
                alias
            ),
            Some(span),
        )
    }
}
