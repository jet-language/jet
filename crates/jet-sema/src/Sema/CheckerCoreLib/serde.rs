impl<'a> Checker<'a> {
        /// D-SERDE: a value type the `@[Codable]`/`@[Encode]` derive (or a blanket impl)
        /// can serialize. Primitives, the dynamic `Json` tree, and lists/options/maps of
        /// encodables qualify; a user type must derive `Encode`.
        fn is_encodable(&self, t: &Type) -> bool {
            match t {
                Type::Int
                | Type::Float
                | Type::Bool
                | Type::String
                | Type::Char
                | Type::IntN { .. }
                | Type::Float32 => true,
                Type::List(e) | Type::Option(e) | Type::Shared(e) => self.is_encodable(e),
                Type::FixedList { elem, .. } => self.is_encodable(elem),
                Type::Map { key, value } => matches!(**key, Type::String) && self.is_encodable(value),
                Type::Named(n) => {
                    is_json_type_name(n) || self.trait_reg.implements_trait(n, crate::Generics::ENCODE)
                }
                // D-SERDE9/10: a generic instantiation `Name<args>` is encodable when
                // `Name` derives Encode and every type arg that reaches the wire is
                // itself encodable. Phantom/skip-only params impose no obligation.
                Type::Apply { name, args } => {
                    apply_serde_ok(name, args, self.trait_reg, crate::Generics::ENCODE, &|t| {
                        self.is_encodable(t)
                    })
                }
                _ => false,
            }
        }
    
        /// D-SERDE: a type `decode<T>` can construct. Mirrors [`Self::is_encodable`] but a
        /// user type must derive `Decode` (the dynamic `Json` tree is reached by bare
        /// `decode`, not the typed path).
        fn is_decodable(&self, t: &Type) -> bool {
            match t {
                Type::Int
                | Type::Float
                | Type::Bool
                | Type::String
                | Type::Char
                | Type::IntN { .. }
                | Type::Float32 => true,
                Type::List(e) | Type::Option(e) | Type::Shared(e) => self.is_decodable(e),
                Type::FixedList { elem, .. } => self.is_decodable(elem),
                Type::Map { key, value } => matches!(**key, Type::String) && self.is_decodable(value),
                Type::Named(n) => self.trait_reg.implements_trait(n, crate::Generics::DECODE),
                Type::Apply { name, args } => {
                    apply_serde_ok(name, args, self.trait_reg, crate::Generics::DECODE, &|t| {
                        self.is_decodable(t)
                    })
                }
                _ => false,
            }
        }
    
        fn check_encodable(&mut self, t: &Type, span: Span) {
            if !self.is_encodable(t) {
                self.diags.push(e2411(&t.show(), true, span));
            }
        }
    
        fn check_decodable(&mut self, t: &Type, span: Span) {
            if !self.is_decodable(t) {
                self.diags.push(e2411(&t.show(), false, span));
            }
        }
    
}
