use crate::AST::Type;
use crate::Diagnostics::Span;
use crate::Sema::Checker;
use super::core_types::is_json_type_name;
use super::serde_diags::{
    apply_serde_ok, e2411, sized_int_has_datatree_form,
};
impl<'a> Checker<'a> {
        fn serde_trait_impl(&self, name: &str, trait_name: &str) -> bool {
            if self.trait_reg.implements_trait(name, trait_name) {
                return true;
            }
            let Some(modules) = self.modules else { return false };
            if let Some((alias, leaf)) = name.split_once('.') {
                return self.imports.get(alias).is_some_and(|idx|
                    modules[*idx].trait_reg.implements_trait(leaf, trait_name)
                        && self.type_is_pub_in(*idx, leaf));
            }
            self.imports.values().any(|idx|
                modules[*idx].trait_reg.implements_trait(name, trait_name)
                    && self.type_is_pub_in(*idx, name))
        }

        fn serde_apply_ok(
            &self,
            name: &str,
            args: &[Type],
            trait_name: &str,
            elem_ok: &dyn Fn(&Type) -> bool,
        ) -> bool {
            if self.trait_reg.implements_trait(name, trait_name) {
                return apply_serde_ok(name, args, self.trait_reg, trait_name, elem_ok);
            }
            let Some(modules) = self.modules else { return false };
            let foreign = if let Some((alias, leaf)) = name.split_once('.') {
                self.imports.get(alias).and_then(|idx| {
                    self.type_is_pub_in(*idx, leaf).then_some((&modules[*idx].trait_reg, leaf))
                })
            } else {
                self.imports.values().find_map(|idx| {
                    self.type_is_pub_in(*idx, name).then_some((&modules[*idx].trait_reg, name))
                })
            };
            foreign.is_some_and(|(reg, leaf)|
                reg.implements_trait(leaf, trait_name)
                    && apply_serde_ok(leaf, args, reg, trait_name, elem_ok))
        }

        /// D-SERDE: a value type the `#[Codable]`/`#[Encode]` derive (or a blanket impl)
        /// can serialize. Primitives, the dynamic `JSON` tree, and lists/options/maps of
        /// encodables qualify; a user type must derive `Encode`.
        pub(crate) fn is_encodable(&self, t: &Type) -> bool {
            match t {
                Type::Int
                | Type::Float
                | Type::Bool
                | Type::String
                | Type::Char
                | Type::Float32 => true,
                // Codable's shared DataTree Int is i64. Admitting U64 here
                // would let sema promise a round trip that no codec lens can
                // preserve for values above i64::MAX.
                Type::IntN { .. } => sized_int_has_datatree_form(t),
                Type::List(e) | Type::Option(e) | Type::Shared(e) => self.is_encodable(e),
                Type::FixedList { elem, .. } => self.is_encodable(elem),
                Type::Map { key, value, .. } => matches!(**key, Type::String) && self.is_encodable(value),
                Type::Named(n) => {
                    n == "Decimal"
                        || n == "DataTree"
                        || is_json_type_name(n)
                        || self.serde_trait_impl(n, crate::Generics::ENCODE)
                        || self.type_param_scope.iter().any(|p|
                            p.name == *n && p.bounds.iter().any(|b| b == crate::Generics::ENCODE))
                }
                // D-SERDE9/10: a generic instantiation `Name<args>` is encodable when
                // `Name` derives Encode and every type arg that reaches the wire is
                // itself encodable. Phantom/skip-only params impose no obligation.
                Type::Apply { name, args } => {
                    self.serde_apply_ok(name, args, crate::Generics::ENCODE, &|t| {
                        self.is_encodable(t)
                    })
                }
                // D-UNIONTYPE1=A: every member must encode; ambiguity is a separate check.
                Type::Union(members) => members.iter().all(|m| self.is_encodable(m)),
                _ => false,
            }
        }
    
        /// D-SERDE: a type `decode<T>` can construct. Mirrors [`Self::is_encodable`] but a
        /// user type must derive `Decode` (the dynamic `JSON` tree is reached by bare
        /// `decode`, not the typed path).
        pub(crate) fn is_decodable(&self, t: &Type) -> bool {
            match t {
                Type::Int
                | Type::Float
                | Type::Bool
                | Type::String
                | Type::Char
                | Type::Float32 => true,
                Type::IntN { .. } => sized_int_has_datatree_form(t),
                Type::List(e) | Type::Option(e) | Type::Shared(e) => self.is_decodable(e),
                Type::FixedList { elem, .. } => self.is_decodable(elem),
                Type::Map { key, value, .. } => matches!(**key, Type::String) && self.is_decodable(value),
                Type::Named(n) => n == "Decimal"
                    || n == "DataTree"
                    || self.serde_trait_impl(n, crate::Generics::DECODE)
                    || self.type_param_scope.iter().any(|p|
                        p.name == *n && p.bounds.iter().any(|b| b == crate::Generics::DECODE)),
                Type::Apply { name, args } => {
                    self.serde_apply_ok(name, args, crate::Generics::DECODE, &|t| {
                        self.is_decodable(t)
                    })
                }
                Type::Union(members) => members.iter().all(|m| self.is_decodable(m)),
                _ => false,
            }
        }
    
        pub(crate) fn check_encodable(&mut self, t: &Type, span: Span) {
            if !self.is_encodable(t) {
                self.diags.push(e2411(&t, true, span));
            }
        }

        pub(crate) fn check_decodable(&mut self, t: &Type, span: Span) {
            if !self.is_decodable(t) {
                self.diags.push(e2411(&t, false, span));
            }
        }
    
}
